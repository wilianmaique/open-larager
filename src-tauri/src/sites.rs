use crate::{
    error::{Result, invalid},
    model::{ServiceId, Settings, Site, SiteMode, SiteServer},
    storage::{Storage, slash},
};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteConfig {
    pub content: String,
    pub generated: String,
    pub path: PathBuf,
    pub customized: bool,
}

pub fn path(storage: &Storage, site: &Site) -> Result<PathBuf> {
    // IDs originate in Rust. Validate again before deriving any filesystem path.
    uuid::Uuid::parse_str(&site.id).map_err(|_| invalid("Invalid domain identifier"))?;
    Ok(storage
        .root
        .join("config/sites")
        .join(&site.id)
        .join("nginx.conf"))
}

pub fn include(storage: &Storage) -> String {
    format!(
        "include {};",
        quoted(&storage.root.join("config/nginx/sites.conf"))
    )
}

fn quoted(path: &std::path::Path) -> String {
    // Nginx expands variables even inside quotes; reject them in project paths.
    format!(
        "\"{}\"",
        slash(path).replace('\\', "\\\\").replace('"', "\\\"")
    )
}

pub fn defaults(mode: &SiteMode) -> String {
    let route = match mode {
        SiteMode::Php => "    try_files $uri $uri/ /index.php?$query_string;",
        SiteMode::Static => "    try_files $uri $uri/ =404;",
        SiteMode::Proxy => {
            "    proxy_pass http://$larager_upstream;\n    proxy_http_version 1.1;\n    proxy_set_header Host $host;\n    proxy_set_header X-Forwarded-Proto $larager_scheme;\n    proxy_set_header X-Forwarded-For $remote_addr;\n    proxy_set_header Upgrade $http_upgrade;\n    proxy_set_header Connection $larager_connection;"
        }
    };
    format!(
        "index index.php index.html;\nclient_max_body_size 64m;\n\nlocation / {{\n{route}\n}}\n"
    )
}

const UPGRADE_MAP: &str =
    "map $http_upgrade $larager_connection {\n    default upgrade;\n    '' close;\n}\n";

pub fn validate_upstreams(settings: &Settings) -> Result<()> {
    for site in &settings.sites {
        if site.mode == SiteMode::Proxy {
            let port = site
                .upstream_port
                .ok_or_else(|| invalid("Missing upstream port"))?;
            if port == 0
                || settings.services[&ServiceId::Caddy].ports.contains(&port)
                || (site.server == SiteServer::Nginx
                    && port == settings.services[&ServiceId::Nginx].ports[0])
            {
                return Err(invalid(format!(
                    "The upstream port for {} would create a proxy loop",
                    site.hostname
                )));
            }
        }
    }
    Ok(())
}

// Parse the Nginx statement grammar before handing text to nginx -t. Only a
// bounded set of site directives is accepted; no file paths, modules, listeners
// or arbitrary upstreams can be supplied through this IPC surface.
#[derive(Debug)]
struct Statement {
    words: Vec<String>,
    children: Option<Vec<Statement>>,
    start: usize,
    end: usize,
}

#[derive(PartialEq)]
enum TokenKind {
    Word,
    Semicolon,
    Open,
    Close,
}
struct Token {
    value: String,
    start: usize,
    kind: TokenKind,
}

fn parse(content: &str) -> Result<Vec<Statement>> {
    if content.len() > 1024 * 1024 || content.contains('\0') {
        return Err(invalid(
            "Configuration exceeds 1 MiB or contains a NUL byte",
        ));
    }
    let mut tokens: Vec<Token> = Vec::new();
    let mut word = String::new();
    let mut start = None;
    let mut quote = None;
    let mut escape = false;
    let mut comment = false;
    for (offset, ch) in content.char_indices() {
        if comment {
            comment = ch != '\n';
            continue;
        }
        if escape {
            word.push(ch);
            escape = false;
            continue;
        }
        if ch == '\\' {
            start.get_or_insert(offset);
            escape = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if ch == delimiter {
                quote = None;
            } else {
                word.push(ch);
            }
            continue;
        }
        if ch == '"' || ch == '\'' {
            start.get_or_insert(offset);
            quote = Some(ch);
        } else if ch.is_whitespace() || matches!(ch, ';' | '{' | '}' | '#') {
            if let Some(start) = start.take() {
                tokens.push(Token {
                    value: std::mem::take(&mut word),
                    start,
                    kind: TokenKind::Word,
                });
            }
            if ch == '#' {
                comment = true;
            } else if !ch.is_whitespace() {
                tokens.push(Token {
                    value: ch.to_string(),
                    start: offset,
                    kind: match ch {
                        ';' => TokenKind::Semicolon,
                        '{' => TokenKind::Open,
                        _ => TokenKind::Close,
                    },
                });
            }
        } else {
            start.get_or_insert(offset);
            word.push(ch);
        }
    }
    if quote.is_some() || escape || start.is_some() {
        return Err(invalid("Unterminated Nginx directive or string"));
    }
    fn block(tokens: &[Token], cursor: &mut usize, depth: usize) -> Result<Vec<Statement>> {
        if depth > 8 {
            return Err(invalid("Nginx blocks are nested too deeply"));
        }
        let mut result = Vec::new();
        let mut words = Vec::new();
        let mut start = 0;
        while let Some(token) = tokens.get(*cursor) {
            *cursor += 1;
            match token.kind {
                TokenKind::Close if words.is_empty() && depth > 0 => return Ok(result),
                TokenKind::Close => return Err(invalid("Unexpected closing brace")),
                TokenKind::Semicolon | TokenKind::Open => {
                    if words.is_empty() {
                        return Err(invalid("Empty Nginx directive"));
                    }
                    let children = if token.kind == TokenKind::Open {
                        Some(block(tokens, cursor, depth + 1)?)
                    } else {
                        None
                    };
                    let end = if children.is_some() {
                        tokens[*cursor - 1].start
                    } else {
                        token.start
                    };
                    result.push(Statement {
                        words: std::mem::take(&mut words),
                        children,
                        start,
                        end,
                    });
                }
                TokenKind::Word => {
                    if words.is_empty() {
                        start = token.start;
                    }
                    words.push(token.value.clone());
                }
            }
        }
        if depth > 0 || !words.is_empty() {
            return Err(invalid("Unterminated Nginx block or directive"));
        }
        Ok(result)
    }
    block(&tokens, &mut 0, 0)
}

pub fn validate(content: &str, mode: &SiteMode) -> Result<()> {
    if content.len() > 64 * 1024 {
        return Err(invalid("Site rules exceed 64 KiB"));
    }
    fn check(statements: &[Statement], mode: &SiteMode) -> Result<()> {
        for statement in statements {
            if statement
                .words
                .iter()
                .any(|word| word.replace('\\', "/").split('/').any(|part| part == ".."))
            {
                return Err(invalid(
                    "Parent directory traversal is not supported in site rules",
                ));
            }
            let name = statement.words[0].as_str();
            if name == "index"
                && statement.words[1..]
                    .iter()
                    .any(|word| word.contains(['$', ':']))
            {
                return Err(invalid("Index entries must be fixed project file names"));
            }
            if name == "try_files" {
                for candidate in statement
                    .words
                    .iter()
                    .skip(1)
                    .take(statement.words.len().saturating_sub(2))
                {
                    let safe_variable = candidate == "$uri"
                        || candidate
                            .strip_prefix("$uri/")
                            .is_some_and(|suffix| !suffix.contains('$'));
                    if candidate.contains(':') || (candidate.contains('$') && !safe_variable) {
                        return Err(invalid(
                            "File candidates may use $uri, not request parameters or absolute filesystem paths",
                        ));
                    }
                }
            }
            if let Some(children) = &statement.children {
                if name != "location" || statement.words.len() < 2 {
                    return Err(invalid("Only location blocks are supported in site rules"));
                }
                check(children, mode)?;
                continue;
            }
            let allowed = matches!(
                name,
                "index"
                    | "try_files"
                    | "rewrite"
                    | "return"
                    | "error_page"
                    | "add_header"
                    | "expires"
                    | "client_max_body_size"
                    | "autoindex"
                    | "charset"
                    | "default_type"
                    | "etag"
                    | "if_modified_since"
                    | "limit_rate"
                    | "send_timeout"
                    | "proxy_http_version"
                    | "proxy_set_header"
                    | "proxy_read_timeout"
                    | "proxy_connect_timeout"
                    | "proxy_send_timeout"
                    | "proxy_buffering"
                    | "proxy_request_buffering"
                    | "proxy_redirect"
                    | "proxy_hide_header"
                    | "proxy_buffer_size"
                    | "proxy_buffers"
                    | "proxy_busy_buffers_size"
                    | "gzip"
                    | "gzip_types"
                    | "gzip_min_length"
                    | "gzip_vary"
            );
            if name == "proxy_pass"
                && *mode == SiteMode::Proxy
                && statement.words == ["proxy_pass", "http://$larager_upstream"]
            {
                continue;
            }
            if !allowed || statement.words.len() < 2 {
                return Err(invalid(format!(
                    "Unsupported site directive: {name}. Listener, root, logs and PHP upstream are managed by the app."
                )));
            }
        }
        Ok(())
    }
    check(&parse(content)?, mode)
}

pub fn render(storage: &Storage, site: &Site, settings: &Settings) -> Result<SiteConfig> {
    let path = path(storage, site)?;
    if slash(&site.root).contains(['$', '\n', '\r', '\0']) {
        return Err(invalid(
            "Project paths cannot contain $, line breaks or NUL bytes",
        ));
    }
    let content = site
        .nginx_directives
        .clone()
        .unwrap_or_else(|| defaults(&site.mode));
    validate(&content, &site.mode)?;
    let log_dir = storage.root.join("logs/sites").join(&site.id);
    let projects = storage.root.join("projects");
    let mut generated = format!(
        "# Generated by Open Larager. Edit site rules in Domains & HTTPS.\nserver {{\n    listen 127.0.0.1:{};\n    server_name {};\n    root {};\n    access_log {};\n    error_log {};\n    set $larager_scheme $scheme;\n    set $larager_https \"\";\n    set $larager_port $server_port;\n    if ($http_x_forwarded_proto = https) {{\n        set $larager_scheme https;\n        set $larager_https on;\n        set $larager_port {};\n    }}\n",
        settings.services[&ServiceId::Nginx].ports[0],
        site.hostname,
        quoted(if site.mode == SiteMode::Proxy {
            &projects
        } else {
            &site.root
        }),
        quoted(&log_dir.join("access.log")),
        quoted(&log_dir.join("error.log")),
        settings.services[&ServiceId::Caddy].ports[1]
    );
    generated.push_str("    location ~ /\\. { deny all; }\n");
    if site.mode == SiteMode::Php {
        generated.push_str(&format!(
            "    location ~* \\.php$ {{\n        try_files $uri =404;\n        fastcgi_pass 127.0.0.1:{};\n        fastcgi_index index.php;\n        fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n        fastcgi_param QUERY_STRING $query_string;\n        fastcgi_param REQUEST_METHOD $request_method;\n        fastcgi_param CONTENT_TYPE $content_type;\n        fastcgi_param CONTENT_LENGTH $content_length;\n        fastcgi_param SCRIPT_NAME $fastcgi_script_name;\n        fastcgi_param REQUEST_URI $request_uri;\n        fastcgi_param DOCUMENT_URI $document_uri;\n        fastcgi_param DOCUMENT_ROOT $document_root;\n        fastcgi_param SERVER_PROTOCOL $server_protocol;\n        fastcgi_param REQUEST_SCHEME $larager_scheme;\n        fastcgi_param HTTPS $larager_https if_not_empty;\n        fastcgi_param GATEWAY_INTERFACE CGI/1.1;\n        fastcgi_param SERVER_SOFTWARE nginx/$nginx_version;\n        fastcgi_param REMOTE_ADDR $remote_addr;\n        fastcgi_param REMOTE_PORT $remote_port;\n        fastcgi_param SERVER_ADDR $server_addr;\n        fastcgi_param SERVER_PORT $larager_port;\n        fastcgi_param SERVER_NAME $host;\n        fastcgi_param REDIRECT_STATUS 200;\n        fastcgi_param HTTP_PROXY \"\";\n    }}\n",
            settings.services[&ServiceId::Php].ports[0]
        ));
    } else if site.mode == SiteMode::Static {
        generated.push_str("    location ~* \\.php(?:/|$) { return 404; }\n");
    }
    // The editor placeholder becomes a literal address. Regex captures in user
    // locations must not be able to override a variable and change the upstream.
    let rendered_rules = if site.mode == SiteMode::Proxy {
        content.replace(
            "$larager_upstream",
            &format!(
                "127.0.0.1:{}",
                site.upstream_port
                    .ok_or_else(|| invalid("Missing upstream port"))?
            ),
        )
    } else {
        content.clone()
    };
    for line in rendered_rules.lines() {
        generated.push_str(&format!("    {line}\n"));
    }
    generated.push_str("}\n");
    Ok(SiteConfig {
        content,
        generated,
        path,
        customized: site.nginx_directives.is_some(),
    })
}

pub fn files(storage: &Storage, settings: &Settings) -> Result<Vec<(PathBuf, String)>> {
    let mut files = Vec::new();
    let mut enabled = format!("# Managed site includes.\n{UPGRADE_MAP}");
    for site in &settings.sites {
        if site.server != SiteServer::Nginx {
            continue;
        }
        let config = render(storage, site, settings)?;
        if site.enabled {
            enabled.push_str(&format!("include {};\n", quoted(&config.path)));
        }
        std::fs::create_dir_all(storage.root.join("logs/sites").join(&site.id))?;
        files.push((config.path, config.generated));
    }
    files.push((storage.root.join("config/nginx/sites.conf"), enabled));
    Ok(files)
}

pub fn attach(main: &str, storage: &Storage) -> Result<String> {
    let directive = include(storage);
    let statements = parse(main)?;
    let http = statements
        .iter()
        .find(|s| s.words == ["http"] && s.children.is_some())
        .ok_or_else(|| invalid("Nginx configuration needs an http block for local sites"))?;
    let expected = slash(&storage.root.join("config/nginx/sites.conf"));
    if let Some(existing) = http
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|s| s.words == ["include", &expected])
    {
        let mut updated = main.to_owned();
        updated.replace_range(existing.start..=existing.end, &directive);
        return Ok(updated);
    }
    let mut updated = main.to_owned();
    updated.insert_str(http.end, &format!("    {directive}\n"));
    Ok(updated)
}

pub fn inline(main: &str, storage: &Storage, settings: &Settings) -> Result<String> {
    let mut servers = UPGRADE_MAP.to_owned();
    for site in &settings.sites {
        // Check disabled sites too, so invalid rules cannot be saved while off.
        if site.server == SiteServer::Nginx {
            servers.push_str(&render(storage, site, settings)?.generated);
        }
    }
    Ok(attach(main, storage)?.replace(&include(storage), &servers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site(mode: SiteMode) -> Site {
        Site {
            id: uuid::Uuid::new_v4().to_string(),
            hostname: "app.test".into(),
            root: "C:/Projects/My app/public".into(),
            mode,
            upstream_port: Some(3000),
            enabled: true,
            server: SiteServer::Nginx,
            nginx_directives: None,
        }
    }

    #[test]
    fn confines_rules_to_site_routing_and_local_managed_upstream() {
        for content in [
            "listen 0.0.0.0:80;",
            "root C:/;",
            "include C:/secret;",
            "alias C:/;",
            "location / { alias C:/; }",
            "} server { listen 0.0.0.0:80; }",
            "load_module evil.dll;",
            "proxy_pass http://example.com;",
            "fastcgi_pass 192.168.1.1:9000;",
            "location / { try_files ../../secrets =404; }",
            "location / { return 200;",
            "return 200",
            "# comment\ninclude secret;",
            "access_log C:/outside.log;",
            "location / { try_files $arg_path =404; }",
            "location / { try_files $uri_argument =404; }",
            "index $arg_path;",
        ] {
            assert!(validate(content, &SiteMode::Php).is_err(), "{content}");
        }
        for mode in [SiteMode::Php, SiteMode::Static, SiteMode::Proxy] {
            validate(&defaults(&mode), &mode).unwrap();
        }
        validate(
            "location ~ \"[{}]$\" { add_header X-Test \"a; # }\"; return 204; }",
            &SiteMode::Static,
        )
        .unwrap();
        assert!(validate(&"#".repeat(65537), &SiteMode::Static).is_err());
    }

    #[test]
    fn attaches_once_inside_http_and_preserves_existing_main_edits() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let main = "# http { fake }\nevents {}\nhttp {\n    server { location / { return 200 \"}\"; } }\n    keepalive_timeout 42;\n}\n";
        let attached = attach(main, &storage).unwrap();
        assert!(attached.contains("keepalive_timeout 42;"));
        assert_eq!(attach(&attached, &storage).unwrap(), attached);
        assert_eq!(attached.matches(&include(&storage)).count(), 1);
        assert!(attach("events {}", &storage).is_err());
        let edited = attached.replace("include ", "include     ");
        assert!(
            !inline(&edited, &storage, &Settings::default())
                .unwrap()
                .contains("sites.conf")
        );
    }

    #[test]
    fn generates_isolated_files_and_does_not_include_disabled_sites() {
        let workspace = tempfile::tempdir().unwrap();
        let storage = Storage::open(workspace.path().to_owned()).unwrap();
        let mut settings = Settings::default();
        let mut php = site(SiteMode::Php);
        php.nginx_directives = Some(
            "client_max_body_size 128m;\nlocation / { try_files $uri /index.php?$query_string; }"
                .into(),
        );
        let generated = render(&storage, &php, &settings).unwrap();
        assert!(generated.generated.contains("listen 127.0.0.1:8081;"));
        assert!(
            generated
                .generated
                .contains("root \"C:/Projects/My app/public\";")
        );
        assert!(generated.generated.contains("try_files $uri =404;"));
        assert!(
            generated
                .generated
                .contains("fastcgi_param HTTPS $larager_https")
        );
        assert!(generated.generated.contains("128m"));
        let mut disabled = site(SiteMode::Static);
        disabled.enabled = false;
        settings.sites = vec![php.clone(), disabled.clone()];
        let files = files(&storage, &settings).unwrap();
        let includes = &files.last().unwrap().1;
        assert!(includes.contains(&php.id));
        assert!(!includes.contains(&disabled.id));
        let mut unsafe_site = php;
        unsafe_site.id = "../../outside".into();
        assert!(path(&storage, &unsafe_site).is_err());
        unsafe_site.id = uuid::Uuid::new_v4().to_string();
        unsafe_site.root = "C:/$nginx_version".into();
        assert!(render(&storage, &unsafe_site, &settings).is_err());
    }

    #[tokio::test]
    async fn preserves_legacy_sites_and_creates_base_config_before_installation() {
        let legacy: Site = serde_json::from_str(r#"{"id":"legacy","hostname":"old.test","root":"C:/site","mode":"php","upstreamPort":null,"enabled":true}"#).unwrap();
        assert_eq!(legacy.server, SiteServer::Caddy);
        let workspace = tempfile::tempdir().unwrap();
        let engine = crate::engine::Engine::open(workspace.path().to_owned()).unwrap();
        let mut new_site = site(SiteMode::Php);
        new_site.id.clear();
        new_site.root = workspace.path().join("projects");
        engine.save_site(new_site).await.unwrap();
        let settings = engine.settings.read().await.clone();
        let site = &settings.sites[0];
        let generated = engine.read_site_config(site.id.clone()).await.unwrap();
        assert!(generated.path.exists());
        assert!(
            engine
                .save_site_config(site.id.clone(), Some("return 204;".into()))
                .await
                .is_err()
        );
        engine.remove_site(site.id.clone()).await.unwrap();
        assert!(
            generated.path.exists(),
            "Removal must retain site configuration"
        );
        assert!(
            !std::fs::read_to_string(storage_include_path(&engine.storage))
                .unwrap()
                .contains(&site.id)
        );
    }

    fn storage_include_path(storage: &Storage) -> PathBuf {
        storage.root.join("config/nginx/sites.conf")
    }
}
