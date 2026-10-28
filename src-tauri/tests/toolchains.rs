#![cfg(windows)]
use larager::{
    catalog, engine::Engine, model::ServiceId, platform, storage::atomic_write, toolchain,
};
use std::{fs, os::windows::process::CommandExt, path::Path, time::Duration};

async fn launchers(engine: &Engine, id: ServiceId) {
    let settings = engine.settings.read().await;
    let layout = toolchain::layout(&engine.storage, &settings, id).unwrap();
    for (path, content) in layout.files {
        atomic_write(&path, content.as_bytes()).unwrap();
    }
}

async fn cli(engine: &Engine, name: &str, args: &str) -> String {
    let id = if name == "composer" {
        "composer"
    } else {
        "nodejs"
    };
    let path = engine
        .storage
        .root
        .join(format!("config/cli/{id}/{name}.cmd"));
    let mut command = platform::command("cmd.exe");
    command
        .args(["/D", "/S", "/C"])
        .raw_arg(format!("\"\"{}\" {args}\"", path.display()))
        .current_dir(engine.storage.root.join("projects"));
    platform::checked_output(command, Duration::from_secs(60))
        .await
        .unwrap()
}

async fn shells(engine: &Engine, node_version: &str, npm_version: &str, composer_version: &str) {
    let settings = engine.settings.read().await;
    let mut directories = toolchain::layout(&engine.storage, &settings, ServiceId::Composer)
        .unwrap()
        .directories;
    directories.extend(
        toolchain::layout(&engine.storage, &settings, ServiceId::Nodejs)
            .unwrap()
            .directories,
    );
    directories.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path = std::env::join_paths(directories).unwrap();
    drop(settings);
    for (executable, arguments) in [
        (
            "powershell.exe",
            vec![
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$resolvedNpm = (Get-Command npm).Source; $expectedNpm = Join-Path $env:LARAGER_TEST_WORKSPACE 'config/cli/nodejs/npm.cmd'; if ([IO.Path]::GetFullPath($resolvedNpm) -ne [IO.Path]::GetFullPath($expectedNpm)) { throw ('Unexpected npm command: ' + $resolvedNpm) }; node --version; npm --version; npx --version; composer --version --no-ansi; exit $LASTEXITCODE",
            ],
        ),
        (
            "cmd.exe",
            vec![
                "/D",
                "/C",
                "node --version && call npm --version && call npx --version && call composer --version --no-ansi",
            ],
        ),
    ] {
        let mut command = platform::command(executable);
        command
            .args(arguments)
            .env("PATH", &path)
            .env("LARAGER_TEST_WORKSPACE", &engine.storage.root)
            .current_dir(engine.storage.root.join("projects"));
        let output = platform::checked_output(command, Duration::from_secs(60))
            .await
            .unwrap();
        assert!(
            output.contains(&format!("v{node_version}")),
            "{executable}: {output}"
        );
        assert!(output.contains(npm_version), "{executable}: {output}");
        assert!(output.contains(composer_version), "{executable}: {output}");
    }
}

#[tokio::test]
#[ignore = "Downloads pinned Node.js, PHP and Composer into a temporary Unicode workspace; never changes the real PATH."]
async fn node_npm_composer_versions_and_preserved_tool_data() {
    let workspace = tempfile::Builder::new()
        .prefix("Larager tools á ! & ")
        .tempdir()
        .unwrap();
    let engine = Engine::open(workspace.path().to_owned()).unwrap();
    let catalog = catalog::packages().unwrap();
    let nodes: Vec<_> = catalog
        .iter()
        .filter(|p| p.service == ServiceId::Nodejs)
        .take(2)
        .collect();
    let phps: Vec<_> = catalog
        .iter()
        .filter(|p| p.service == ServiceId::Php)
        .take(2)
        .collect();
    let composer = catalog
        .iter()
        .find(|p| p.service == ServiceId::Composer)
        .unwrap();
    for node in &nodes {
        engine
            .install(ServiceId::Nodejs, node.version.clone(), |_| {})
            .await
            .unwrap();
    }
    engine.stack_action("start").await.unwrap();
    assert!(
        engine
            .snapshot()
            .await
            .unwrap()
            .services
            .iter()
            .all(|s| s.pid.is_none())
    );
    assert!(
        engine
            .service_action(ServiceId::Nodejs, "start")
            .await
            .is_err()
    );
    assert!(
        engine
            .save_service(
                ServiceId::Nodejs,
                Some(nodes[0].version.clone()),
                vec![],
                true
            )
            .await
            .is_err()
    );
    assert!(engine.read_config(ServiceId::Nodejs).await.is_err());
    launchers(&engine, ServiceId::Nodejs).await;
    assert_eq!(
        cli(&engine, "npm", "--version").await.trim(),
        nodes[0].bundled_tools["npm"]
    );
    assert_eq!(
        cli(&engine, "npx", "--version").await.trim(),
        nodes[0].bundled_tools["npm"]
    );
    let prefix = engine
        .storage
        .data(ServiceId::Nodejs, &nodes[0].version)
        .join("global");
    let reported = cli(&engine, "npm", "config get prefix").await;
    assert_eq!(Path::new(reported.trim()), prefix);
    cli(&engine, "npm", "config set fund false").await;
    assert!(
        engine
            .storage
            .config_dir(ServiceId::Nodejs, &nodes[0].version)
            .join("npmrc")
            .is_file()
    );
    let project = engine.storage.root.join("projects");
    fs::create_dir(project.join("fixture")).unwrap();
    fs::write(
        project.join("fixture/package.json"),
        r#"{"name":"larager-fixture-cli","version":"1.0.0","bin":{"larager-fixture":"index.js"}}"#,
    )
    .unwrap();
    fs::write(
        project.join("fixture/index.js"),
        "#!/usr/bin/env node\nconsole.log(process.version);\n",
    )
    .unwrap();
    cli(
        &engine,
        "npm",
        "install --global --ignore-scripts --install-links --no-audit ./fixture",
    )
    .await;
    assert!(
        prefix
            .join("node_modules/larager-fixture-cli/package.json")
            .is_file()
    );
    for php in &phps {
        engine
            .install(ServiceId::Php, php.version.clone(), |_| {})
            .await
            .unwrap();
    }
    engine
        .install(ServiceId::Composer, composer.version.clone(), |_| {})
        .await
        .unwrap();
    launchers(&engine, ServiceId::Composer).await;
    let output = cli(&engine, "composer", "--version --no-ansi").await;
    assert!(output.contains(&composer.version), "{output}");
    shells(
        &engine,
        &nodes[0].version,
        &nodes[0].bundled_tools["npm"],
        &composer.version,
    )
    .await;
    fs::write(project.join("composer.json"), r#"{"name":"larager/smoke","scripts":{"smoke":"@php -r \"echo json_encode([PHP_VERSION, php_ini_loaded_file()]);\""}}"#).unwrap();
    let output = cli(&engine, "composer", "run-script smoke --no-ansi").await;
    assert!(
        output.contains(phps[0].version.split('-').next().unwrap()),
        "{output}"
    );
    assert!(output.contains("php.ini"), "{output}");
    engine
        .save_service(
            ServiceId::Nodejs,
            Some(nodes[1].version.clone()),
            vec![],
            false,
        )
        .await
        .unwrap();
    engine
        .save_service(
            ServiceId::Php,
            Some(phps[1].version.clone()),
            ServiceId::Php.ports(),
            false,
        )
        .await
        .unwrap();
    launchers(&engine, ServiceId::Nodejs).await;
    launchers(&engine, ServiceId::Composer).await;
    shells(
        &engine,
        &nodes[1].version,
        &nodes[1].bundled_tools["npm"],
        &composer.version,
    )
    .await;
    assert_eq!(
        cli(&engine, "npm", "--version").await.trim(),
        nodes[1].bundled_tools["npm"]
    );
    let output = cli(&engine, "composer", "run-script smoke --no-ansi").await;
    assert!(
        output.contains(phps[1].version.split('-').next().unwrap()),
        "{output}"
    );
    assert!(!engine.settings.read().await.services[&ServiceId::Php].add_to_path);
    engine
        .remove_package(ServiceId::Nodejs, nodes[0].version.clone())
        .await
        .unwrap();
    assert!(
        prefix
            .join("node_modules/larager-fixture-cli/package.json")
            .is_file()
    );
    assert!(
        !engine
            .storage
            .package(ServiceId::Nodejs, &nodes[0].version)
            .exists()
    );
    assert_eq!(
        engine
            .snapshot()
            .await
            .unwrap()
            .services
            .iter()
            .find(|s| s.id == ServiceId::Nodejs)
            .unwrap()
            .status,
        "ready"
    );
    engine.shutdown().await;
}
