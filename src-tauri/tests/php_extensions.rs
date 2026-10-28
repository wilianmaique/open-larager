#![cfg(windows)]
use larager::{catalog, engine::Engine, model::ServiceId, php_extensions, storage::atomic_write};
use std::{
    fs,
    net::{Ipv4Addr, TcpListener},
};

#[tokio::test]
#[ignore = "Downloads the four pinned PHP runtimes and curated PECL DLLs; exercises real Windows loading and reloads."]
async fn compatible_extensions_across_php_versions() {
    let workspace = tempfile::Builder::new()
        .prefix("Larager PHP extensions á ")
        .tempdir()
        .unwrap();
    let engine = Engine::open(workspace.path().to_owned()).unwrap();
    let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = reservation.local_addr().unwrap().port();
    drop(reservation);
    engine
        .save_service(ServiceId::Php, None, vec![port], false)
        .await
        .unwrap();
    let mut previous_version = None;
    for package in catalog::packages()
        .unwrap()
        .into_iter()
        .filter(|p| p.service == ServiceId::Php)
    {
        println!("Testing PHP {}", package.version);
        engine
            .install(ServiceId::Php, package.version.clone(), |_| {})
            .await
            .unwrap();
        engine
            .save_service(
                ServiceId::Php,
                Some(package.version.clone()),
                vec![port],
                false,
            )
            .await
            .unwrap();
        let initial = engine.php_extensions().await.unwrap();
        assert!(initial.diagnostics.is_empty(), "{}", initial.diagnostics);
        assert_eq!(
            initial.runtime.branch,
            package
                .version
                .split('.')
                .take(2)
                .collect::<Vec<_>>()
                .join(".")
        );
        let path = engine.storage.config(ServiceId::Php, &package.version);
        let original = fs::read_to_string(&path).unwrap();
        atomic_write(
            &path,
            format!("{original}\n; user setting\nmemory_limit=384M\n").as_bytes(),
        )
        .unwrap();
        if let Some(previous) = previous_version.take() {
            assert!(
                engine
                    .set_php_extension(previous, "intl".into(), true, |_| {})
                    .await
                    .is_err()
            );
        }
        engine
            .service_action(ServiceId::Php, "start")
            .await
            .unwrap();
        let initial_pid = engine
            .snapshot()
            .await
            .unwrap()
            .services
            .into_iter()
            .find(|s| s.id == ServiceId::Php)
            .unwrap()
            .pid;
        for id in [
            "intl", "exif", "opcache", "igbinary", "redis", "apcu", "xdebug",
        ] {
            // PHP 8.5 can include OPcache in the runtime itself.
            if initial.extensions.iter().any(|e| e.id == id && e.builtin) {
                continue;
            }
            let inventory = engine
                .set_php_extension(package.version.clone(), id.into(), true, |_| {})
                .await
                .unwrap_or_else(|error| panic!("PHP {} / {id}: {error}", package.version));
            assert!(
                inventory.extensions.iter().any(|e| e.id == id && e.loaded),
                "{id}"
            );
            assert!(inventory.diagnostics.is_empty());
            assert!(
                fs::read_to_string(&path)
                    .unwrap()
                    .contains("memory_limit=384M")
            );
        }
        let new_pid = engine
            .snapshot()
            .await
            .unwrap()
            .services
            .into_iter()
            .find(|s| s.id == ServiceId::Php)
            .unwrap()
            .pid;
        assert!(new_pid.is_some());
        assert_ne!(
            initial_pid, new_pid,
            "PHP must reload after extension changes"
        );
        // Fail startup after stopping PHP, then let recovery find the executable again.
        let cgi = engine
            .storage
            .package(ServiceId::Php, &package.version)
            .join("php-cgi.exe");
        let backup_cgi = cgi.with_extension("rollback-test.exe");
        let before_rollback = fs::read_to_string(&path).unwrap();
        let failure = engine
            .set_php_extension(package.version.clone(), "gd".into(), true, |progress| {
                if progress.phase == "reloading" {
                    fs::rename(&cgi, &backup_cgi).unwrap();
                }
                if progress.phase == "restoring" && backup_cgi.exists() {
                    fs::rename(&backup_cgi, &cgi).unwrap();
                }
            })
            .await
            .unwrap_err();
        assert!(
            failure
                .to_string()
                .contains("previous configuration and service were restored"),
            "{failure}"
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before_rollback);
        assert!(
            engine
                .snapshot()
                .await
                .unwrap()
                .services
                .iter()
                .any(|service| service.id == ServiceId::Php && service.status == "running")
        );
        let new_pid = engine
            .snapshot()
            .await
            .unwrap()
            .services
            .into_iter()
            .find(|s| s.id == ServiceId::Php)
            .unwrap()
            .pid;
        let before_cancel = fs::read_to_string(&path).unwrap();
        assert!(
            engine
                .set_php_extension(package.version.clone(), "gd".into(), true, |progress| {
                    if progress.phase == "validating" {
                        engine.cancel_install(ServiceId::Php).unwrap();
                    }
                })
                .await
                .unwrap_err()
                .to_string()
                .contains("cancelled")
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), before_cancel);
        assert!(
            engine
                .set_php_extension(package.version.clone(), "mbstring".into(), false, |_| {})
                .await
                .is_err(),
            "exif still needs mbstring"
        );
        // A PHP startup warning can still exit successfully: reject it and preserve the running process/config.
        let enabled = fs::read_to_string(&path).unwrap();
        atomic_write(
            &path,
            format!("{enabled}\nextension=missing_larager_test\n").as_bytes(),
        )
        .unwrap();
        let invalid = fs::read_to_string(&path).unwrap();
        assert!(
            engine
                .set_php_extension(package.version.clone(), "gd".into(), true, |_| {})
                .await
                .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        assert_eq!(
            engine
                .snapshot()
                .await
                .unwrap()
                .services
                .into_iter()
                .find(|s| s.id == ServiceId::Php)
                .unwrap()
                .pid,
            new_pid
        );
        atomic_write(&path, enabled.as_bytes()).unwrap();
        for id in ["xdebug", "redis", "igbinary", "apcu", "intl", "exif"] {
            let inventory = engine
                .set_php_extension(package.version.clone(), id.into(), false, |_| {})
                .await
                .unwrap_or_else(|error| {
                    panic!(
                        "Disable PHP {} / {id}: {error}; ini: {}",
                        package.version,
                        fs::read_to_string(&path).unwrap()
                    )
                });
            assert!(inventory.extensions.iter().any(|e| e.id == id && !e.loaded));
        }
        engine
            .set_php_extension(package.version.clone(), "mbstring".into(), false, |_| {})
            .await
            .unwrap();
        let with_dependency = engine
            .set_php_extension(package.version.clone(), "exif".into(), true, |_| {})
            .await
            .unwrap();
        assert!(
            with_dependency
                .extensions
                .iter()
                .any(|entry| entry.id == "mbstring" && entry.loaded)
        );
        engine.service_action(ServiceId::Php, "stop").await.unwrap();
        engine
            .set_php_extension(package.version.clone(), "intl".into(), true, |_| {})
            .await
            .unwrap();
        assert!(
            engine
                .snapshot()
                .await
                .unwrap()
                .services
                .into_iter()
                .find(|s| s.id == ServiceId::Php)
                .unwrap()
                .pid
                .is_none(),
            "Stopped PHP stays stopped"
        );
        assert!(
            engine
                .set_php_extension(package.version.clone(), "../../evil".into(), true, |_| {})
                .await
                .is_err()
        );
        assert!(
            php_extensions::probe(&engine.storage, &package.version, Some(&path))
                .await
                .unwrap()
                .diagnostics
                .is_empty()
        );
        previous_version = Some(package.version);
    }
    engine.shutdown().await;
}
