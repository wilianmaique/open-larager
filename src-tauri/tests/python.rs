#![cfg(windows)]
use larager::{
    catalog, engine::Engine, model::ServiceId, platform, python, storage::atomic_write, toolchain,
    workspace::Workspace,
};
use std::{fs, os::windows::process::CommandExt, path::Path, time::Duration};

async fn launchers(engine: &Engine) -> std::ffi::OsString {
    let settings = engine.settings.read().await;
    let layout = toolchain::layout(&engine.storage, &settings, ServiceId::Python).unwrap();
    for (path, content) in layout.files {
        assert!(content.is_ascii());
        atomic_write(&path, content.as_bytes()).unwrap();
    }
    let mut directories = layout.directories;
    directories.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    std::env::join_paths(directories).unwrap()
}

async fn shell(engine: &Engine, path: &std::ffi::OsStr, executable: &str, script: &str) -> String {
    let mut command = platform::command(executable);
    if executable == "cmd.exe" {
        command.args(["/D", "/S", "/C"]).raw_arg(script);
    } else {
        command.args(["-NoProfile", "-NonInteractive", "-Command", script]);
    }
    command
        .env("PATH", path)
        .current_dir(engine.storage.root.join("projects"));
    platform::checked_output(command, Duration::from_secs(120))
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "Downloads two pinned full CPython runtimes in a temporary Unicode workspace; never changes the real PATH."]
async fn python_pip_venv_switch_removal_and_reinstall() {
    let temporary = tempfile::Builder::new()
        .prefix("Larager Python á ! & ")
        .tempdir()
        .unwrap();
    let workspace = Workspace::open(
        temporary.path().join("source é"),
        temporary.path().join("location.json"),
    )
    .unwrap();
    let engine = workspace.engine.read().await;
    let packages: Vec<_> = catalog::packages()
        .unwrap()
        .into_iter()
        .filter(|package| package.service == ServiceId::Python)
        .take(2)
        .collect();
    assert_eq!(packages.len(), 2);
    for package in &packages {
        engine
            .install(ServiceId::Python, package.version.clone(), |_| {})
            .await
            .unwrap();
    }
    assert!(
        engine
            .service_action(ServiceId::Python, "start")
            .await
            .is_err()
    );
    assert!(
        engine
            .save_service(
                ServiceId::Python,
                Some(packages[0].version.clone()),
                vec![],
                true
            )
            .await
            .is_err()
    );
    assert!(
        engine
            .save_service(
                ServiceId::Python,
                Some(packages[0].version.clone()),
                vec![8000],
                false
            )
            .await
            .is_err()
    );
    assert!(engine.read_config(ServiceId::Python).await.is_err());
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
    let path = launchers(&engine).await;
    let project = engine.storage.root.join("projects");
    fs::write(project.join("probe.py"), "import sys, ssl, sqlite3, tkinter, pip\nprint('.'.join(map(str, sys.version_info[:3])))\nprint(sys.prefix)\nprint(sys.argv[1])\n").unwrap();
    for executable in ["powershell.exe", "cmd.exe"] {
        let output = shell(
            &engine,
            &path,
            executable,
            "python probe.py \"argument with spaces\"",
        )
        .await;
        assert!(
            output.contains(&packages[0].version),
            "{executable}: {output}"
        );
        assert!(output.contains("data"), "{output}");
        assert!(output.contains("argument with spaces"), "{output}");
        for command in [
            "python3 --version",
            "pip --version",
            "pip3 --version",
            "python -m pip --version",
        ] {
            let output = shell(&engine, &path, executable, command).await;
            assert!(
                output.contains(if command.starts_with("python3") {
                    &packages[0].version
                } else {
                    "pip "
                }),
                "{output}"
            );
        }
    }
    let output = shell(
        &engine,
        &path,
        "cmd.exe",
        "python probe.py \"argument with spaces\"",
    )
    .await;
    assert!(output.contains("argument with spaces"), "{output}");
    // Build a local wheel directly: no network or unpinned build dependency is needed.
    fs::write(project.join("wheel.py"), r#"import zipfile
with zipfile.ZipFile('larager_fixture-1.0-py3-none-any.whl', 'w') as w:
    w.writestr('larager_fixture.py', 'VALUE = 42\n')
    w.writestr('larager_fixture-1.0.dist-info/METADATA', 'Metadata-Version: 2.1\nName: larager-fixture\nVersion: 1.0\n')
    w.writestr('larager_fixture-1.0.dist-info/WHEEL', 'Wheel-Version: 1.0\nGenerator: fixture\nRoot-Is-Purelib: true\nTag: py3-none-any\n')
    w.writestr('larager_fixture-1.0.dist-info/RECORD', '')
"#).unwrap();
    shell(&engine, &path, "powershell.exe", "python wheel.py; pip install --no-index --no-deps ./larager_fixture-1.0-py3-none-any.whl; exit $LASTEXITCODE").await;
    let installed = python::environment(&engine.storage, &packages[0].version)
        .join("Lib/site-packages/larager_fixture.py");
    assert!(installed.is_file());
    assert!(
        !engine
            .storage
            .package(ServiceId::Python, &packages[0].version)
            .join("Lib/site-packages/larager_fixture.py")
            .exists()
    );
    shell(&engine, &path, "powershell.exe", "python -m venv '.venv project'; & '.\\.venv project\\Scripts\\python.exe' -m pip --version; exit $LASTEXITCODE").await;
    for package in [&packages[1], &packages[0]] {
        engine
            .save_service(
                ServiceId::Python,
                Some(package.version.clone()),
                vec![],
                false,
            )
            .await
            .unwrap();
        launchers(&engine).await;
        // Stable launcher directory allows version switching even in an existing terminal.
        let output = shell(
            &engine,
            &path,
            "powershell.exe",
            "python --version; pip --version",
        )
        .await;
        assert!(output.contains(&package.version), "{output}");
    }
    engine
        .remove_package(ServiceId::Python, packages[0].version.clone())
        .await
        .unwrap();
    assert!(installed.is_file());
    assert!(project.join(".venv project/pyvenv.cfg").exists());
    engine
        .install(ServiceId::Python, packages[0].version.clone(), |_| {})
        .await
        .unwrap();
    engine
        .save_service(
            ServiceId::Python,
            Some(packages[0].version.clone()),
            vec![],
            false,
        )
        .await
        .unwrap();
    launchers(&engine).await;
    let output = shell(
        &engine,
        &path,
        "powershell.exe",
        "python -c 'import larager_fixture; print(larager_fixture.VALUE)'",
    )
    .await;
    assert_eq!(output.trim(), "42");
    let environment = python::environment(&engine.storage, &packages[0].version);
    assert!(Path::new(&environment).join(".larager-python").is_file());
    let old_configuration = fs::read(project.join(".venv project/pyvenv.cfg")).unwrap();
    drop(engine);
    let destination = temporary.path().join("moved ó");
    fs::create_dir(&destination).unwrap();
    let selected = workspace.select(Some(destination)).await.unwrap().unwrap();
    let status = workspace.relocate(selected.token).await.unwrap();
    assert!(
        status.cleanup_source.is_none(),
        "{:?}",
        status.cleanup_error
    );
    let engine = workspace.engine.read().await;
    let path = launchers(&engine).await;
    let output = shell(
        &engine, &path, "powershell.exe",
        "python -c 'import larager_fixture; print(larager_fixture.VALUE)'; pip --version; exit $LASTEXITCODE",
    ).await;
    assert!(output.contains("42"), "{output}");
    assert!(output.contains("pip "), "{output}");
    assert_eq!(
        fs::read(
            engine
                .storage
                .root
                .join("projects/.venv project/pyvenv.cfg")
        )
        .unwrap(),
        old_configuration,
    );
    engine.shutdown().await;
}

#[tokio::test]
async fn refuses_foreign_environments_without_replacing_data() {
    let temporary = tempfile::tempdir().unwrap();
    let storage = larager::storage::Storage::open(temporary.path().to_owned()).unwrap();
    let environment = python::environment(&storage, "3.15.2");
    fs::create_dir_all(&environment).unwrap();
    let keep = environment.join("keep.txt");
    fs::write(&keep, "user data").unwrap();
    assert!(python::initialize(&storage, "3.15.2").await.is_err());
    assert_eq!(fs::read_to_string(&keep).unwrap(), "user data");
    fs::write(
        environment.join("pyvenv.cfg"),
        "home = C:/External/Python\n",
    )
    .unwrap();
    assert!(python::initialize(&storage, "3.15.2").await.is_err());
    let relative = Path::new("data/python/3.15.2/environment/pyvenv.cfg");
    assert!(!python::is_managed_config(&storage, relative));
    fs::write(environment.join(".larager-python"), "3.15.2").unwrap();
    assert!(python::is_managed_config(&storage, relative));
    assert!(!python::is_managed_config(
        &storage,
        Path::new("projects/.venv/pyvenv.cfg")
    ));
    assert!(!python::is_managed_config(
        &storage,
        Path::new("data/python/3.15.2/environment/nested/pyvenv.cfg")
    ));
    assert_eq!(fs::read_to_string(&keep).unwrap(), "user data");
}
