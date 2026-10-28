use crate::{
    error::{Result, invalid},
    model::{Package, PackageFormat, ServiceId},
    platform,
    storage::Storage,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    pub id: String,
    pub module: String,
    pub source: String,
    #[serde(default)]
    pub zend: bool,
    #[serde(default)]
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub id: String,
    pub version: String,
    pub php_version: String,
    pub thread_safe: bool,
    pub compiler: String,
    pub architecture: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
    pub verification: String,
}

#[derive(Deserialize)]
pub struct Catalog {
    pub extensions: Vec<Definition>,
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub version: String,
    pub branch: String,
    pub architecture: String,
    pub thread_safe: bool,
    pub debug: bool,
    pub compiler: String,
    pub build: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub runtime: Runtime,
    pub modules: BTreeMap<String, Option<String>>,
    #[serde(default)]
    pub diagnostics: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Extension {
    pub id: String,
    pub source: String,
    pub loaded: bool,
    pub builtin: bool,
    pub compatible: bool,
    pub version: Option<String>,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub php_version: String,
    pub runtime: Runtime,
    pub extensions: Vec<Extension>,
    pub diagnostics: String,
}

pub fn catalog() -> Result<Catalog> {
    let catalog: Catalog = serde_json::from_str(include_str!("../catalog/php-extensions.json"))?;
    let mut ids = std::collections::BTreeSet::new();
    for definition in &catalog.extensions {
        if definition.id.is_empty()
            || !definition
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'_' || b.is_ascii_digit())
            || !ids.insert(&definition.id)
            || !["bundled", "pecl"].contains(&definition.source.as_str())
        {
            return Err(invalid("Invalid PHP extension catalog"));
        }
        for dependency in &definition.dependencies {
            if !catalog
                .extensions
                .iter()
                .any(|entry| &entry.id == dependency && entry.id != definition.id)
            {
                return Err(invalid("Invalid PHP extension dependency"));
            }
        }
    }
    let mut builds = std::collections::BTreeSet::new();
    for artifact in &catalog.artifacts {
        crate::model::validate_version(&artifact.version)?;
        crate::catalog::validate_download_url(&artifact.url)?;
        let url =
            reqwest::Url::parse(&artifact.url).map_err(|_| invalid("Invalid extension URL"))?;
        let file = format!(
            "php_{}-{}-{}-{}-{}-{}.zip",
            artifact.id,
            artifact.version,
            artifact.php_version,
            if artifact.thread_safe { "ts" } else { "nts" },
            artifact.compiler,
            artifact.architecture
        );
        if url.host_str() != Some("downloads.php.net")
            || url.path()
                != format!(
                    "/~windows/pecl/releases/{}/{}/{}",
                    artifact.id, artifact.version, file
                )
            || artifact.sha256.len() != 64
            || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || !catalog
                .extensions
                .iter()
                .any(|entry| entry.id == artifact.id && entry.source == "pecl")
            || !builds.insert((
                &artifact.id,
                &artifact.php_version,
                artifact.thread_safe,
                &artifact.compiler,
                &artifact.architecture,
            ))
        {
            return Err(invalid("Invalid PHP extension artifact"));
        }
    }
    Ok(catalog)
}

impl Artifact {
    pub fn matches(&self, runtime: &Runtime) -> bool {
        !runtime.debug
            && self.php_version == runtime.branch
            && self.thread_safe == runtime.thread_safe
            && self.compiler == runtime.compiler
            && self.architecture == runtime.architecture
    }
    pub fn package(&self) -> Package {
        Package {
            service: ServiceId::Php,
            version: self.version.clone(),
            url: self.url.clone(),
            sha256: self.sha256.clone(),
            archive_root: String::new(),
            source: "PHP PECL".into(),
            source_url: "https://pecl.php.net/".into(),
            size: self.size,
            community: false,
            verification: self.verification.clone(),
            format: PackageFormat::Zip,
            bundled_tools: Default::default(),
        }
    }
    pub fn directory(&self, storage: &Storage, version: &str) -> PathBuf {
        storage
            .root
            .join("packages/php-extensions")
            .join(version)
            .join(&self.id)
            .join(&self.sha256)
    }
}

pub fn dll(definition: &Definition) -> String {
    format!("php_{}.dll", definition.id)
}

// No user-supplied PHP, shell command or executable is accepted by IPC.
const SCRIPT: &str = r#"ob_start(); phpinfo(INFO_GENERAL); $info=ob_get_clean(); preg_match('/^PHP Extension Build => (.+)$/m', $info, $match); $build=trim($match[1]??''); $parts=explode(',', $build); $modules=[]; foreach(get_loaded_extensions() as $name) { $modules[strtolower($name)]=phpversion($name)?:null; } echo "LARAGER_PHP_PROBE\n",json_encode(['runtime'=>['version'=>PHP_VERSION,'branch'=>PHP_MAJOR_VERSION.'.'.PHP_MINOR_VERSION,'architecture'=>PHP_INT_SIZE===8?'x64':'x86','threadSafe'=>(bool)PHP_ZTS,'debug'=>(bool)PHP_DEBUG,'compiler'=>strtolower(end($parts)),'build'=>$build],'modules'=>$modules],JSON_THROW_ON_ERROR);"#;

pub async fn probe(storage: &Storage, version: &str, ini: Option<&Path>) -> Result<Probe> {
    let package = storage.package(ServiceId::Php, version);
    let mut command = platform::command(package.join("php.exe"));
    command.current_dir(&package);
    match ini {
        Some(path) => {
            command.arg("-c").arg(platform::runtime_path(path)?);
        }
        None => {
            command.arg("-n");
        }
    }
    command.args([
        "-d",
        "display_errors=stderr",
        "-d",
        "display_startup_errors=1",
        "-d",
        "log_errors=0",
        "-d",
        "opcache.enable_cli=1",
        "-r",
        SCRIPT,
    ]);
    let output = platform::checked_output(command, Duration::from_secs(30)).await?;
    parse_probe(&output)
}

fn parse_probe(output: &str) -> Result<Probe> {
    let (before, json) = output.split_once("LARAGER_PHP_PROBE\n").ok_or_else(|| {
        invalid(format!(
            "PHP inspection failed: {}",
            output.chars().take(2000).collect::<String>()
        ))
    })?;
    let mut stream = serde_json::Deserializer::from_str(json).into_iter::<Probe>();
    let mut probe = stream
        .next()
        .ok_or_else(|| invalid("PHP inspection returned no data"))??;
    probe.diagnostics = format!("{}{}", before, &json[stream.byte_offset()..])
        .trim()
        .to_string();
    if probe.runtime.build.is_empty() {
        return Err(invalid("PHP did not report its extension ABI"));
    }
    Ok(probe)
}

pub fn inventory(
    storage: &Storage,
    version: &str,
    base: &Probe,
    current: Probe,
) -> Result<Inventory> {
    let catalog = catalog()?;
    let mut extensions = Vec::new();
    for definition in catalog.extensions {
        let module = definition.module.to_ascii_lowercase();
        let builtin = base.modules.contains_key(&module);
        let artifact = catalog
            .artifacts
            .iter()
            .find(|artifact| artifact.id == definition.id && artifact.matches(&base.runtime));
        let compatible = builtin
            || if definition.source == "bundled" {
                storage
                    .package(ServiceId::Php, version)
                    .join("ext")
                    .join(dll(&definition))
                    .is_file()
            } else {
                artifact.is_some()
            };
        extensions.push(Extension {
            id: definition.id,
            source: if builtin {
                "builtin".into()
            } else {
                definition.source.clone()
            },
            loaded: current.modules.contains_key(&module),
            builtin,
            compatible,
            version: current
                .modules
                .get(&module)
                .cloned()
                .flatten()
                .or_else(|| artifact.map(|a| a.version.clone())),
            dependencies: definition.dependencies,
        });
    }
    // Report compiled-in extensions too; they never offer a remove action.
    for (module, module_version) in &base.modules {
        if !extensions.iter().any(|entry| entry.id == module_id(module)) {
            extensions.push(Extension {
                id: module.clone(),
                source: "builtin".into(),
                loaded: true,
                builtin: true,
                compatible: true,
                version: module_version.clone(),
                dependencies: Vec::new(),
            });
        }
    }
    extensions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(Inventory {
        php_version: version.into(),
        runtime: base.runtime.clone(),
        extensions,
        diagnostics: current.diagnostics,
    })
}

fn module_id(module: &str) -> String {
    if module == "zend opcache" {
        "opcache".into()
    } else {
        module.to_ascii_lowercase()
    }
}

fn directive(line: &str) -> Option<(&str, String)> {
    let (key, value) = line.trim().split_once('=')?;
    let key = key.trim();
    if !key.eq_ignore_ascii_case("extension") && !key.eq_ignore_ascii_case("zend_extension") {
        return None;
    }
    let value = value.trim();
    let value = if let Some(quote) = value.chars().next().filter(|c| *c == '\'' || *c == '"') {
        let rest = &value[1..];
        &rest[..rest.find(quote)?]
    } else {
        value.split(';').next()?.trim()
    };
    // Existing configurations may contain Windows 8.3 aliases for the DLL itself.
    let value = if value.contains('~') {
        fs::canonicalize(value)
            .map(|path| crate::storage::slash(&path))
            .unwrap_or_else(|_| value.replace('\\', "/"))
    } else {
        value.replace('\\', "/")
    };
    let name = value.rsplit('/').next()?.to_ascii_lowercase();
    let name = name.strip_prefix("php_").unwrap_or(&name);
    Some((key, name.strip_suffix(".dll").unwrap_or(name).to_string()))
}

/// Preserve comments and unrelated settings, including the user's extension_dir.
pub fn configure(content: &str, definition: &Definition, path: Option<&Path>) -> Result<String> {
    if content.len() > 1024 * 1024 {
        return Err(invalid("Configuration exceeds 1 MiB"));
    }
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = content
        .lines()
        .map(|line| {
            if directive(line).is_some_and(|(_, id)| id == definition.id) {
                format!("; Open Larager: {line}")
            } else {
                line.to_owned()
            }
        })
        .collect();
    if let Some(path) = path {
        // Convert only the directory: shortening php_xdebug.dll to PHP_XD~1.DLL
        // would hide the extension's identity from future configuration edits.
        let directory = path
            .parent()
            .ok_or_else(|| invalid("Invalid extension directory"))?;
        let filename = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| invalid("Invalid extension filename"))?;
        let path = format!("{}/{filename}", platform::runtime_path(directory)?);
        if path.contains(['"', '\n', '\r']) {
            return Err(invalid("Invalid extension path"));
        }
        let setting = format!(
            "{}=\"{}\"",
            if definition.zend {
                "zend_extension"
            } else {
                "extension"
            },
            path
        );
        // OPcache must load before Xdebug, including manually configured Xdebug.
        if definition.id == "opcache"
            && let Some(index) = lines
                .iter()
                .position(|line| directive(line).is_some_and(|(_, id)| id == "xdebug"))
        {
            lines.insert(index, setting);
        } else {
            lines.extend([String::new(), "[PHP]".into(), setting]);
        }
    }
    Ok(lines.join(newline) + newline)
}

pub fn has_module(probe: &Probe, definition: &Definition) -> bool {
    probe
        .modules
        .contains_key(&definition.module.to_ascii_lowercase())
}

pub fn assert_clean(probe: &Probe) -> Result<()> {
    if !probe.diagnostics.is_empty() {
        return Err(invalid(format!(
            "PHP extension validation failed: {}",
            probe.diagnostics.chars().take(4000).collect::<String>()
        )));
    }
    Ok(())
}

pub fn verify_cached(
    artifact: &Artifact,
    destination: &Path,
    definition: &Definition,
) -> Result<()> {
    if !destination.join(dll(definition)).is_file()
        || fs::read_to_string(destination.join(".larager-artifact"))
            .ok()
            .as_deref()
            != Some(&artifact.sha256)
    {
        return Err(invalid(
            "The cached PHP extension is incomplete or belongs to another artifact",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspection_preserves_startup_warnings_even_with_valid_json() {
        let json = serde_json::json!({
            "runtime": { "version": "8.4.0", "branch": "8.4", "architecture": "x64", "threadSafe": false, "debug": false, "compiler": "vs17", "build": "API20240924,NTS,VS17" },
            "modules": { "json": "8.4.0" }
        });
        let clean = parse_probe(&format!("LARAGER_PHP_PROBE\n{json}")).unwrap();
        assert_clean(&clean).unwrap();
        for output in [
            format!("PHP Warning: Unable to load DLL\nLARAGER_PHP_PROBE\n{json}"),
            format!("LARAGER_PHP_PROBE\n{json}\nPHP Warning: module already loaded"),
        ] {
            let probe = parse_probe(&output).unwrap();
            assert!(assert_clean(&probe).is_err());
            assert!(probe.modules.contains_key("json"));
        }
        assert!(parse_probe("PHP Fatal error").is_err());
    }
    #[test]
    fn catalog_is_pinned_and_abi_matching_is_strict() {
        let catalog = catalog().unwrap();
        assert!(!catalog.artifacts.is_empty());
        for artifact in catalog.artifacts {
            let runtime = Runtime {
                version: format!("{}.99", artifact.php_version),
                branch: artifact.php_version.clone(),
                architecture: artifact.architecture.clone(),
                thread_safe: artifact.thread_safe,
                debug: false,
                compiler: artifact.compiler.clone(),
                build: "API,NTS,VS17".into(),
            };
            assert!(artifact.matches(&runtime));
            assert!(!artifact.matches(&Runtime {
                branch: "9.0".into(),
                ..runtime.clone()
            }));
            assert!(!artifact.matches(&Runtime {
                thread_safe: !runtime.thread_safe,
                ..runtime.clone()
            }));
            assert!(!artifact.matches(&Runtime {
                compiler: "vs99".into(),
                ..runtime.clone()
            }));
            assert!(!artifact.matches(&Runtime {
                architecture: "x86".into(),
                ..runtime.clone()
            }));
            assert!(!artifact.matches(&Runtime {
                debug: true,
                ..runtime
            }));
        }
    }
    #[test]
    fn ini_edits_preserve_user_settings_and_remove_duplicates() {
        let catalog = catalog().unwrap();
        let definition = catalog.extensions.iter().find(|e| e.id == "intl").unwrap();
        let content = "; my settings\r\n[PHP]\r\nmemory_limit=512M\r\nextension_dir=custom\r\nextension=php_intl.dll\r\nEXTENSION = 'C:\\old\\PHP_INTL.dll' ; old\r\n;extension=intl\r\nextension=gd\r\n";
        let enabled = configure(
            content,
            definition,
            Some(Path::new("C:/PHP/ext/php_intl.dll")),
        )
        .unwrap();
        assert_eq!(
            enabled
                .lines()
                .filter(|line| directive(line).is_some_and(|(_, id)| id == "intl"))
                .count(),
            1
        );
        assert!(enabled.contains("memory_limit=512M\r\nextension_dir=custom\r\n"));
        let disabled = configure(&enabled, definition, None).unwrap();
        assert!(
            !disabled
                .lines()
                .any(|line| directive(line).is_some_and(|(_, id)| id == "intl"))
        );
        assert!(disabled.contains("extension=gd\r\n"));
    }
    #[test]
    fn zend_extensions_use_correct_directive_and_load_order() {
        let catalog = catalog().unwrap();
        let opcache = catalog
            .extensions
            .iter()
            .find(|e| e.id == "opcache")
            .unwrap();
        let content = "[PHP]\nzend_extension=php_xdebug.dll\n";
        let updated = configure(
            content,
            opcache,
            Some(Path::new("C:/PHP/ext/php_opcache.dll")),
        )
        .unwrap();
        assert!(updated.find("php_opcache.dll").unwrap() < updated.find("php_xdebug.dll").unwrap());
    }
}
