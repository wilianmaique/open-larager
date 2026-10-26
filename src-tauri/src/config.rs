use crate::{
    error::{Result, invalid},
    model::{ServiceId, Settings, SiteMode},
    storage::{Storage, atomic_write, slash},
};
use std::{fs, path::Path};

fn quote(path: &Path) -> String {
    serde_json::to_string(&slash(path)).expect("path serialization")
}

pub fn ensure(storage: &Storage, id: ServiceId, version: &str, settings: &Settings) -> Result<()> {
    storage.prepare(id, version)?;
    let path = storage.config(id, version);
    if !path.exists() || id == ServiceId::Caddy {
        atomic_write(&path, generate(storage, id, version, settings)?.as_bytes())?;
    }
    Ok(())
}

pub fn generate(
    storage: &Storage,
    id: ServiceId,
    version: &str,
    settings: &Settings,
) -> Result<String> {
    let service = settings
        .services
        .get(&id)
        .ok_or_else(|| invalid("Missing service settings"))?;
    let ports = &service.ports;
    let package = storage.package(id, version);
    let data = storage.data(id, version);
    let logs = storage.logs(id, version);
    let projects = storage.root.join("projects");
    let config_dir = storage.config_dir(id, version);
    let php_port = settings.services[&ServiceId::Php].ports[0];
    Ok(match id {
        ServiceId::Php => format!(
            "; Managed defaults. Stop PHP before editing.\n[PHP]\nextension_dir={}\ndate.timezone=UTC\nmemory_limit=256M\nmax_execution_time=120\nupload_max_filesize=64M\npost_max_size=64M\ndisplay_errors=On\nlog_errors=On\nerror_log={}\ncgi.force_redirect=0\ncgi.fix_pathinfo=1\nexpose_php=Off\nextension=curl\nextension=fileinfo\nextension=mbstring\nextension=openssl\nextension=pdo_mysql\nextension=pdo_pgsql\nextension=pgsql\nextension=mysqli\nextension=zip\n[Session]\nsession.save_path={}\n[mail function]\nSMTP=127.0.0.1\nsmtp_port={}\n",
            quote(&package.join("ext")),
            quote(&logs.join("php-error.log")),
            quote(&data),
            settings.services[&ServiceId::Mailpit].ports[0]
        ),
        ServiceId::Apache => format!(
            r#"ServerRoot {}
Listen 127.0.0.1:{}
ServerName localhost
PidFile {}
LoadModule authz_core_module modules/mod_authz_core.so
LoadModule authz_host_module modules/mod_authz_host.so
LoadModule dir_module modules/mod_dir.so
LoadModule mime_module modules/mod_mime.so
LoadModule log_config_module modules/mod_log_config.so
LoadModule rewrite_module modules/mod_rewrite.so
LoadModule proxy_module modules/mod_proxy.so
LoadModule proxy_fcgi_module modules/mod_proxy_fcgi.so
TypesConfig {}
DocumentRoot {}
<Directory {}>
    AllowOverride All
    Require local
    Options -Indexes +FollowSymLinks
</Directory>
DirectoryIndex index.php index.html
ProxyFCGIBackendType GENERIC
ProxyFCGISetEnvIf "reqenv('SCRIPT_FILENAME') =~ m|^/([A-Za-z]:/.*)$|" SCRIPT_FILENAME "$1"
<FilesMatch \.php$>
    SetHandler "proxy:fcgi://127.0.0.1:{}/"
</FilesMatch>
ErrorLog {}
LogFormat "%h %l %u %t \"%r\" %>s %b" common
CustomLog {} common
"#,
            quote(&package),
            ports[0],
            quote(&config_dir.join("httpd.pid")),
            quote(&package.join("conf/mime.types")),
            quote(&projects),
            quote(&projects),
            php_port,
            quote(&logs.join("error.log")),
            quote(&logs.join("access.log"))
        ),
        ServiceId::Mysql => format!(
            "[mysqld]\nbasedir={}\ndatadir={}\nport={}\nbind-address=127.0.0.1\nmysqlx=0\nlocal_infile=0\nlog-error={}\ncharacter-set-server=utf8mb4\ncollation-server=utf8mb4_0900_ai_ci\n",
            serde_json::to_string(&crate::platform::runtime_path(&package)?)?,
            serde_json::to_string(&crate::platform::runtime_path(&data)?)?,
            ports[0],
            serde_json::to_string(&crate::platform::runtime_path(
                &logs.join("mysql-error.log")
            )?)?
        ),
        ServiceId::Postgresql => format!(
            "listen_addresses = '127.0.0.1'\nport = {}\npassword_encryption = 'scram-sha-256'\nlogging_collector = on\nlog_directory = '{}'\nlog_filename = 'postgresql.log'\nmax_connections = 100\nshared_buffers = '128MB'\n",
            ports[0],
            crate::platform::runtime_path(&logs)?.replace('\'', "''")
        ),
        ServiceId::Nginx => format!(
            "worker_processes 1;\npid {};\nerror_log {};\nevents {{ worker_connections 1024; }}\nhttp {{\n    include {};\n    default_type application/octet-stream;\n    access_log {};\n    client_body_temp_path {};\n    proxy_temp_path {};\n    fastcgi_temp_path {};\n    scgi_temp_path {};\n    uwsgi_temp_path {};\n    server {{\n        listen 127.0.0.1:{};\n        server_name localhost;\n        root {};\n        index index.php index.html;\n        location / {{ try_files $uri $uri/ /index.php?$query_string; }}\n        location ~ \\.php$ {{\n            try_files $uri =404;\n            include {};\n            fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n            fastcgi_pass 127.0.0.1:{};\n        }}\n    }}\n}}\n",
            quote(&config_dir.join("nginx.pid")),
            quote(&logs.join("error.log")),
            quote(&package.join("conf/mime.types")),
            quote(&logs.join("access.log")),
            quote(&data.join("client")),
            quote(&data.join("proxy")),
            quote(&data.join("fastcgi")),
            quote(&data.join("scgi")),
            quote(&data.join("uwsgi")),
            ports[0],
            quote(&projects),
            quote(&package.join("conf/fastcgi_params")),
            php_port
        ),
        ServiceId::Redis => format!(
            "bind 127.0.0.1\nprotected-mode yes\nport {}\ndir {}\nappendonly yes\nappendfsync everysec\nsave 900 1\nsave 300 10\nlogfile {}\n",
            ports[0],
            serde_json::to_string(&crate::storage::cygwin_path(&data))?,
            serde_json::to_string(&crate::storage::cygwin_path(&logs.join("redis.log")))?
        ),
        ServiceId::Mailpit => "{\n  \"maxMessages\": 500,\n  \"verbose\": false\n}\n".into(),
        ServiceId::Caddy => {
            let mut out = format!(
                "{{\n    admin 127.0.0.1:{}\n    default_bind 127.0.0.1\n    http_port {}\n    https_port {}\n    skip_install_trust\n    pki {{\n        ca local {{\n            name \"Open Larager Local Authority\"\n        }}\n    }}\n}}\n\n",
                ports[2], ports[0], ports[1]
            );
            let sites: Vec<_> = settings.sites.iter().filter(|s| s.enabled).collect();
            if sites.is_empty() {
                out.push_str(&format!("https://localhost:{} {{\n    bind 127.0.0.1\n    tls internal\n    respond \"Open Larager HTTPS gateway is ready.\" 200\n}}\n", ports[1]));
            }
            for site in sites {
                out.push_str(&format!("https://{}:{} {{\n    bind 127.0.0.1\n    tls internal\n    encode zstd gzip\n", site.hostname, ports[1]));
                match site.mode {
                    SiteMode::Static => out.push_str(&format!(
                        "    root * {}\n    file_server\n",
                        quote(&site.root)
                    )),
                    SiteMode::Php => out.push_str(&format!(
                        "    root * {}\n    php_fastcgi 127.0.0.1:{}\n    file_server\n",
                        quote(&site.root),
                        php_port
                    )),
                    SiteMode::Proxy => out.push_str(&format!(
                        "    reverse_proxy 127.0.0.1:{}\n",
                        site.upstream_port
                            .ok_or_else(|| invalid("Missing upstream port"))?
                    )),
                }
                out.push_str("}\n\n");
            }
            out
        }
    })
}

pub fn read(storage: &Storage, id: ServiceId, version: &str) -> Result<String> {
    let path = storage.config(id, version);
    if fs::metadata(&path)?.len() > 1024 * 1024 {
        return Err(invalid("Configuration exceeds 1 MiB"));
    }
    Ok(fs::read_to_string(path)?)
}
