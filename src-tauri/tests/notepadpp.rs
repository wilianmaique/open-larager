#![cfg(windows)]
use larager::{catalog, engine::Engine, model::ServiceId, platform};
use std::fs;

#[tokio::test]
#[ignore = "Downloads the official portable Notepad++ ZIP; does not change Windows associations."]
async fn portable_install_and_removal_preserve_editor_profile() {
    let directory = tempfile::Builder::new()
        .prefix("Larager editor á ")
        .tempdir()
        .unwrap();
    let engine = Engine::open(directory.path().to_owned()).unwrap();
    let package = catalog::packages()
        .unwrap()
        .into_iter()
        .find(|p| p.service == ServiceId::Notepadpp)
        .unwrap();
    engine
        .install(ServiceId::Notepadpp, package.version.clone(), |_| {})
        .await
        .unwrap();
    let binary = engine
        .storage
        .package(ServiceId::Notepadpp, &package.version);
    assert!(binary.join("notepad++.exe").is_file());
    assert!(binary.join("doLocalConf.xml").is_file());
    assert!(binary.join("disableNppAutoUpdate.xml").is_file());
    assert!(!binary.join("updater/GUP.exe").exists());
    let snapshot = engine.snapshot().await.unwrap();
    let status = snapshot
        .services
        .iter()
        .find(|s| s.id == ServiceId::Notepadpp)
        .unwrap();
    assert_eq!(status.status, "ready");
    assert!(!snapshot.settings.notepadpp.file_associations);
    assert!(!snapshot.settings.services[&ServiceId::Notepadpp].add_to_path);
    assert!(
        engine
            .set_user_path(ServiceId::Notepadpp, true)
            .await
            .is_err()
    );
    assert!(
        engine
            .service_action(ServiceId::Notepadpp, "start")
            .await
            .is_err()
    );
    let profile = engine.storage.root.join("config/notepadpp/profile");
    fs::create_dir_all(profile.join("backup")).unwrap();
    fs::write(profile.join("backup/unsaved.txt"), "unsaved buffer á").unwrap();
    // This upstream mode runs silently, exports a function list and exits.
    // It exercises the real executable and the separate Unicode settings path.
    let sample = engine.storage.root.join("projects/editor-probe.cpp");
    fs::write(&sample, "int main() { return 0; }\n").unwrap();
    let mut probe = platform::command(binary.join("notepad++.exe"));
    probe
        .arg("-export=functionList")
        .arg("-lcpp")
        .arg(format!(
            "-settingsDir={}",
            larager::storage::slash(&profile)
        ))
        .arg(&sample);
    platform::checked_output(probe, std::time::Duration::from_secs(20))
        .await
        .unwrap();
    // The editor expands its default settings on first launch. Preserve the
    // resulting user profile byte-for-byte through removal and reinstall.
    let config = fs::read(profile.join("config.xml")).unwrap();
    assert!(
        config.len() > 1000,
        "The real editor must use the separate profile"
    );
    engine
        .remove_package(ServiceId::Notepadpp, package.version.clone())
        .await
        .unwrap();
    assert!(!binary.exists());
    assert_eq!(fs::read(profile.join("config.xml")).unwrap(), config);
    assert_eq!(
        fs::read_to_string(profile.join("backup/unsaved.txt")).unwrap(),
        "unsaved buffer á"
    );
    engine
        .install(ServiceId::Notepadpp, package.version, |_| {})
        .await
        .unwrap();
    assert_eq!(fs::read(profile.join("config.xml")).unwrap(), config);
    assert_eq!(
        fs::read_to_string(profile.join("backup/unsaved.txt")).unwrap(),
        "unsaved buffer á"
    );
}
