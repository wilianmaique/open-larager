#![cfg(windows)]
use larager::{
    catalog,
    engine::Engine,
    model::{ServiceId, Site, SiteMode},
    platform,
};
use std::{
    fs,
    net::{Ipv4Addr, SocketAddr, TcpListener},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "Downloads a Redis distribution for Windows compatibility verification."]
async fn redis_windows_lifecycle() {
    let workspace = tempfile::tempdir().unwrap();
    let engine = Engine::open(workspace.path().to_owned()).unwrap();
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    engine
        .save_service(ServiceId::Redis, None, vec![port], false)
        .await
        .unwrap();
    let package = catalog::packages()
        .unwrap()
        .into_iter()
        .find(|p| p.service == ServiceId::Redis)
        .unwrap();
    engine
        .install(ServiceId::Redis, package.version.clone(), |_| {})
        .await
        .unwrap();
    let mut probe = platform::command(
        engine
            .storage
            .package(ServiceId::Redis, &package.version)
            .join("redis-server.exe"),
    );
    probe.arg("--version");
    assert!(
        platform::checked_output(probe, Duration::from_secs(15))
            .await
            .unwrap()
            .contains(&format!("v={}", package.version))
    );
    engine
        .service_action(ServiceId::Redis, "start")
        .await
        .unwrap();
    engine
        .service_action(ServiceId::Redis, "stop")
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "Downloads PHP, Apache and Nginx and verifies FastCGI requests on real sockets."]
async fn php_web_servers_routing() {
    let workspace = tempfile::Builder::new()
        .prefix("Open Larager á web ")
        .tempdir()
        .unwrap();
    let engine = Engine::open(workspace.path().to_owned()).unwrap();
    let ids = [ServiceId::Php, ServiceId::Apache, ServiceId::Nginx];
    for id in ids {
        let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = socket.local_addr().unwrap().port();
        drop(socket);
        engine
            .save_service(id, None, vec![port], false)
            .await
            .unwrap();
    }
    fs::write(
        workspace.path().join("projects/index.php"),
        "<?php echo 'php-ok';",
    )
    .unwrap();
    for id in ids {
        let package = catalog::packages()
            .unwrap()
            .into_iter()
            .find(|p| p.service == id)
            .unwrap();
        if id == ServiceId::Php {
            let orphan = engine.storage.package(id, &package.version);
            fs::create_dir_all(&orphan).unwrap();
            fs::write(orphan.join("interrupted-install"), "unregistered binaries").unwrap();
        }
        engine.install(id, package.version, |_| {}).await.unwrap();
        engine.service_action(id, "start").await.unwrap();
    }
    for id in [ServiceId::Apache, ServiceId::Nginx] {
        let port = engine.settings.read().await.services[&id].ports[0];
        let response = reqwest::get(format!("http://127.0.0.1:{port}/index.php"))
            .await
            .unwrap();
        let code = response.status();
        let body = response.text().await.unwrap();
        assert!(
            code.is_success(),
            "{id:?}: {body}\n{}",
            engine.logs(id).await.unwrap()
        );
        assert_eq!(body, "php-ok");
    }
    engine.service_action(ServiceId::Php, "stop").await.unwrap();
    let original = engine.read_config(ServiceId::Php).await.unwrap();
    let edited = format!("{original}\n; Keep this user configuration\n");
    engine
        .save_config(ServiceId::Php, edited.clone())
        .await
        .unwrap();
    let php = engine.settings.read().await.services[&ServiceId::Php].clone();
    engine
        .save_service(ServiceId::Php, php.active_version, php.ports, true)
        .await
        .unwrap();
    assert_eq!(
        engine.read_config(ServiceId::Php).await.unwrap(),
        edited,
        "Changing autostart must preserve manual configuration"
    );
}

#[tokio::test]
#[ignore = "Downloads the gateway and mail server to verify live domain reload and reverse proxy."]
async fn gateway_proxy_hot_reload() {
    let workspace = tempfile::Builder::new()
        .prefix("Open Larager á gateway ")
        .tempdir()
        .unwrap();
    let engine = Engine::open(workspace.path().to_owned()).unwrap();
    for id in [ServiceId::Mailpit, ServiceId::Caddy] {
        let mut sockets = Vec::new();
        let mut ports = Vec::new();
        for _ in id.ports() {
            let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            ports.push(socket.local_addr().unwrap().port());
            sockets.push(socket);
        }
        drop(sockets);
        engine.save_service(id, None, ports, false).await.unwrap();
        let package = catalog::packages()
            .unwrap()
            .into_iter()
            .find(|package| package.service == id)
            .unwrap();
        engine.install(id, package.version, |_| {}).await.unwrap();
        engine.service_action(id, "start").await.unwrap();
    }
    let snapshot = engine.snapshot().await.unwrap();
    let settings = snapshot.settings;
    let gateway_pid = snapshot
        .services
        .iter()
        .find(|s| s.id == ServiceId::Caddy)
        .unwrap()
        .pid;
    let details = engine.certificate_action("inspect").await.unwrap();
    assert!(details.contains("Open Larager Local Authority"));
    assert!(
        !details.contains(['\u{fffd}', '\u{feff}', '\0']),
        "{details}"
    );
    engine
        .save_site(Site {
            id: String::new(),
            hostname: "mail.larager.test".into(),
            root: Default::default(),
            mode: SiteMode::Proxy,
            upstream_port: Some(settings.services[&ServiceId::Mailpit].ports[1]),
            enabled: true,
        })
        .await
        .unwrap();
    let ca = reqwest::Certificate::from_pem(&fs::read(engine.certificate_path()).unwrap()).unwrap();
    let port = settings.services[&ServiceId::Caddy].ports[1];
    let client = reqwest::Client::builder()
        .tls_certs_only([ca])
        .resolve(
            "mail.larager.test",
            SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        )
        .build()
        .unwrap();
    let response = client
        .get(format!("https://mail.larager.test:{port}/api/v1/messages"))
        .send()
        .await;
    assert!(
        response.is_ok(),
        "{response:?}\n{}",
        engine.logs(ServiceId::Caddy).await.unwrap()
    );
    assert!(response.unwrap().status().is_success());
    let after = engine.snapshot().await.unwrap();
    assert_eq!(
        after
            .services
            .iter()
            .find(|s| s.id == ServiceId::Caddy)
            .unwrap()
            .pid,
        gateway_pid,
        "Reload should preserve the gateway process"
    );
    let id = after.settings.sites[0].id.clone();
    engine.remove_site(id).await.unwrap();
    assert!(engine.settings.read().await.sites.is_empty());
    engine.shutdown().await;
}

#[tokio::test]
#[ignore = "Downloads portable distributions and launches real processes. Does not change hosts or certificate trust."]
async fn portable_services_complete_lifecycle() {
    let workspace = tempfile::Builder::new()
        .prefix("Open Larager á smoke ")
        .tempdir()
        .unwrap();
    let engine = Engine::open(workspace.path().to_owned()).unwrap();
    let available = catalog::packages().unwrap();
    let mut held = Vec::new();
    for id in ServiceId::ALL {
        let mut ports = Vec::new();
        for _ in id.ports() {
            let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            ports.push(listener.local_addr().unwrap().port());
            held.push(listener);
        }
        engine.save_service(id, None, ports, false).await.unwrap();
    }
    drop(held);
    fs::write(
        workspace.path().join("projects/index.html"),
        "larager-static-ok",
    )
    .unwrap();
    fs::write(workspace.path().join("projects/index.php"), "<?php header('Content-Type: application/json'); echo json_encode(['ok' => true, 'version' => PHP_VERSION]);").unwrap();
    engine
        .save_site(Site {
            id: String::new(),
            hostname: "larager-static.test".into(),
            root: workspace.path().join("projects"),
            mode: SiteMode::Static,
            upstream_port: None,
            enabled: true,
        })
        .await
        .unwrap();
    engine
        .save_site(Site {
            id: String::new(),
            hostname: "larager-php.test".into(),
            root: workspace.path().join("projects"),
            mode: SiteMode::Php,
            upstream_port: None,
            enabled: true,
        })
        .await
        .unwrap();
    for id in ServiceId::ALL {
        let package = available.iter().find(|p| p.service == id).unwrap();
        eprintln!("Installing {} {}", id.key(), package.version);
        engine
            .install(id, package.version.clone(), |_| {})
            .await
            .unwrap();
        eprintln!("Starting {}", id.key());
        engine.service_action(id, "start").await.unwrap();
        let snapshot = engine.snapshot().await.unwrap();
        assert_eq!(
            snapshot
                .services
                .iter()
                .find(|s| s.id == id)
                .unwrap()
                .status,
            "running"
        );
    }
    let snapshot = engine.snapshot().await.unwrap();
    let settings = snapshot.settings;
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    for id in [ServiceId::Apache, ServiceId::Nginx] {
        let url = format!(
            "http://127.0.0.1:{}/index.php",
            settings.services[&id].ports[0]
        );
        let result = http
            .get(url)
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap();
        assert_eq!(result["ok"], true, "PHP through {id:?}");
    }
    // The server must present a chain trusted by its own CA, without disabling validation.
    let ca = reqwest::Certificate::from_pem(&fs::read(engine.certificate_path()).unwrap()).unwrap();
    let tls_port = settings.services[&ServiceId::Caddy].ports[1];
    let tls = reqwest::Client::builder()
        .tls_certs_only([ca])
        .resolve(
            "larager-static.test",
            SocketAddr::from((Ipv4Addr::LOCALHOST, tls_port)),
        )
        .resolve(
            "larager-php.test",
            SocketAddr::from((Ipv4Addr::LOCALHOST, tls_port)),
        )
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    assert_eq!(
        tls.get(format!("https://larager-static.test:{tls_port}/index.html"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .text()
            .await
            .unwrap(),
        "larager-static-ok"
    );
    assert_eq!(
        tls.get(format!("https://larager-php.test:{tls_port}/index.php"))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap()["ok"],
        true
    );
    // Verify authenticated database writes and reads.
    let pg_version = settings.services[&ServiceId::Postgresql]
        .active_version
        .as_deref()
        .unwrap();
    let pg = engine
        .credentials(ServiceId::Postgresql, pg_version)
        .unwrap();
    let mut psql = platform::command(
        engine
            .storage
            .package(ServiceId::Postgresql, pg_version)
            .join("bin/psql.exe"),
    );
    psql.env("PGPASSWORD", pg.password).args(["-h", "127.0.0.1", "-p", &settings.services[&ServiceId::Postgresql].ports[0].to_string(), "-U", "postgres", "-d", "postgres", "-v", "ON_ERROR_STOP=1", "-c", "CREATE TABLE smoke (value text); INSERT INTO smoke VALUES ('postgres-ok'); SELECT * FROM smoke;"]);
    assert!(
        platform::checked_output(psql, Duration::from_secs(15))
            .await
            .unwrap()
            .contains("postgres-ok")
    );
    let my_version = settings.services[&ServiceId::Mysql]
        .active_version
        .as_deref()
        .unwrap();
    let my = engine.credentials(ServiceId::Mysql, my_version).unwrap();
    let mut mysql = platform::command(
        engine
            .storage
            .package(ServiceId::Mysql, my_version)
            .join("bin/mysql.exe"),
    );
    mysql.env("MYSQL_PWD", my.password).args(["--protocol=TCP", "-h", "127.0.0.1", "-P", &settings.services[&ServiceId::Mysql].ports[0].to_string(), "-u", "root", "-e", "CREATE DATABASE smoke; CREATE TABLE smoke.items (value TEXT); INSERT INTO smoke.items VALUES ('mysql-ok'); SELECT * FROM smoke.items;"]);
    assert!(
        platform::checked_output(mysql, Duration::from_secs(15))
            .await
            .unwrap()
            .contains("mysql-ok")
    );
    let mut redis = tokio::net::TcpStream::connect((
        Ipv4Addr::LOCALHOST,
        settings.services[&ServiceId::Redis].ports[0],
    ))
    .await
    .unwrap();
    redis.write_all(b"*1\r\n$4\r\nPING\r\n").await.unwrap();
    let mut reply = [0; 7];
    redis.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply, b"+PONG\r\n");
    drop(redis);
    assert!(
        http.get(format!(
            "http://127.0.0.1:{}/api/v1/messages",
            settings.services[&ServiceId::Mailpit].ports[1]
        ))
        .send()
        .await
        .unwrap()
        .status()
        .is_success()
    );
    for id in ServiceId::ALL {
        eprintln!("Restarting {}", id.key());
        engine.service_action(id, "restart").await.unwrap();
        engine.service_action(id, "stop").await.unwrap();
        for port in &settings.services[&id].ports {
            assert!(
                TcpListener::bind((Ipv4Addr::LOCALHOST, *port)).is_ok(),
                "Port {port} leaked by {id:?}"
            );
        }
    }
    let data = engine.storage.data(ServiceId::Mysql, my_version);
    engine
        .remove_package(ServiceId::Mysql, my_version.into())
        .await
        .unwrap();
    assert!(
        data.join("mysql").exists(),
        "Package removal must preserve database files"
    );
    eprintln!(
        "Verified all eight portable services, authenticated databases, PHP routing, trusted TLS, restart, shutdown and data preservation."
    );
}
