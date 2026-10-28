#![cfg(windows)]
use larager::{
    catalog,
    engine::Engine,
    model::{ServiceId, Site, SiteMode, SiteServer},
    storage::slash,
};
use std::{
    fs,
    net::{Ipv4Addr, TcpListener},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "Downloads pinned Nginx, PHP and Caddy into a temporary workspace; never changes hosts or trusted certificates."]
async fn nginx_sites_routing_validation_and_persistence() {
    let workspace = tempfile::Builder::new()
        .prefix("Larager sites á ")
        .tempdir()
        .unwrap();
    let engine = Engine::open(workspace.path().to_owned()).unwrap();
    let upstream = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let upstream_port = upstream.local_addr().unwrap().port();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = requests.clone();
    let upstream_task = tokio::spawn(async move {
        while let Ok((mut socket, _)) = upstream.accept().await {
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(request).unwrap().to_lowercase();
            let upgrade = request.contains("upgrade: websocket");
            captured.lock().unwrap().push(request);
            let response = if upgrade {
                "HTTP/1.1 101 Switching Protocols\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n"
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 13\r\nConnection: close\r\n\r\nproxy-site-ok"
            };
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    let sockets: Vec<_> = (0..5)
        .map(|_| TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap())
        .collect();
    let ports: Vec<_> = sockets
        .iter()
        .map(|s| s.local_addr().unwrap().port())
        .collect();
    drop(sockets);
    for (id, ports) in [
        (ServiceId::Php, vec![ports[0]]),
        (ServiceId::Nginx, vec![ports[1]]),
        (ServiceId::Caddy, ports[2..5].to_vec()),
    ] {
        engine.save_service(id, None, ports, false).await.unwrap();
    }
    let php_root = workspace.path().join("projects/php public");
    let static_root = workspace.path().join("projects/static public");
    fs::create_dir_all(&php_root).unwrap();
    fs::create_dir_all(&static_root).unwrap();
    fs::write(php_root.join("index.php"), "<?php header('Content-Type: application/json'); echo json_encode(['route'=>$_SERVER['REQUEST_URI'], 'https'=>$_SERVER['HTTPS'] ?? '', 'host'=>$_SERVER['HTTP_HOST'], 'port'=>$_SERVER['SERVER_PORT']]);").unwrap();
    fs::write(php_root.join(".env"), "secret").unwrap();
    fs::write(static_root.join("index.html"), "static-site-ok").unwrap();
    fs::write(static_root.join("secret.php"), "<?php secret").unwrap();
    for (hostname, root, mode) in [
        ("php.larager.test", php_root, SiteMode::Php),
        ("static.larager.test", static_root, SiteMode::Static),
    ] {
        engine
            .save_site(Site {
                id: String::new(),
                hostname: hostname.into(),
                root,
                mode,
                upstream_port: None,
                enabled: true,
                server: SiteServer::Nginx,
                nginx_directives: None,
            })
            .await
            .unwrap();
    }
    engine
        .save_site(Site {
            id: String::new(),
            hostname: "proxy.larager.test".into(),
            root: Default::default(),
            mode: SiteMode::Proxy,
            upstream_port: Some(upstream_port),
            enabled: true,
            server: SiteServer::Nginx,
            nginx_directives: None,
        })
        .await
        .unwrap();
    for id in [ServiceId::Php, ServiceId::Nginx, ServiceId::Caddy] {
        let package = catalog::packages()
            .unwrap()
            .into_iter()
            .find(|p| p.service == id)
            .unwrap();
        engine.install(id, package.version, |_| {}).await.unwrap();
    }
    let php = engine.settings.read().await.sites[0].clone();
    let original = engine.read_site_config(php.id.clone()).await.unwrap();
    let invalid = format!(
        "{}\nclient_max_body_size invalid_value;",
        original.content.replace("client_max_body_size 64m;", "")
    );
    assert!(
        engine
            .save_site_config(php.id.clone(), Some(invalid.clone()))
            .await
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(&original.path).unwrap(),
        original.generated
    );
    assert!(
        engine.settings.read().await.sites[0]
            .nginx_directives
            .is_none()
    );
    let custom =
        original.content.replace("64m", "128m") + "\nadd_header X-Larager-Test saved always;\n";
    engine
        .save_site_config(php.id.clone(), Some(custom.clone()))
        .await
        .unwrap();
    assert!(original.path.with_extension("previous").exists());
    let nginx_version = engine.settings.read().await.services[&ServiceId::Nginx]
        .active_version
        .clone();
    // Regenerating the global service config must retain the per-site custom rules.
    engine
        .save_service(ServiceId::Nginx, nginx_version, vec![ports[1]], true)
        .await
        .unwrap();
    for id in [ServiceId::Php, ServiceId::Nginx, ServiceId::Caddy] {
        engine.service_action(id, "start").await.unwrap();
    }
    let ca = reqwest::Certificate::from_pem(&fs::read(engine.certificate_path()).unwrap()).unwrap();
    let client = reqwest::Client::builder()
        .tls_certs_only([ca])
        .no_proxy()
        .timeout(Duration::from_secs(15))
        .resolve("php.larager.test", (Ipv4Addr::LOCALHOST, ports[3]).into())
        .resolve("proxy.larager.test", (Ipv4Addr::LOCALHOST, ports[3]).into())
        .resolve(
            "static.larager.test",
            (Ipv4Addr::LOCALHOST, ports[3]).into(),
        )
        .build()
        .unwrap();
    let url = format!("https://php.larager.test:{}/nested/route?hello=1", ports[3]);
    let response = client.get(url).send().await.unwrap();
    assert_eq!(response.headers().get("x-larager-test").unwrap(), "saved");
    assert!(
        response.status().is_success(),
        "{}",
        engine.logs(ServiceId::Nginx).await.unwrap()
    );
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["route"], "/nested/route?hello=1");
    assert_eq!(json["https"], "on");
    assert_eq!(json["host"], "php.larager.test");
    assert_eq!(json["port"], ports[3].to_string());
    for (host, path, expected) in [
        ("php", "/missing.php", 404),
        ("php", "/.env", 403),
        ("static", "/missing", 404),
        ("static", "/secret.php", 404),
        ("static", "/secret.PHP", 404),
    ] {
        assert_eq!(
            client
                .get(format!("https://{host}.larager.test:{}{path}", ports[3]))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            expected
        );
    }
    assert_eq!(
        client
            .get(format!("https://static.larager.test:{}/", ports[3]))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "static-site-ok"
    );
    let response = client
        .get(format!("https://php.larager.test:{}/index.PHP", ports[3]))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["https"],
        "on"
    );
    let proxy_url = format!("https://proxy.larager.test:{}/", ports[3]);
    assert_eq!(
        client
            .get(&proxy_url)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
        "proxy-site-ok"
    );
    assert_eq!(
        client
            .get(&proxy_url)
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .send()
            .await
            .unwrap()
            .status()
            .as_u16(),
        101
    );
    {
        let requests = requests.lock().unwrap();
        assert!(requests[0].contains("host: proxy.larager.test"));
        assert!(requests[0].contains("x-forwarded-proto: https"));
        assert!(requests[0].contains("connection: close"));
        assert!(requests[1].contains("connection: upgrade"));
    }
    assert!(
        engine.save_site_config(php.id.clone(), None).await.is_err(),
        "Running Nginx must block writes"
    );
    for id in [ServiceId::Caddy, ServiceId::Nginx, ServiceId::Php] {
        engine.service_action(id, "stop").await.unwrap();
    }
    let nginx_port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let version = engine.settings.read().await.services[&ServiceId::Nginx]
        .active_version
        .clone();
    assert!(
        engine
            .save_service(
                ServiceId::Nginx,
                version.clone(),
                vec![upstream_port],
                false
            )
            .await
            .is_err()
    );
    engine
        .save_service(ServiceId::Nginx, version.clone(), vec![nginx_port], false)
        .await
        .unwrap();
    let updated = engine.read_site_config(php.id.clone()).await.unwrap();
    assert!(
        updated
            .generated
            .contains(&format!("listen 127.0.0.1:{nginx_port};"))
    );
    assert_eq!(updated.content, custom);
    let caddy = engine.read_config(ServiceId::Caddy).await.unwrap();
    assert!(caddy.contains(&format!("reverse_proxy 127.0.0.1:{nginx_port}")));
    engine.save_site_config(php.id.clone(), None).await.unwrap();
    assert!(
        !engine
            .read_site_config(php.id.clone())
            .await
            .unwrap()
            .customized
    );
    let mut disabled = php.clone();
    disabled.enabled = false;
    engine.save_site(disabled).await.unwrap();
    assert!(
        engine
            .save_site_config(php.id.clone(), Some(invalid))
            .await
            .is_err(),
        "Disabled sites still require nginx -t validation"
    );
    assert!(
        !fs::read_to_string(engine.storage.root.join("config/nginx/sites.conf"))
            .unwrap()
            .contains(&php.id)
    );
    engine.remove_site(php.id).await.unwrap();
    assert!(original.path.exists());
    assert!(
        workspace
            .path()
            .join("projects/php public/index.php")
            .exists()
    );
    engine
        .remove_package(ServiceId::Nginx, version.unwrap())
        .await
        .unwrap();
    assert!(original.path.exists());
    let root = slash(&engine.storage.root);
    drop(engine);
    let reopened = Engine::open(root.into()).unwrap();
    assert_eq!(reopened.settings.read().await.sites.len(), 2);
    upstream_task.abort();
}
