use larager::{
    config,
    engine::Engine,
    model::{Installation, ServiceId, Settings},
    php_tls,
    storage::{Storage, atomic_write},
};
use std::fs;

#[test]
fn upgrades_installed_ini_with_backup_and_preserves_explicit_overrides() {
    let temporary = tempfile::tempdir().unwrap();
    let root = temporary.path().join("environment");
    let storage = Storage::open(root.clone()).unwrap();
    let mut settings = Settings::default();
    let php = settings.services.get_mut(&ServiceId::Php).unwrap();
    php.active_version = Some("8.4.0".into());
    php.installations.push(Installation {
        version: "8.4.0".into(),
        installed_at: 0,
        sha256: "0".repeat(64),
    });
    storage.save(&settings).unwrap();
    storage.prepare(ServiceId::Php, "8.4.0").unwrap();
    let ini = storage.config(ServiceId::Php, "8.4.0");
    let before = "; custom configuration\r\nmemory_limit=512M\r\nextension=curl\r\nextension=openssl\r\n[curl]\r\ncurl.cainfo=\r\n[openssl]\r\nopenssl.cafile=\"\"\r\n";
    fs::write(&ini, before).unwrap();
    drop(storage);
    let engine = Engine::open(root.clone()).unwrap();
    let after = fs::read_to_string(&ini).unwrap();
    assert!(after.contains("mozilla-"));
    assert!(after.contains("memory_limit=512M\r\n"));
    assert_eq!(
        fs::read_to_string(ini.with_extension("previous")).unwrap(),
        before
    );
    config::ensure(&engine.storage, ServiceId::Php, "8.4.0", &settings).unwrap();
    assert_eq!(
        fs::read_to_string(ini.with_extension("previous")).unwrap(),
        before
    );
    let custom = "[curl]\ncurl.cainfo=\"C:/Company/roots.pem\"\n[openssl]\nopenssl.cafile=\"C:/Company/roots.pem\"\n";
    fs::write(&ini, custom).unwrap();
    drop(engine);
    let reopened = Engine::open(root).unwrap();
    assert_eq!(fs::read_to_string(ini).unwrap(), custom);
    assert!(php_tls::bundle_path(&reopened.storage).unwrap().is_file());
}

#[cfg(windows)]
mod windows {
    use super::*;
    use larager::{catalog, platform, toolchain, workspace::Workspace};
    use std::{
        net::{Ipv4Addr, TcpListener},
        os::windows::process::CommandExt,
        path::Path,
        time::Duration,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn cli(engine: &Engine, version: &str) -> serde_json::Value {
        let settings = engine.settings.read().await;
        for (path, content) in toolchain::layout(&engine.storage, &settings, ServiceId::Php)
            .unwrap()
            .files
        {
            atomic_write(&path, content.as_bytes()).unwrap();
        }
        drop(settings);
        let launcher = engine.storage.root.join("config/cli/php/php.cmd");
        let script = engine.storage.root.join("projects/tls.php");
        let mut command = platform::command("cmd.exe");
        command.args(["/D", "/S", "/C"]).raw_arg(format!(
            "\"\"{}\" \"{}\"\"",
            launcher.display(),
            script.display()
        ));
        let output = platform::checked_output(command, Duration::from_secs(120))
            .await
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(output.trim()).unwrap();
        assert_eq!(value["version"], version.split('-').next().unwrap());
        verify(&value);
        value
    }

    fn verify(value: &serde_json::Value) {
        assert_eq!(value["cainfo"], value["cafile"]);
        assert!(!value["cainfo"].as_str().unwrap().is_empty());
        for host in ["accounts.google.com", "steamcommunity.com"] {
            assert_eq!(value[host]["curl"]["error"], 0, "{value}");
            assert_eq!(value[host]["curl"]["verify"], 0, "{value}");
            assert_eq!(value[host]["stream"], true, "{value}");
        }
        assert_eq!(value["untrusted"]["curl"]["error"], 60, "{value}");
        assert_eq!(value["untrusted"]["stream"], false, "{value}");
    }

    fn record(kind: u8, content: &[u8]) -> Vec<u8> {
        let mut record = vec![
            1,
            kind,
            0,
            1,
            (content.len() >> 8) as u8,
            content.len() as u8,
            0,
            0,
        ];
        record.extend_from_slice(content);
        record
    }

    fn length(bytes: &mut Vec<u8>, size: usize) {
        if size < 128 {
            bytes.push(size as u8);
        } else {
            bytes.extend_from_slice(&((size as u32) | 0x80000000).to_be_bytes());
        }
    }

    async fn fastcgi(port: u16, script: &Path) -> serde_json::Value {
        let mut socket = tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, port))
            .await
            .unwrap();
        socket
            .write_all(&record(1, &[0, 1, 0, 0, 0, 0, 0, 0]))
            .await
            .unwrap();
        let mut params = Vec::new();
        for (name, value) in [
            ("SCRIPT_FILENAME", script.to_str().unwrap()),
            ("SCRIPT_NAME", "/tls.php"),
            ("REQUEST_METHOD", "GET"),
            ("SERVER_PROTOCOL", "HTTP/1.1"),
            ("GATEWAY_INTERFACE", "CGI/1.1"),
            ("REDIRECT_STATUS", "200"),
        ] {
            length(&mut params, name.len());
            length(&mut params, value.len());
            params.extend_from_slice(name.as_bytes());
            params.extend_from_slice(value.as_bytes());
        }
        for bytes in [record(4, &params), record(4, &[]), record(5, &[])] {
            socket.write_all(&bytes).await.unwrap();
        }
        let mut output = Vec::new();
        loop {
            let mut header = [0; 8];
            socket.read_exact(&mut header).await.unwrap();
            let size = u16::from_be_bytes([header[4], header[5]]) as usize;
            let mut data = vec![0; size + header[6] as usize];
            socket.read_exact(&mut data).await.unwrap();
            match header[1] {
                3 => break,
                6 => output.extend_from_slice(&data[..size]),
                7 => assert_eq!(size, 0, "{}", String::from_utf8_lossy(&data)),
                _ => {}
            }
        }
        let text = String::from_utf8(output).unwrap();
        let (_, body) = text.split_once("\r\n\r\n").expect(&text);
        let value = serde_json::from_str(body).expect(body);
        verify(&value);
        value
    }

    #[tokio::test]
    #[ignore = "Downloads pinned PHP into a temporary Unicode workspace; verifies public HTTPS, rejected self-signed certificates, real FastCGI, CLI and relocation. Never changes Windows trust."]
    async fn real_php_https_trust_cli_fastcgi_and_relocation() {
        let temporary = tempfile::Builder::new()
            .prefix("Larager TLS á ! & ")
            .tempdir()
            .unwrap();
        let source = temporary.path().join("source é");
        let workspace = Workspace::open(source, temporary.path().join("location.json")).unwrap();
        let php = catalog::packages()
            .unwrap()
            .into_iter()
            .find(|package| package.service == ServiceId::Php)
            .unwrap();
        let reservation = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = reservation.local_addr().unwrap().port();
        drop(reservation);
        let engine = workspace.engine.read().await;
        engine
            .save_service(ServiceId::Php, None, vec![port], false)
            .await
            .unwrap();
        engine
            .install(ServiceId::Php, php.version.clone(), |_| {})
            .await
            .unwrap();
        let script = engine.storage.root.join("projects/tls.php");
        fs::write(&script, r#"<?php
function checkCurl($host) {
    $curl = curl_init('https://' . $host . '/');
    curl_setopt_array($curl, [CURLOPT_NOBODY => true, CURLOPT_RETURNTRANSFER => true, CURLOPT_CONNECTTIMEOUT => 15, CURLOPT_TIMEOUT => 30]);
    curl_exec($curl);
    return ['error' => curl_errno($curl), 'verify' => curl_getinfo($curl, CURLINFO_SSL_VERIFYRESULT), 'message' => curl_error($curl)];
}
function checkStream($host) {
    $stream = @stream_socket_client('tls://' . $host . ':443', $code, $message, 20, STREAM_CLIENT_CONNECT);
    if ($stream === false) return false;
    fclose($stream);
    return true;
}
$out = ['version' => PHP_VERSION, 'cainfo' => ini_get('curl.cainfo'), 'cafile' => ini_get('openssl.cafile'), 'ini' => php_ini_loaded_file()];
foreach (['accounts.google.com', 'steamcommunity.com'] as $host) {
    $out[$host] = ['curl' => checkCurl($host), 'stream' => checkStream($host)];
}
$out['untrusted'] = ['curl' => checkCurl('self-signed.badssl.com'), 'stream' => checkStream('self-signed.badssl.com')];
echo json_encode($out, JSON_THROW_ON_ERROR);
"#).unwrap();
        println!("Verifying PHP {} CLI and public HTTPS", php.version);
        let result = cli(&engine, &php.version).await;
        assert!(Path::new(result["cainfo"].as_str().unwrap()).is_file());
        engine
            .service_action(ServiceId::Php, "start")
            .await
            .unwrap();
        println!("Verifying the managed FastCGI process");
        let result = tokio::time::timeout(Duration::from_secs(120), fastcgi(port, &script))
            .await
            .unwrap();
        assert_eq!(
            Path::new(result["ini"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            engine
                .storage
                .config(ServiceId::Php, &php.version)
                .canonicalize()
                .unwrap()
        );
        engine.service_action(ServiceId::Php, "stop").await.unwrap();
        drop(engine);
        let target = temporary.path().join("moved ó");
        fs::create_dir(&target).unwrap();
        let selected = workspace.select(Some(target)).await.unwrap().unwrap();
        let status = workspace.relocate(selected.token).await.unwrap();
        assert!(
            status.cleanup_source.is_none(),
            "{:?}",
            status.cleanup_error
        );
        let engine = workspace.engine.read().await;
        println!("Verifying PHP after environment relocation");
        cli(&engine, &php.version).await;
        engine.shutdown().await;
    }
}
