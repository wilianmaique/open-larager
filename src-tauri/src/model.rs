use crate::error::{Result, invalid};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum ServiceId {
    Php,
    Apache,
    Mysql,
    Postgresql,
    Nginx,
    Redis,
    Mailpit,
    Caddy,
    Nodejs,
    Composer,
}

/// Shared with React. Only this compiled registry defines package capabilities.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceDefinition {
    pub daemon: bool,
    pub ports: Vec<u16>,
    pub executable: String,
    pub cli: String,
    pub config_name: Option<String>,
    pub dependencies: Vec<ServiceId>,
    pub required_files: Vec<String>,
}

impl ServiceId {
    pub const ALL: [Self; 10] = [
        Self::Php,
        Self::Apache,
        Self::Mysql,
        Self::Postgresql,
        Self::Nginx,
        Self::Redis,
        Self::Mailpit,
        Self::Caddy,
        Self::Nodejs,
        Self::Composer,
    ];
    pub fn key(self) -> &'static str {
        match self {
            Self::Php => "php",
            Self::Apache => "apache",
            Self::Mysql => "mysql",
            Self::Postgresql => "postgresql",
            Self::Nginx => "nginx",
            Self::Redis => "redis",
            Self::Mailpit => "mailpit",
            Self::Caddy => "caddy",
            Self::Nodejs => "nodejs",
            Self::Composer => "composer",
        }
    }
    pub fn definition(self) -> &'static ServiceDefinition {
        static DEFINITIONS: std::sync::OnceLock<BTreeMap<ServiceId, ServiceDefinition>> =
            std::sync::OnceLock::new();
        &DEFINITIONS.get_or_init(|| {
            serde_json::from_str(include_str!("../catalog/services.json"))
                .expect("valid compiled service registry")
        })[&self]
    }
    pub fn is_daemon(self) -> bool {
        self.definition().daemon
    }
    pub fn ports(self) -> Vec<u16> {
        self.definition().ports.clone()
    }
    pub fn executable(self) -> &'static str {
        &self.definition().executable
    }
    pub fn config_name(self) -> &'static str {
        self.definition().config_name.as_deref().unwrap_or("")
    }
    pub fn cli_executable(self) -> &'static str {
        &self.definition().cli
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PackageFormat {
    #[default]
    Zip,
    Phar,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Package {
    pub service: ServiceId,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub archive_root: String,
    pub source: String,
    pub source_url: String,
    pub size: u64,
    #[serde(default)]
    pub community: bool,
    pub verification: String,
    #[serde(default)]
    pub format: PackageFormat,
    #[serde(default)]
    pub bundled_tools: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub version: String,
    pub installed_at: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSettings {
    pub active_version: Option<String>,
    pub installations: Vec<Installation>,
    pub ports: Vec<u16>,
    pub auto_start: bool,
    #[serde(default)]
    pub add_to_path: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SiteMode {
    Php,
    Static,
    Proxy,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SiteServer {
    #[default]
    Caddy,
    Nginx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Site {
    pub id: String,
    pub hostname: String,
    pub root: PathBuf,
    pub mode: SiteMode,
    pub upstream_port: Option<u16>,
    pub enabled: bool,
    #[serde(default)]
    pub server: SiteServer,
    #[serde(default)]
    pub nginx_directives: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    #[serde(default)]
    pub revision: u64,
    pub language: String,
    pub theme: String,
    pub services: BTreeMap<ServiceId, ServiceSettings>,
    pub sites: Vec<Site>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            language: "auto".into(),
            theme: "system".into(),
            sites: Vec::new(),
            services: ServiceId::ALL
                .into_iter()
                .map(|id| {
                    (
                        id,
                        ServiceSettings {
                            active_version: None,
                            installations: Vec::new(),
                            ports: id.ports(),
                            auto_start: false,
                            add_to_path: false,
                        },
                    )
                })
                .collect(),
        }
    }
}

pub fn validate_version(version: &str) -> Result<()> {
    if version.is_empty()
        || version.len() > 48
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err(invalid("Invalid package version"));
    }
    Ok(())
}

pub fn validate_hostname(host: &str) -> Result<String> {
    let host = host.trim().to_ascii_lowercase();
    if host.len() > 253
        || !host.contains('.')
        || host.parse::<std::net::IpAddr>().is_ok()
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(invalid(
            "Use a hostname such as my-site.test, without a scheme, port, path or wildcard",
        ));
    }
    if host == "localhost" || host.ends_with(".localhost") {
        return Err(invalid("Use a .test hostname instead of localhost"));
    }
    Ok(host)
}

pub fn validate_ports(id: ServiceId, ports: &[u16], settings: &Settings) -> Result<()> {
    if ports.len() != id.ports().len() || ports.contains(&0) {
        return Err(invalid("Invalid service ports"));
    }
    let unique: std::collections::BTreeSet<_> = ports.iter().collect();
    if unique.len() != ports.len() {
        return Err(invalid("Each listener needs a different port"));
    }
    for (other, config) in &settings.services {
        if *other != id && config.ports.iter().any(|p| ports.contains(p)) {
            return Err(invalid(format!("Port reserved by {}", other.key())));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_capabilities_are_consistent() {
        for id in ServiceId::ALL {
            let definition = id.definition();
            assert_eq!(definition.daemon, !definition.ports.is_empty());
            assert_eq!(definition.daemon, definition.config_name.is_some());
            assert!(definition.required_files.contains(&definition.executable));
            for dependency in &definition.dependencies {
                assert!(ServiceId::ALL.contains(dependency));
            }
        }
        assert!(validate_ports(ServiceId::Nodejs, &[], &Settings::default()).is_ok());
        assert!(validate_ports(ServiceId::Composer, &[3000], &Settings::default()).is_err());
    }
    #[test]
    fn rejects_unsafe_domains_and_versions() {
        for host in [
            "https://app.test",
            "a.test\n127.0.0.1 evil.test",
            "*.test",
            "a..test",
            "-a.test",
            "127.0.0.1",
            "a.test:443",
        ] {
            assert!(validate_hostname(host).is_err(), "{host}");
        }
        assert_eq!(validate_hostname("My-Site.test").unwrap(), "my-site.test");
        for version in ["../foo", "C:evil", "a/b", "", "a\\b"] {
            assert!(validate_version(version).is_err());
        }
    }
    #[test]
    fn rejects_conflicting_ports() {
        let settings = Settings::default();
        assert!(validate_ports(ServiceId::Apache, &[3306], &settings).is_err());
        assert!(validate_ports(ServiceId::Mailpit, &[8025, 8025], &settings).is_err());
        assert!(validate_ports(ServiceId::Apache, &[8090], &settings).is_ok());
    }
}
