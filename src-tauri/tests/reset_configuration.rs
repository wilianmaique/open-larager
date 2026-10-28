use larager::{
    engine::Engine,
    model::{CloseBehavior, Installation, ServiceId, Site, SiteMode, SiteServer},
    storage::atomic_write,
};
use std::{collections::BTreeMap, fs, path::Path};

fn write(path: &Path, bytes: &[u8]) {
    atomic_write(path, bytes).unwrap();
}

async fn configured(root: &Path) -> Engine {
    let engine = Engine::open(root.to_owned()).unwrap();
    let mut settings = engine.settings.write().await;
    settings.language = "pt-BR".into();
    settings.theme = "dark".into();
    settings.close_behavior = CloseBehavior::Exit;
    for id in ServiceId::ALL {
        let version = "1.0.0";
        engine.storage.prepare(id, version).unwrap();
        let package = engine.storage.package(id, version);
        write(&package.join(id.executable()), b"package fixture");
        if id == ServiceId::Php {
            write(&package.join("php.exe"), b"cli fixture");
            fs::create_dir_all(package.join("ext")).unwrap();
        }
        if id == ServiceId::Nodejs {
            for script in ["npm-cli.js", "npx-cli.js"] {
                write(
                    &package.join("node_modules/npm/bin").join(script),
                    b"script fixture",
                );
            }
        }
        let service = settings.services.get_mut(&id).unwrap();
        service.active_version = Some(version.into());
        service.installations.push(Installation {
            version: version.into(),
            installed_at: 123,
            sha256: "a".repeat(64),
        });
        service.auto_start = id.is_daemon();
        write(
            &engine.storage.config_dir(id, version).join("custom.conf"),
            b"custom setting",
        );
    }
    settings.services.get_mut(&ServiceId::Php).unwrap().ports = vec![23456];
    let site = Site {
        id: uuid::Uuid::new_v4().to_string(),
        hostname: "custom.test".into(),
        root: engine.storage.root.join("projects/site"),
        mode: SiteMode::Php,
        upstream_port: None,
        enabled: true,
        server: SiteServer::Nginx,
        nginx_directives: Some("index other.php;".into()),
    };
    write(&site.root.join("index.php"), b"user project");
    write(
        &engine
            .storage
            .root
            .join("config/sites")
            .join(&site.id)
            .join("nginx.conf"),
        b"custom site rules",
    );
    settings.sites.push(site);
    engine.storage.save(&settings).unwrap();
    drop(settings);
    write(
        &engine.storage.config(ServiceId::Php, "1.0.0"),
        b"memory_limit=123M\nextension=redis\ncurl.cainfo=C:/custom.pem\n",
    );
    write(
        &engine.storage.root.join("config/orphan/obsolete.ini"),
        &[0, 255, 4, 12],
    );
    write(
        &engine.storage.root.join("config/certificates/company.pem"),
        b"custom corporate CA",
    );
    for (path, bytes) in protected_files() {
        write(&engine.storage.root.join(path), &bytes);
    }
    engine
}

fn protected_files() -> BTreeMap<&'static str, Vec<u8>> {
    [
        ("data/mysql/1.0.0/database.ibd", vec![0, 255, 1, 99]),
        ("data/postgresql/1.0.0/PG_VERSION", b"1".to_vec()),
        (
            "data/tls/caddy/pki/authorities/local/root.key",
            b"local CA private key".to_vec(),
        ),
        (
            "data/nodejs/1.0.0/global/package.json",
            b"global package".to_vec(),
        ),
        (
            "data/composer/1.0.0/home/auth.json",
            b"composer credentials".to_vec(),
        ),
        ("projects/site/index.php", b"user project".to_vec()),
        ("logs/mysql/1.0.0/mysql.log", b"user logs".to_vec()),
        ("secrets/mysql-1.0.0.bin", b"protected credentials".to_vec()),
    ]
    .into_iter()
    .collect()
}

#[tokio::test]
async fn resets_configurations_with_verified_backup_and_preserves_all_environment_data() {
    let temporary = tempfile::Builder::new()
        .prefix("Larager reset á ")
        .tempdir()
        .unwrap();
    let root = temporary.path().join("environment");
    let engine = configured(&root).await;
    let previous = engine.settings.read().await.clone();
    let settings_bytes = fs::read(root.join("settings.json")).unwrap();
    let php_before = fs::read(engine.storage.config(ServiceId::Php, "1.0.0")).unwrap();
    let result = engine.reset_configuration().await.unwrap();
    assert!(result.removed_files > 0);
    assert_eq!(
        fs::read(result.backup_path.join("settings.json")).unwrap(),
        settings_bytes
    );
    assert_eq!(
        fs::read(result.backup_path.join("config/php/1.0.0/php.ini")).unwrap(),
        php_before
    );
    assert_eq!(
        fs::read(result.backup_path.join("config/orphan/obsolete.ini")).unwrap(),
        [0, 255, 4, 12]
    );
    let settings = engine.settings.read().await;
    assert_eq!(
        (&settings.language, &settings.theme, settings.close_behavior),
        (
            &"auto".to_string(),
            &"system".to_string(),
            CloseBehavior::Ask
        )
    );
    assert!(settings.sites.is_empty());
    assert_eq!(settings.revision, previous.revision + 1);
    for id in ServiceId::ALL {
        assert_eq!(
            serde_json::to_value(&settings.services[&id].installations).unwrap(),
            serde_json::to_value(&previous.services[&id].installations).unwrap()
        );
        assert_eq!(
            settings.services[&id].active_version,
            previous.services[&id].active_version
        );
        assert_eq!(settings.services[&id].ports, id.ports());
        assert!(!settings.services[&id].auto_start);
        assert!(!settings.services[&id].add_to_path);
        assert_eq!(
            fs::read(engine.storage.package(id, "1.0.0").join(id.executable())).unwrap(),
            b"package fixture"
        );
        assert!(
            !engine
                .storage
                .config_dir(id, "1.0.0")
                .join("custom.conf")
                .exists()
        );
    }
    drop(settings);
    for (path, bytes) in protected_files() {
        assert_eq!(fs::read(root.join(path)).unwrap(), bytes, "{path}");
    }
    assert!(!root.join("config/orphan/obsolete.ini").exists());
    assert!(!root.join("config/certificates/company.pem").exists());
    let php = fs::read_to_string(engine.storage.config(ServiceId::Php, "1.0.0")).unwrap();
    assert!(php.contains("memory_limit=256M"));
    assert!(php.contains("curl.cainfo"));
    assert!(php.contains("openssl.cafile"));
    assert!(!php.contains("extension=redis"));
    assert!(!php.contains("C:/custom.pem"));
    assert!(
        engine
            .snapshot()
            .await
            .unwrap()
            .services
            .iter()
            .all(|service| service.pid.is_none())
    );
    let again = engine.reset_configuration().await.unwrap();
    assert_eq!(again.removed_files, 0);
    assert!(result.backup_path.exists());
    drop(engine);
    let reopened = Engine::open(root).unwrap();
    assert_eq!(reopened.settings.read().await.language, "auto");
}

#[tokio::test]
async fn refuses_to_reset_a_project_inside_the_configuration_tree() {
    let temporary = tempfile::tempdir().unwrap();
    let engine = configured(temporary.path()).await;
    let project = engine.storage.root.join("config/user-project");
    write(&project.join("index.php"), b"user project inside config");
    engine.settings.write().await.sites[0].root = project.clone();
    assert!(engine.reset_configuration().await.is_err());
    assert_eq!(
        fs::read(project.join("index.php")).unwrap(),
        b"user project inside config"
    );
    assert_eq!(engine.settings.read().await.language, "pt-BR");
}

#[cfg(windows)]
#[tokio::test]
async fn rolls_back_defaults_and_deleted_files_when_a_configuration_file_is_locked() {
    use std::os::windows::fs::OpenOptionsExt;
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().to_owned();
    let engine = configured(&root).await;
    let locked = root.join("config/orphan/obsolete.ini");
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1 | 2)
        .open(&locked)
        .unwrap();
    let settings = fs::read(root.join("settings.json")).unwrap();
    let php = fs::read(engine.storage.config(ServiceId::Php, "1.0.0")).unwrap();
    let error = engine
        .reset_configuration()
        .await
        .err()
        .expect("locked deletion must fail");
    assert!(
        matches!(error, larager::error::Error::FileOperation(_)),
        "{error}"
    );
    assert_eq!(fs::read(root.join("settings.json")).unwrap(), settings);
    assert_eq!(
        fs::read(engine.storage.config(ServiceId::Php, "1.0.0")).unwrap(),
        php
    );
    assert_eq!(fs::read(&locked).unwrap(), [0, 255, 4, 12]);
    assert_eq!(engine.settings.read().await.language, "pt-BR");
    assert!(!root.join("backups/pending-config.json").exists());
    drop(handle);
    engine.reset_configuration().await.unwrap();
    assert!(!locked.exists());
    for (path, bytes) in protected_files() {
        assert_eq!(fs::read(root.join(path)).unwrap(), bytes, "{path}");
    }
}

#[cfg(windows)]
#[tokio::test]
async fn refuses_junctions_without_touching_the_external_target() {
    use std::{os::windows::process::CommandExt, process::Command};
    let temporary = tempfile::tempdir().unwrap();
    let engine = configured(&temporary.path().join("environment")).await;
    let outside = temporary.path().join("outside");
    fs::create_dir(&outside).unwrap();
    write(&outside.join("keep.txt"), b"external user data");
    let link = engine.storage.root.join("config/external");
    let status = Command::new("cmd.exe")
        .creation_flags(0x08000000)
        .args(["/D", "/C", "mklink", "/J"])
        .arg(link.to_string_lossy().replace('/', "\\"))
        .arg(&outside)
        .status()
        .unwrap();
    assert!(status.success());
    assert!(engine.reset_configuration().await.is_err());
    assert_eq!(
        fs::read(outside.join("keep.txt")).unwrap(),
        b"external user data"
    );
    fs::remove_dir(link).unwrap();
}
