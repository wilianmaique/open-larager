use crate::{
    catalog::validate_download_url,
    error::{Result, invalid},
    model::{Package, PackageFormat},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

const MAX_DOWNLOAD: u64 = 3 * 1024 * 1024 * 1024;
const MAX_EXTRACTED: u64 = 8 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub service: String,
    pub version: String,
    pub phase: String,
    pub downloaded: u64,
    pub total: Option<u64>,
}

pub async fn install(
    package: &Package,
    staging_root: &Path,
    destination: &Path,
    cancelled: Arc<AtomicBool>,
    progress: impl Fn(Progress) + Send + Sync,
) -> Result<()> {
    if destination.exists() {
        return Err(invalid(
            "Package directory already exists; remove the inactive installation before reinstalling",
        ));
    }
    let staging = staging_root.join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&staging)?;
    let result = install_inner(package, &staging, destination, cancelled, progress).await;
    let _ = fs::remove_dir_all(staging);
    result
}

async fn install_inner(
    package: &Package,
    staging: &Path,
    destination: &Path,
    cancelled: Arc<AtomicBool>,
    progress: impl Fn(Progress) + Send + Sync,
) -> Result<()> {
    validate_download_url(&package.url)?;
    let client = reqwest::Client::builder()
        .user_agent("OpenLarager/0.1")
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(1800))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 8 {
                return attempt.error("Too many redirects");
            }
            if validate_download_url(attempt.url().as_str()).is_err() {
                attempt.error("Untrusted redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()?;
    let mut response = client.get(&package.url).send().await?.error_for_status()?;
    let total = response.content_length();
    if total.is_some_and(|n| n > MAX_DOWNLOAD) {
        return Err(invalid("Archive is too large"));
    }
    let archive = staging.join("package.zip");
    let mut file = tokio::fs::File::create(&archive).await?;
    let mut hash = Sha256::new();
    let mut downloaded = 0;
    let mut last = Instant::now() - Duration::from_secs(1);
    while let Some(chunk) = response.chunk().await? {
        if cancelled.load(Ordering::Relaxed) {
            return Err(invalid("Installation cancelled"));
        }
        downloaded += chunk.len() as u64;
        if downloaded > MAX_DOWNLOAD {
            return Err(invalid("Archive exceeded size limit"));
        }
        file.write_all(&chunk).await?;
        hash.update(&chunk);
        if last.elapsed() > Duration::from_millis(120) {
            progress(Progress {
                service: package.service.key().into(),
                version: package.version.clone(),
                phase: "downloading".into(),
                downloaded,
                total,
            });
            last = Instant::now();
        }
    }
    file.sync_all().await?;
    drop(file);
    progress(Progress {
        service: package.service.key().into(),
        version: package.version.clone(),
        phase: "verifying".into(),
        downloaded,
        total,
    });
    if hex::encode(hash.finalize()) != package.sha256.to_ascii_lowercase() {
        return Err(invalid(
            "SHA-256 mismatch. The downloaded artifact has been discarded.",
        ));
    }
    let unpacked = staging.join("unpacked");
    let extract_to = unpacked.clone();
    let extract_cancel = cancelled.clone();
    progress(Progress {
        service: package.service.key().into(),
        version: package.version.clone(),
        phase: "extracting".into(),
        downloaded,
        total,
    });
    match package.format {
        PackageFormat::Zip => {
            tokio::task::spawn_blocking(move || extract(&archive, &extract_to, &extract_cancel))
                .await
                .map_err(|e| invalid(e.to_string()))??;
        }
        PackageFormat::Phar => {
            fs::create_dir_all(&unpacked)?;
            fs::rename(&archive, unpacked.join("composer.phar"))?;
        }
    }
    if cancelled.load(Ordering::Relaxed) {
        return Err(invalid("Installation cancelled"));
    }
    let content = unpacked.join(&package.archive_root);
    for required in &package.service.definition().required_files {
        if !content.join(required).is_file() {
            return Err(invalid(format!(
                "Expected package file is missing: {required}"
            )));
        }
    }
    fs::create_dir_all(
        destination
            .parent()
            .ok_or_else(|| invalid("Missing installation parent"))?,
    )?;
    fs::rename(content, destination)?;
    progress(Progress {
        service: package.service.key().into(),
        version: package.version.clone(),
        phase: "complete".into(),
        downloaded,
        total,
    });
    Ok(())
}

pub fn safe_entry(name: &str) -> Result<PathBuf> {
    let normalized = name.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') {
        return Err(invalid("Unsafe archive path"));
    }
    let mut path = PathBuf::new();
    for component in normalized.trim_end_matches('/').split('/') {
        let stem = component
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with(['.', ' '])
            || component.bytes().any(|b| b < 32)
            || [
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9",
            ]
            .contains(&stem.as_str())
        {
            return Err(invalid("Unsafe archive entry"));
        }
        path.push(component);
    }
    Ok(path)
}

fn extract(archive: &Path, destination: &Path, cancelled: &AtomicBool) -> Result<()> {
    fs::create_dir_all(destination)?;
    let mut zip = zip::ZipArchive::new(File::open(archive)?)?;
    if zip.len() > 150_000 {
        return Err(invalid("Too many archive entries"));
    }
    let mut total = 0;
    for i in 0..zip.len() {
        if cancelled.load(Ordering::Relaxed) {
            return Err(invalid("Installation cancelled"));
        }
        let mut entry = zip.by_index(i)?;
        if entry.is_symlink() {
            return Err(invalid("Archive links are not supported"));
        }
        let path = destination.join(safe_entry(&entry.name()?)?);
        total += entry.size();
        if total > MAX_EXTRACTED {
            return Err(invalid("Expanded archive exceeded size limit"));
        }
        if entry.is_dir() {
            fs::create_dir_all(&path)?;
            continue;
        }
        fs::create_dir_all(
            path.parent()
                .ok_or_else(|| invalid("Missing entry parent"))?,
        )?;
        let mut output = File::options().create_new(true).write(true).open(path)?;
        let mut buffer = [0; 65536];
        loop {
            if cancelled.load(Ordering::Relaxed) {
                return Err(invalid("Installation cancelled"));
            }
            let count = entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            output.write_all(&buffer[..count])?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_zip_slip_and_windows_device_paths() {
        for path in [
            "../x",
            "a/../../x",
            "C:/x",
            "\\\\server\\x",
            "/x",
            "a\\..\\x",
            "CON.txt",
            "a/COM1",
            "a/file:stream",
            "a./x",
            "a /x",
        ] {
            assert!(safe_entry(path).is_err(), "{path}");
        }
        assert_eq!(
            safe_entry("bin/httpd.exe").unwrap(),
            PathBuf::from("bin").join("httpd.exe")
        );
    }
    #[tokio::test]
    async fn rejects_untrusted_artifact_before_download() {
        let temp = tempfile::tempdir().unwrap();
        let pkg = Package {
            service: crate::model::ServiceId::Php,
            version: "1".into(),
            url: "https://evil.test/archive.zip".into(),
            sha256: "0".repeat(64),
            archive_root: "".into(),
            source: "test".into(),
            source_url: "".into(),
            size: 0,
            community: false,
            verification: "test".into(),
            format: PackageFormat::Zip,
            bundled_tools: Default::default(),
        };
        assert!(
            install(
                &pkg,
                temp.path(),
                &temp.path().join("live"),
                Arc::new(AtomicBool::new(false)),
                |_| {}
            )
            .await
            .is_err()
        );
        assert!(!temp.path().join("live").exists());
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 0);
    }
}
