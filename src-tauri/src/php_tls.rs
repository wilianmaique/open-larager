//! Public PHP HTTPS trust. Local Caddy roots and private keys are never imported.
use crate::{
    error::{Result, invalid},
    platform,
    storage::{Storage, atomic_write},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

const BUNDLE: &str = include_str!("../certificates/mozilla.pem");
const NOTICE: &str = include_str!("../certificates/NOTICE.txt");

#[derive(Deserialize)]
struct BundleMetadata {
    revision: String,
    sha256: String,
    size: usize,
    certificates: usize,
}

fn metadata() -> Result<BundleMetadata> {
    let metadata: BundleMetadata = serde_json::from_str(include_str!("../catalog/ca-bundle.json"))?;
    if BUNDLE.len() != metadata.size
        || hex::encode(Sha256::digest(BUNDLE.as_bytes())) != metadata.sha256
        || BUNDLE.matches("-----BEGIN CERTIFICATE-----").count() != metadata.certificates
        || !valid_revision(&metadata.revision)
    {
        return Err(invalid(
            "The bundled Mozilla CA store failed integrity verification",
        ));
    }
    Ok(metadata)
}

fn valid_revision(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

pub fn bundle_path(storage: &Storage) -> Result<PathBuf> {
    Ok(storage
        .root
        .join("config/certificates")
        .join(format!("mozilla-{}.pem", metadata()?.revision)))
}

pub fn ensure_bundle(storage: &Storage) -> Result<PathBuf> {
    let metadata = metadata()?;
    let path = bundle_path(storage)?;
    let directory = path.parent().unwrap();
    let mut ancestor = directory;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| invalid("Invalid certificate directory"))?;
    }
    if !ancestor
        .canonicalize()?
        .starts_with(storage.root.canonicalize()?)
    {
        return Err(invalid(
            "Certificate directory points outside managed storage",
        ));
    }
    if path.exists() {
        if fs::metadata(&path)?.len() != metadata.size as u64
            || hex::encode(Sha256::digest(fs::read(&path)?)) != metadata.sha256
        {
            return Err(invalid(
                "The managed Mozilla CA file was modified. Restore it from the app; use a separate file for custom certificate authorities.",
            ));
        }
    } else {
        atomic_write(&path, BUNDLE.as_bytes())?;
    }
    let notice = directory.join("NOTICE.txt");
    if !notice.exists() {
        atomic_write(&notice, NOTICE.as_bytes())?;
    }
    Ok(path)
}

// Only parse the two scalar path directives. Leave other INI syntax untouched.
fn scalar(value: &str) -> &str {
    let mut quote = None;
    let mut end = value.len();
    for (index, character) in value.char_indices() {
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
        } else if character == ';' && quote.is_none() {
            end = index;
            break;
        }
    }
    let value = value[..end].trim();
    value
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|text| text.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn managed_value(storage: &Storage, value: &str) -> bool {
    let path = PathBuf::from(value);
    let resolved = path.canonicalize().unwrap_or(path);
    let Some(name) = resolved.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(revision) = name
        .strip_prefix("mozilla-")
        .and_then(|name| name.strip_suffix(".pem"))
    else {
        return false;
    };
    valid_revision(revision)
        && resolved
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            == storage.root.join("config/certificates").canonicalize().ok()
}

pub fn configure(storage: &Storage, before: &str) -> Result<String> {
    let bundle = ensure_bundle(storage)?;
    configure_with_bundle(storage, before, bundle)
}

pub(crate) fn default_files(storage: &Storage) -> Result<Vec<(PathBuf, String)>> {
    Ok(vec![
        (bundle_path(storage)?, BUNDLE.into()),
        (
            storage.root.join("config/certificates/NOTICE.txt"),
            NOTICE.into(),
        ),
    ])
}

pub(crate) fn configure_defaults(storage: &Storage, before: &str) -> Result<String> {
    configure_with_bundle(storage, before, bundle_path(storage)?)
}

fn configure_with_bundle(storage: &Storage, before: &str, bundle: PathBuf) -> Result<String> {
    // OpenSSL/libcurl in Windows PHP builds need an ANSI-compatible absolute path.
    let path = serde_json::to_string(&platform::runtime_path(&bundle)?)?;
    let newline = if before.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = before.split_inclusive('\n').map(str::to_owned).collect();
    let mut directives = [None, None];
    let mut scoped = false;
    for (index, line) in lines.iter().enumerate() {
        let line = line.trim();
        if let Some(section) = line.strip_prefix('[') {
            scoped = section.split_once('=').is_some_and(|(name, _)| {
                name.trim().eq_ignore_ascii_case("path") || name.trim().eq_ignore_ascii_case("host")
            });
        }
        if scoped || line.starts_with([';', '#']) {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            for (slot, name) in ["curl.cainfo", "openssl.cafile"].iter().enumerate() {
                if key.trim() == *name {
                    directives[slot] = Some((index, scalar(value).to_owned()));
                }
            }
        }
    }
    let mut appended = String::new();
    for (slot, (section, name)) in [("curl", "curl.cainfo"), ("openssl", "openssl.cafile")]
        .iter()
        .enumerate()
    {
        match &directives[slot] {
            Some((index, value)) if value != scalar(&path)
                && (value.trim().is_empty() || managed_value(storage, value)) => {
                lines[*index] = format!("{name} = {path}{newline}");
            }
            None => appended.push_str(&format!("{newline}[{section}]{newline}; Public HTTPS trust: Mozilla CA store (managed by Open Larager).{newline}{name} = {path}{newline}")),
            _ => {} // An explicit custom CA path belongs to the user.
        }
    }
    let mut after = lines.concat();
    if !appended.is_empty() {
        if !after.is_empty() && !after.ends_with('\n') {
            after.push_str(newline);
        }
        after.push_str(&appended);
    }
    Ok(after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustls::pki_types::{CertificateDer, pem::PemObject};

    #[test]
    fn pinned_bundle_contains_valid_public_trust_anchors() {
        let metadata = metadata().unwrap();
        let certs = CertificateDer::pem_slice_iter(BUNDLE.as_bytes())
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();
        let mut store = rustls::RootCertStore::empty();
        let (valid, invalid) = store.add_parsable_certificates(certs);
        assert_eq!((valid, invalid), (metadata.certificates, 0));
        assert!(!BUNDLE.contains("PRIVATE KEY"));
    }

    #[test]
    fn fills_effective_empty_values_preserves_custom_paths_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("environment")).unwrap();
        let before = "; user setting\r\nmemory_limit=512M\r\n[curl]\r\ncurl.cainfo=\"C:/corporate.pem\"\r\ncurl.cainfo = \"\" ; not configured\r\n[openssl]\r\nopenssl.cafile='C:/custom;roots.pem' ; preserve\r\n[PATH=/project]\r\ncurl.cainfo=\"C:/project.pem\"\r\n";
        let after = configure(&storage, before).unwrap();
        assert!(after.contains("memory_limit=512M\r\n"));
        assert!(after.contains("openssl.cafile='C:/custom;roots.pem' ; preserve\r\n"));
        assert!(after.contains("curl.cainfo=\"C:/project.pem\"\r\n"));
        assert!(after.contains("mozilla-"));
        assert_eq!(configure(&storage, &after).unwrap(), after);
    }

    #[test]
    fn adds_missing_globals_after_scoped_sections_and_rejects_modified_bundle() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("environment")).unwrap();
        let after = configure(&storage, "[HOST=example.test]\nopenssl.cafile=/custom.pem").unwrap();
        assert!(after.contains("\n[curl]\n"));
        assert!(after.contains("\n[openssl]\n"));
        assert_eq!(configure(&storage, &after).unwrap(), after);
        let bundle = ensure_bundle(&storage).unwrap();
        fs::write(&bundle, "untrusted replacement").unwrap();
        assert!(configure(&storage, &after).is_err());
        assert_eq!(fs::read_to_string(bundle).unwrap(), "untrusted replacement");
    }

    #[test]
    fn refreshes_only_managed_ca_paths_when_the_pinned_bundle_changes() {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path().join("environment")).unwrap();
        let old = storage
            .root
            .join("config/certificates/mozilla-2020-01-01.pem");
        atomic_write(&old, b"previous release bundle").unwrap();
        let old_path = serde_json::to_string(&platform::runtime_path(&old).unwrap()).unwrap();
        let before =
            format!("[curl]\ncurl.cainfo={old_path}\n[openssl]\nopenssl.cafile={old_path}\n");
        let after = configure(&storage, &before).unwrap();
        let current = platform::runtime_path(&bundle_path(&storage).unwrap()).unwrap();
        assert_eq!(after.matches(&current).count(), 2);
        assert_eq!(fs::read_to_string(old).unwrap(), "previous release bundle");
        assert_eq!(configure(&storage, &after).unwrap(), after);
    }
}
