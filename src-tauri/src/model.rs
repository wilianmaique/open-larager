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
}

impl ServiceId {
    pub const ALL: [Self; 8] = [
        Self::Php,
        Self::Apache,
        Self::Mysql,
        Self::Postgresql,
        Self::Nginx,
        Self::Redis,
        Self::Mailpit,
        Self::Caddy,
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
        }
    }
    pub fn ports(self) -> Vec<u16> {
        match self {
            Self::Php => vec![9000],
            Self::Apache => vec![8080],
            Self::Mysql => vec![3306],
            Self::Postgresql => vec![5432],
            Self::Nginx => vec![8081],
            Self::Redis => vec![6379],
            Self::Mailpit => vec![1025, 8025],
            Self::Caddy => vec![80, 443, 2019],
        }
    }
    pub fn executable(self) -> &'static str {
        match self {
            Self::Php => "php-cgi.exe",
            Self::Apache => "bin/httpd.exe",
            Self::Mysql => "bin/mysqld.exe",
            Self::Postgresql => "bin/postgres.exe",
            Self::Nginx => "nginx.exe",
            Self::Redis => "redis-server.exe",
            Self::Mailpit => "mailpit.exe",
            Self::Caddy => "caddy.exe",
        }
    }
    pub fn config_name(self) -> &'static str {
        match self {
            Self::Php => "php.ini",
            Self::Apache => "httpd.conf",
            Self::Mysql => "my.ini",
            Self::Postgresql => "postgresql.conf",
            Self::Nginx => "nginx.conf",
            Self::Redis => "redis.conf",
            Self::Mailpit => "mailpit.json",
            Self::Caddy => "Caddyfile",
        }
    }
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
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SiteMode {
    Php,
    Static,
    Proxy,
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
