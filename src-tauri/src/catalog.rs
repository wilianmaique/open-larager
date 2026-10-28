use crate::{
    error::{Result, invalid},
    model::{Package, ServiceId, validate_version},
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    packages: Vec<Package>,
}

pub fn packages() -> Result<Vec<Package>> {
    let catalog: Catalog = serde_json::from_str(include_str!("../catalog/packages.json"))?;
    for package in &catalog.packages {
        validate_version(&package.version)?;
        if package.sha256.len() != 64 || !package.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("Invalid catalog digest"));
        }
        validate_download_url(&package.url)?;
        if !package.archive_root.is_empty() {
            crate::installer::safe_entry(&package.archive_root)?;
        }
        if package.format == crate::model::PackageFormat::Phar
            && (package.service != ServiceId::Composer || !package.archive_root.is_empty())
        {
            return Err(invalid("PHAR artifacts require the Composer adapter"));
        }
    }
    Ok(catalog.packages)
}

pub fn find(id: ServiceId, version: &str) -> Result<Package> {
    packages()?
        .into_iter()
        .find(|p| p.service == id && p.version == version)
        .ok_or_else(|| invalid("This version is not in the verified catalog"))
}

pub fn validate_download_url(url: &str) -> Result<()> {
    let url = reqwest::Url::parse(url).map_err(|_| invalid("Invalid download URL"))?;
    let allowed = [
        "downloads.php.net",
        "www.apachelounge.com",
        "nginx.org",
        "cdn.mysql.com",
        "get.enterprisedb.com",
        "github.com",
        "release-assets.githubusercontent.com",
        "objects.githubusercontent.com",
        "nodejs.org",
        "getcomposer.org",
    ];
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !allowed.contains(&url.host_str().unwrap_or_default())
    {
        return Err(invalid("Download origin is not allowed"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_covers_every_service_with_pinned_artifacts() {
        let packages = packages().unwrap();
        for id in ServiceId::ALL {
            assert!(packages.iter().any(|p| p.service == id), "{id:?}");
        }
        assert!(find(ServiceId::Php, "../../escape").is_err());
        assert!(validate_download_url("http://downloads.php.net/a.zip").is_err());
        assert!(validate_download_url("https://github.com.evil.test/a.zip").is_err());
    }
}
