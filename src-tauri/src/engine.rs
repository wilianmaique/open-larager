use crate::{
    catalog, config,
    error::{Result, invalid},
    installer,
    model::*,
    php_extensions,
    platform::{self, Process},
    sites,
    storage::{Storage, atomic_write, slash},
    toolchain, user_path,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, RwLock},
};

struct Running {
    process: Process,
    version: String,
    started: Instant,
}
pub struct Engine {
    pub storage: Storage,
    pub settings: RwLock<Settings>,
    operation: Mutex<()>,
    running: Mutex<BTreeMap<ServiceId, Running>>,
    cancel: StdMutex<Option<(ServiceId, Arc<AtomicBool>)>>,
    failures: RwLock<BTreeMap<ServiceId, String>>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub id: ServiceId,
    pub status: String,
    pub version: Option<String>,
    pub pid: Option<u32>,
    pub uptime: u64,
    pub message: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub root: PathBuf,
    pub system_locale: String,
    pub settings: Settings,
    pub services: Vec<ServiceStatus>,
    pub hosts_synced: bool,
    pub busy_service: Option<ServiceId>,
    pub certificate: CertificateStatus,
    pub environment_move: crate::workspace::MoveStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CertificateStatus {
    pub available: bool,
    pub trusted: bool,
    pub fingerprint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credentials {
    pub username: String,
    pub password: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MailpitConfig {
    max_messages: u32,
    verbose: bool,
}

impl Engine {
    pub async fn ensure_stopped(&self) -> Result<()> {
        if !self.running.lock().await.is_empty() {
            return Err(invalid("Stop all services before moving the environment"));
        }
        Ok(())
    }

    pub fn open(root: PathBuf) -> Result<Self> {
        let storage = Storage::open(root)?;
        let mut engine = Self::from_storage(storage)?;
        user_path::recover(&engine.storage, engine.settings.get_mut())?;
        user_path::upgrade_php_cli(&engine.storage, engine.settings.get_mut())?;
        Ok(engine)
    }
    pub(crate) fn from_storage(storage: Storage) -> Result<Self> {
        let settings = storage.load()?;
        Ok(Self {
            storage,
            settings: RwLock::new(settings),
            operation: Mutex::new(()),
            running: Mutex::new(BTreeMap::new()),
            cancel: StdMutex::new(None),
            failures: RwLock::new(BTreeMap::new()),
        })
    }
    pub async fn snapshot(&self) -> Result<Snapshot> {
        let settings = self.settings.read().await.clone();
        let mut running = self.running.lock().await;
        let mut failures = self.failures.write().await;
        let mut dead = Vec::new();
        for (id, process) in running.iter_mut() {
            if !process.process.alive()? {
                dead.push(*id);
                failures.insert(
                    *id,
                    "The process exited. Inspect its log before restarting.".into(),
                );
            }
        }
        for id in dead {
            running.remove(&id);
        }
        let services = ServiceId::ALL
            .into_iter()
            .map(|id| {
                let config = &settings.services[&id];
                let process = running.get(&id);
                ServiceStatus {
                    id,
                    status: if process.is_some() {
                        "running"
                    } else if failures.contains_key(&id) {
                        "error"
                    } else if config.active_version.is_some() {
                        if id.is_daemon() {
                            "stopped"
                        } else if id.definition().dependencies.iter().any(|dependency| {
                            settings.services[dependency].active_version.is_none()
                        }) {
                            "dependencyMissing"
                        } else {
                            "ready"
                        }
                    } else {
                        "notInstalled"
                    }
                    .into(),
                    version: config.active_version.clone(),
                    pid: process.map(|p| p.process.child.id()),
                    uptime: process.map(|p| p.started.elapsed().as_secs()).unwrap_or(0),
                    message: failures.get(&id).cloned(),
                }
            })
            .collect();
        let hosts: Vec<_> = settings
            .sites
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.hostname.clone())
            .collect();
        let hosts_synced = crate::hosts::read()
            .map(|current| crate::hosts::render(&current, &hosts).is_ok_and(|new| new == current))
            .unwrap_or(false);
        let busy_service = self
            .cancel
            .lock()
            .map_err(|_| invalid("Operation lock unavailable"))?
            .as_ref()
            .map(|(id, _)| *id);
        Ok(Snapshot {
            root: self.storage.root.clone(),
            system_locale: sys_locale::get_locale().unwrap_or_else(|| "en".into()),
            settings,
            services,
            hosts_synced,
            busy_service,
            certificate: self.certificate_status(),
            environment_move: crate::workspace::MoveStatus::default(),
        })
    }
    async fn commit(&self, settings: Settings) -> Result<()> {
        self.commit_files(settings, Vec::new()).await
    }
    async fn commit_files(
        &self,
        settings: Settings,
        changes: Vec<(PathBuf, String)>,
    ) -> Result<()> {
        self.commit_workspace(settings, changes, None).await
    }
    async fn commit_workspace(
        &self,
        mut settings: Settings,
        changes: Vec<(PathBuf, String)>,
        refresh_path: Option<ServiceId>,
    ) -> Result<()> {
        let previous = self.settings.read().await.clone();
        settings.revision = previous
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("Settings revision overflow"))?;
        user_path::transaction(
            &self.storage,
            &previous,
            &mut settings,
            changes,
            refresh_path,
        )?;
        *self.settings.write().await = settings;
        Ok(())
    }
    fn active<'a>(&self, settings: &'a Settings, id: ServiceId) -> Result<&'a str> {
        settings.services[&id]
            .active_version
            .as_deref()
            .ok_or_else(|| invalid("Install and select a version first"))
    }
    pub async fn install(
        &self,
        id: ServiceId,
        version: String,
        progress: impl Fn(installer::Progress) + Send + Sync,
    ) -> Result<()> {
        self.install_with_path(id, version, false, progress).await
    }
    pub async fn install_with_path(
        &self,
        id: ServiceId,
        version: String,
        add_to_path: bool,
        progress: impl Fn(installer::Progress) + Send + Sync,
    ) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let package = catalog::find(id, &version)?;
        let mut settings = self.settings.read().await.clone();
        if settings.services[&id]
            .installations
            .iter()
            .any(|p| p.version == version)
        {
            return Err(invalid("This version is already installed"));
        }
        let destination = self.storage.package(id, &version);
        if destination.exists() {
            // A crash between package promotion and state commit can leave unregistered binaries.
            // Explicit reinstall replaces only that scoped package; configuration and data are separate.
            let allowed = self.storage.root.join("packages").canonicalize()?;
            let resolved = destination.canonicalize()?;
            if resolved == allowed || !resolved.starts_with(&allowed) {
                return Err(invalid("Package directory points outside managed storage"));
            }
            fs::remove_dir_all(&destination)?;
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        *self
            .cancel
            .lock()
            .map_err(|_| invalid("Operation lock unavailable"))? = Some((id, cancelled.clone()));
        let result = installer::install(
            &package,
            &self.storage.root.join(".staging"),
            &destination,
            cancelled,
            progress,
        )
        .await;
        *self
            .cancel
            .lock()
            .map_err(|_| invalid("Operation lock unavailable"))? = None;
        result?;
        let service = settings
            .services
            .get_mut(&id)
            .ok_or_else(|| invalid("Unknown service"))?;
        service.installations.push(Installation {
            version: version.clone(),
            installed_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            sha256: package.sha256,
        });
        if service.active_version.is_none() {
            service.active_version = Some(version.clone());
        }
        if add_to_path {
            service.add_to_path = true;
        }
        self.storage.prepare(id, &version)?;
        toolchain::prepare(&self.storage, id, &version)?;
        let path = self.storage.config(id, &version);
        let changes = if id.definition().config_name.is_some() && !path.exists() {
            vec![(
                path,
                config::generate(&self.storage, id, &version, &settings)?,
            )]
        } else {
            Vec::new()
        };
        if let Err(error) = self.commit_files(settings, changes).await {
            let _ = fs::remove_dir_all(destination);
            return Err(error);
        }
        Ok(())
    }
    pub async fn path_status(&self, id: ServiceId) -> Result<user_path::PathStatus> {
        user_path::status(&self.storage, &*self.settings.read().await, id)
    }
    pub async fn set_user_path(&self, id: ServiceId, enabled: bool) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let mut settings = self.settings.read().await.clone();
        if enabled && settings.services[&id].active_version.is_none() {
            return Err(invalid(
                "Install and select a version before registering PATH",
            ));
        }
        settings.services.get_mut(&id).unwrap().add_to_path = enabled;
        self.commit_workspace(settings, Vec::new(), Some(id)).await
    }
    pub fn cancel_install(&self, id: ServiceId) -> Result<()> {
        if let Some((active, cancel)) = self
            .cancel
            .lock()
            .map_err(|_| invalid("Operation lock unavailable"))?
            .as_ref()
            && *active == id
        {
            cancel.store(true, Ordering::Relaxed);
            return Ok(());
        }
        Err(invalid("No matching download is in progress"))
    }
    pub async fn service_action(&self, id: ServiceId, action: &str) -> Result<()> {
        if !id.is_daemon() {
            return Err(invalid(
                "This is a terminal tool; it has no managed background process",
            ));
        }
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let result = match action {
            "start" => self.start(id).await,
            "stop" => self.stop(id).await,
            "restart" => {
                self.stop(id).await?;
                self.start(id).await
            }
            _ => Err(invalid("Unknown service action")),
        };
        match &result {
            Ok(_) => {
                self.failures.write().await.remove(&id);
            }
            Err(error) => {
                self.failures.write().await.insert(id, error.to_string());
            }
        }
        result
    }
    pub async fn stack_action(&self, action: &str) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        if action == "start" {
            let settings = self.settings.read().await.clone();
            for id in ServiceId::ALL {
                if id.is_daemon() && settings.services[&id].active_version.is_some() {
                    self.start(id).await?;
                }
            }
        } else if action == "stop" {
            let mut error = None;
            for id in ServiceId::ALL.into_iter().rev() {
                if let Err(e) = self.stop(id).await {
                    error = Some(e);
                }
            }
            if let Some(e) = error {
                return Err(e);
            }
        } else {
            return Err(invalid("Unknown stack action"));
        }
        Ok(())
    }
    pub async fn autostart(&self) {
        let _operation = self.operation.lock().await;
        let settings = self.settings.read().await.clone();
        for id in ServiceId::ALL {
            if id.is_daemon()
                && settings.services[&id].auto_start
                && settings.services[&id].active_version.is_some()
                && let Err(e) = self.start(id).await
            {
                self.failures.write().await.insert(id, e.to_string());
            }
        }
    }
    pub async fn shutdown(&self) {
        if let Ok(cancel) = self.cancel.lock()
            && let Some((_, flag)) = cancel.as_ref()
        {
            flag.store(true, Ordering::Relaxed);
        }
        let _operation = self.operation.lock().await;
        for id in ServiceId::ALL.into_iter().rev() {
            let _ = self.stop(id).await;
        }
    }
    async fn start(&self, id: ServiceId) -> Result<()> {
        if !id.is_daemon() {
            return Err(invalid("Terminal tools cannot be started as services"));
        }
        {
            let mut running = self.running.lock().await;
            if let Some(process) = running.get_mut(&id)
                && process.process.alive()?
            {
                return Ok(());
            }
            running.remove(&id);
        }
        let settings = self.settings.read().await.clone();
        let version = self.active(&settings, id)?.to_owned();
        config::ensure(&self.storage, id, &version, &settings)?;
        let mut reserved = Vec::new();
        for port in &settings.services[&id].ports {
            reserved.push(TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, *port)).await.map_err(|_| invalid(format!("Port {port} is already in use. Choose a different port in service settings.")))?);
        }
        self.initialize(id, &version).await?;
        drop(reserved);
        self.validate_config(id, &version, &self.storage.config(id, &version))
            .await?;
        let mut cmd = self.start_command(id, &version, &settings)?;
        let auth_marker = self
            .storage
            .config_dir(id, &version)
            .join("mysql-auth-pending");
        if id == ServiceId::Mysql && auth_marker.exists() {
            let credentials = self.credentials(id, &version)?;
            let path = self
                .storage
                .config_dir(id, &version)
                .join("initialize-auth.sql");
            atomic_write(
                &path,
                format!(
                    "ALTER USER 'root'@'localhost' IDENTIFIED BY '{}';\n",
                    credentials.password
                )
                .as_bytes(),
            )?;
            cmd.arg(format!("--init-file={}", platform::runtime_path(&path)?));
        }
        let mut process =
            Process::spawn(cmd, &self.storage.logs(id, &version).join("process.log"))?;
        let deadline = Instant::now() + Duration::from_secs(45);
        let port = settings.services[&id].ports[0];
        let mut ready = false;
        while Instant::now() < deadline {
            if !process.alive()? {
                break;
            }
            if tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port))
                .await
                .is_ok()
            {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(160)).await;
        }
        if !ready {
            let exit = process
                .child
                .try_wait()?
                .map(|status| status.to_string())
                .unwrap_or_else(|| "readiness timeout".into());
            let _ = process.terminate();
            return Err(invalid(format!(
                "{} failed to become ready on port {} ({exit}). {}",
                id.key(),
                port,
                self.logs(id)
                    .await
                    .unwrap_or_default()
                    .chars()
                    .rev()
                    .take(1800)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            )));
        }
        if id == ServiceId::Mysql {
            let _ = fs::remove_file(auth_marker);
            let _ = fs::remove_file(
                self.storage
                    .config_dir(id, &version)
                    .join("initialize-auth.sql"),
            );
        }
        if id == ServiceId::Caddy {
            self.wait_gateway_tls(&settings).await?;
        }
        self.running.lock().await.insert(
            id,
            Running {
                process,
                version,
                started: Instant::now(),
            },
        );
        Ok(())
    }
    async fn stop(&self, id: ServiceId) -> Result<()> {
        let Some(mut running) = self.running.lock().await.remove(&id) else {
            return Ok(());
        };
        if !running.process.alive()? {
            return Ok(());
        }
        let settings = self.settings.read().await.clone();
        let version = &running.version;
        let package = self.storage.package(id, version);
        let data = self.storage.data(id, version);
        let mut secret_file = None;
        let graceful = match id {
            ServiceId::Nginx => {
                let mut cmd = platform::command(package.join("nginx.exe"));
                cmd.current_dir(self.storage.config_dir(id, version)).args([
                    "-s",
                    "quit",
                    "-p",
                    "./",
                    "-c",
                    "nginx.conf",
                    "-e",
                    "stderr",
                ]);
                Some(cmd)
            }
            ServiceId::Apache => {
                let mut cmd = platform::command(package.join("bin/httpd.exe"));
                cmd.args(["-k", "shutdown", "-d"])
                    .arg(&package)
                    .arg("-f")
                    .arg(self.storage.config(id, version));
                Some(cmd)
            }
            ServiceId::Postgresql => {
                let mut cmd =
                    platform::command(platform::runtime_path(&package.join("bin/pg_ctl.exe"))?);
                cmd.arg("-D")
                    .arg(platform::runtime_path(&data)?)
                    .args(["-m", "fast", "-w", "stop"]);
                Some(cmd)
            }
            ServiceId::Mysql => {
                let credentials = self.credentials(id, version)?;
                let path = self
                    .storage
                    .config_dir(id, version)
                    .join("shutdown-client.ini");
                atomic_write(
                    &path,
                    format!(
                        "[client]\nuser={}\npassword={}\nhost=127.0.0.1\nport={}\n",
                        credentials.username, credentials.password, settings.services[&id].ports[0]
                    )
                    .as_bytes(),
                )?;
                let mut cmd = platform::command(package.join("bin/mysqladmin.exe"));
                cmd.arg(format!(
                    "--defaults-extra-file={}",
                    platform::runtime_path(&path)?
                ))
                .arg("shutdown");
                secret_file = Some(path);
                Some(cmd)
            }
            ServiceId::Redis => {
                let mut cmd = platform::command(package.join("redis-cli.exe"));
                cmd.args([
                    "-h",
                    "127.0.0.1",
                    "-p",
                    &settings.services[&id].ports[0].to_string(),
                    "shutdown",
                    "save",
                ]);
                Some(cmd)
            }
            ServiceId::Caddy => {
                let mut cmd = platform::command(package.join("caddy.exe"));
                cmd.args([
                    "stop",
                    "--address",
                    &format!("127.0.0.1:{}", settings.services[&id].ports[2]),
                ]);
                Some(cmd)
            }
            _ => None,
        };
        if let Some(cmd) = graceful {
            let _ = platform::checked_output(cmd, Duration::from_secs(20)).await;
        }
        let clean = if matches!(id, ServiceId::Php | ServiceId::Mailpit) {
            false
        } else {
            platform::wait_exit(&mut running.process, Duration::from_secs(10)).await?
        };
        if !clean {
            running.process.terminate()?;
        }
        if let Some(path) = secret_file {
            let _ = fs::remove_file(path);
        }
        Ok(())
    }
    fn start_command(
        &self,
        id: ServiceId,
        version: &str,
        settings: &Settings,
    ) -> Result<std::process::Command> {
        let package = self.storage.package(id, version);
        let config = self.storage.config(id, version);
        let data = self.storage.data(id, version);
        let executable = package.join(id.executable());
        let executable = if matches!(id, ServiceId::Mysql | ServiceId::Postgresql) {
            platform::runtime_path(&executable)?
        } else {
            slash(&executable)
        };
        let mut cmd = platform::command(executable);
        cmd.current_dir(&package);
        let ports = &settings.services[&id].ports;
        match id {
            ServiceId::Php => {
                cmd.arg("-c")
                    .arg(config)
                    .args(["-b", &format!("127.0.0.1:{}", ports[0])]);
                cmd.env("PHP_FCGI_MAX_REQUESTS", "0");
            }
            ServiceId::Apache => {
                cmd.arg("-d").arg(&package).arg("-f").arg(config);
            }
            ServiceId::Mysql => {
                cmd.arg(format!(
                    "--defaults-file={}",
                    platform::runtime_path(&config)?
                ))
                .arg("--console");
            }
            ServiceId::Postgresql => {
                cmd.arg("-D")
                    .arg(platform::runtime_path(&data)?)
                    .arg("-c")
                    .arg(format!("config_file={}", platform::runtime_path(&config)?));
            }
            ServiceId::Nginx => {
                // Nginx uses wide APIs for its working directory but narrow argv on Windows.
                cmd.current_dir(self.storage.config_dir(id, version)).args([
                    "-p",
                    "./",
                    "-c",
                    "nginx.conf",
                    "-e",
                    "stderr",
                ]);
            }
            ServiceId::Redis => {
                cmd.arg(crate::storage::cygwin_path(&config));
            }
            ServiceId::Mailpit => {
                let options: MailpitConfig =
                    serde_json::from_str(&config::read(&self.storage, id, version)?)?;
                cmd.args([
                    "--smtp",
                    &format!("127.0.0.1:{}", ports[0]),
                    "--listen",
                    &format!("127.0.0.1:{}", ports[1]),
                    "--max",
                    &options.max_messages.to_string(),
                ])
                .arg("--database")
                .arg(data.join("mailpit.db"));
                if options.verbose {
                    cmd.arg("--verbose");
                }
            }
            ServiceId::Caddy => {
                cmd.args(["run", "--config"])
                    .arg(config)
                    .args(["--adapter", "caddyfile"]);
                self.caddy_env(&mut cmd);
            }
            ServiceId::Nodejs | ServiceId::Composer => {
                return Err(invalid("Terminal tools cannot be started as services"));
            }
        }
        Ok(cmd)
    }
    fn caddy_env(&self, cmd: &mut std::process::Command) {
        cmd.env("XDG_DATA_HOME", self.storage.root.join("data/tls"))
            .env("XDG_CONFIG_HOME", self.storage.root.join("config/tls"));
    }
    async fn initialize(&self, id: ServiceId, version: &str) -> Result<()> {
        let package = self.storage.package(id, version);
        let data = self.storage.data(id, version);
        if id == ServiceId::Nginx {
            for sub in ["client", "proxy", "fastcgi", "scgi", "uwsgi"] {
                fs::create_dir_all(data.join(sub))?;
            }
        }
        if id == ServiceId::Mysql && !data.join("mysql").exists() {
            let _ = self.credentials(id, version)?;
            atomic_write(
                &self
                    .storage
                    .config_dir(id, version)
                    .join("mysql-auth-pending"),
                b"pending",
            )?;
            let mut cmd = platform::command(package.join("bin/mysqld.exe"));
            cmd.arg(format!(
                "--defaults-file={}",
                platform::runtime_path(&self.storage.config(id, version))?
            ))
            .arg("--initialize-insecure");
            platform::checked_output(cmd, Duration::from_secs(180)).await?;
        }
        if id == ServiceId::Postgresql && !data.join("PG_VERSION").exists() {
            let credentials = self.credentials(id, version)?;
            let pwfile = self
                .storage
                .config_dir(id, version)
                .join("initialize-password");
            atomic_write(&pwfile, credentials.password.as_bytes())?;
            let mut cmd =
                platform::command(platform::runtime_path(&package.join("bin/initdb.exe"))?);
            cmd.arg("-D")
                .arg(platform::runtime_path(&data)?)
                .arg("-L")
                .arg(platform::runtime_path(&package.join("share"))?)
                .args([
                    "-U",
                    "postgres",
                    "--encoding=UTF8",
                    "--locale=C",
                    "--auth-host=scram-sha-256",
                    "--auth-local=scram-sha-256",
                    "--pwfile",
                ])
                .arg(platform::runtime_path(&pwfile)?);
            let result = platform::checked_output(cmd, Duration::from_secs(120)).await;
            let _ = fs::remove_file(pwfile);
            result?;
        }
        Ok(())
    }
    pub fn credentials(&self, id: ServiceId, version: &str) -> Result<Credentials> {
        validate_version(version)?;
        if !matches!(id, ServiceId::Mysql | ServiceId::Postgresql) {
            return Err(invalid("This service has no managed database credentials"));
        }
        let path = self
            .storage
            .root
            .join("secrets")
            .join(format!("{}-{version}.dpapi", id.key()));
        if path.exists() {
            return Ok(serde_json::from_slice(&platform::unprotect(&fs::read(
                path,
            )?)?)?);
        }
        let credentials = Credentials {
            username: if id == ServiceId::Mysql {
                "root"
            } else {
                "postgres"
            }
            .into(),
            password: format!("Ll{}", uuid::Uuid::new_v4().simple()),
        };
        atomic_write(
            &path,
            &platform::protect(&serde_json::to_vec(&credentials)?)?,
        )?;
        Ok(credentials)
    }
    pub async fn save_service(
        &self,
        id: ServiceId,
        version: Option<String>,
        ports: Vec<u16>,
        auto_start: bool,
    ) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        if self.running.lock().await.contains_key(&id) {
            return Err(invalid("Stop the service before changing its settings"));
        }
        let current = self.settings.read().await.clone();
        let mut settings = current.clone();
        validate_ports(id, &ports, &settings)?;
        if auto_start && !id.is_daemon() {
            return Err(invalid("Terminal tools do not support auto-start"));
        }
        if let Some(version) = &version
            && !settings.services[&id]
                .installations
                .iter()
                .any(|i| &i.version == version)
        {
            return Err(invalid("Select an installed version"));
        }
        let service = settings
            .services
            .get_mut(&id)
            .ok_or_else(|| invalid("Missing service"))?;
        service.active_version = version.clone();
        service.ports = ports;
        service.auto_start = auto_start;
        sites::validate_upstreams(&settings)?;
        let mut changes = Vec::new();
        if let Some(version) = version.filter(|_| id.definition().config_name.is_some()) {
            // Each PHP version retains its php.ini and extension choices.
            // Other services regenerate on port/version changes, keeping a backup.
            let path = self.storage.config(id, &version);
            if (id != ServiceId::Php
                && (current.services[&id].ports != settings.services[&id].ports
                    || current.services[&id].active_version
                        != settings.services[&id].active_version))
                || !path.exists()
            {
                if path.exists() {
                    fs::copy(&path, path.with_extension("previous"))?;
                }
                changes.push((
                    path,
                    config::generate(&self.storage, id, &version, &settings)?,
                ));
            }
        }
        if current.services[&id].ports != settings.services[&id].ports
            && matches!(id, ServiceId::Php | ServiceId::Mailpit)
        {
            let dependents: &[ServiceId] = if id == ServiceId::Php {
                &[ServiceId::Apache, ServiceId::Nginx, ServiceId::Caddy]
            } else {
                &[ServiceId::Php]
            };
            for dependent in dependents {
                if self.running.lock().await.contains_key(dependent) {
                    return Err(invalid(format!(
                        "Stop {} before changing the {} port",
                        dependent.key(),
                        id.key()
                    )));
                }
                for install in &settings.services[dependent].installations {
                    let path = self.storage.config(*dependent, &install.version);
                    let before = config::read(&self.storage, *dependent, &install.version)?;
                    let after = if *dependent == ServiceId::Caddy {
                        config::generate(&self.storage, *dependent, &install.version, &settings)?
                    } else if id == ServiceId::Php {
                        before.replace(
                            &format!("127.0.0.1:{}", current.services[&id].ports[0]),
                            &format!("127.0.0.1:{}", settings.services[&id].ports[0]),
                        )
                    } else {
                        before.replace(
                            &format!("smtp_port={}", current.services[&id].ports[0]),
                            &format!("smtp_port={}", settings.services[&id].ports[0]),
                        )
                    };
                    changes.push((path, after));
                }
            }
        }
        if id == ServiceId::Nginx && current.services[&id].ports != settings.services[&id].ports {
            if self.running.lock().await.contains_key(&ServiceId::Caddy) {
                return Err(invalid("Stop Caddy before changing the Nginx port"));
            }
            for install in &settings.services[&ServiceId::Caddy].installations {
                changes.push((
                    self.storage.config(ServiceId::Caddy, &install.version),
                    config::generate(&self.storage, ServiceId::Caddy, &install.version, &settings)?,
                ));
            }
        }
        if matches!(id, ServiceId::Nginx | ServiceId::Php | ServiceId::Caddy) {
            // Per-site roots and user rules survive service port/version changes.
            for (path, content) in sites::files(&self.storage, &settings)? {
                if fs::read_to_string(&path).ok().as_deref() != Some(&content) {
                    if self.running.lock().await.contains_key(&ServiceId::Nginx) {
                        return Err(invalid(
                            "Stop Nginx before changing ports used by local sites",
                        ));
                    }
                    changes.push((path, content));
                }
            }
        }
        self.commit_files(settings, changes).await
    }
    pub async fn remove_package(&self, id: ServiceId, version: String) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        validate_version(&version)?;
        if self
            .running
            .lock()
            .await
            .get(&id)
            .is_some_and(|r| r.version == version)
        {
            return Err(invalid("Stop the service before removing its package"));
        }
        let mut settings = self.settings.read().await.clone();
        let service = settings
            .services
            .get_mut(&id)
            .ok_or_else(|| invalid("Missing service"))?;
        if !service.installations.iter().any(|i| i.version == version) {
            return Err(invalid("Package is not installed"));
        }
        let directory = self.storage.package(id, &version);
        let staged = self
            .storage
            .root
            .join(".staging")
            .join(uuid::Uuid::new_v4().to_string());
        fs::rename(&directory, &staged)?;
        service.installations.retain(|i| i.version != version);
        if service.active_version.as_ref() == Some(&version) {
            service.active_version = service.installations.first().map(|i| i.version.clone());
        }
        if let Err(error) = self.commit(settings).await {
            let _ = fs::rename(staged, directory);
            return Err(error);
        }
        fs::remove_dir_all(staged)?;
        Ok(())
    }
    pub async fn logs(&self, id: ServiceId) -> Result<String> {
        let settings = self.settings.read().await;
        let version = self.active(&settings, id)?;
        let directory = self.storage.logs(id, version);
        let mut files = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_file() && path.extension().is_some_and(|e| e == "log") {
                files.push(path);
            }
        }
        files.sort();
        let mut output = String::new();
        for path in files {
            let mut file = fs::File::open(&path)?;
            let length = file.metadata()?.len();
            file.seek(SeekFrom::Start(length.saturating_sub(48 * 1024)))?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            // A bounded tail may start inside a multibyte character. Discard the
            // incomplete first line before deciding which encoding the log uses.
            if length > 48 * 1024
                && let Some(newline) = bytes.iter().position(|byte| *byte == b'\n')
            {
                bytes.drain(..=newline);
            }
            output.push_str(&format!(
                "\n── {} ──\n{}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                platform::decode_service_text(&bytes)
            ));
        }
        Ok(output)
    }
    pub async fn read_config(&self, id: ServiceId) -> Result<String> {
        if id.definition().config_name.is_none() {
            return Err(invalid("This tool has no service configuration file"));
        }
        let settings = self.settings.read().await;
        config::read(&self.storage, id, self.active(&settings, id)?)
    }
    pub async fn save_config(&self, id: ServiceId, content: String) -> Result<()> {
        if id.definition().config_name.is_none() {
            return Err(invalid("This tool has no service configuration file"));
        }
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        if id == ServiceId::Caddy {
            return Err(invalid(
                "Gateway configuration is generated from your domains",
            ));
        }
        if content.len() > 1024 * 1024 {
            return Err(invalid("Configuration exceeds 1 MiB"));
        }
        if self.running.lock().await.contains_key(&id) {
            return Err(invalid("Stop the service before editing configuration"));
        }
        let settings = self.settings.read().await;
        let content = if id == ServiceId::Nginx {
            sites::attach(&content, &self.storage)?
        } else {
            content
        };
        let version = self.active(&settings, id)?;
        let path = self.storage.config(id, version);
        let candidate = path.with_extension("candidate");
        atomic_write(&candidate, content.as_bytes())?;
        let result = self.validate_config(id, version, &candidate).await;
        let _ = fs::remove_file(candidate);
        result?;
        if path.exists() {
            fs::copy(&path, path.with_extension("previous"))?;
        }
        let current = settings.clone();
        drop(settings);
        self.commit_files(current, vec![(path, content)]).await
    }
    async fn validate_config(&self, id: ServiceId, version: &str, path: &Path) -> Result<()> {
        let package = self.storage.package(id, version);
        let executable = package.join(id.executable());
        let executable = if matches!(id, ServiceId::Mysql | ServiceId::Postgresql) {
            platform::runtime_path(&executable)?
        } else {
            slash(&executable)
        };
        let mut cmd = platform::command(executable);
        cmd.current_dir(&package);
        match id {
            ServiceId::Apache => {
                cmd.arg("-t").arg("-d").arg(&package).arg("-f").arg(path);
            }
            ServiceId::Nginx => {
                cmd.current_dir(self.storage.config_dir(id, version))
                    .args(["-t", "-p", "./", "-c"])
                    .arg(
                        path.file_name()
                            .ok_or_else(|| invalid("Invalid configuration name"))?,
                    )
                    .args(["-e", "stderr"]);
            }
            ServiceId::Mysql => {
                cmd.arg(format!("--defaults-file={}", platform::runtime_path(path)?))
                    .arg("--validate-config");
            }
            ServiceId::Postgresql => {
                if !self.storage.data(id, version).join("PG_VERSION").exists() {
                    return Ok(());
                }
                cmd.arg("-D")
                    .arg(platform::runtime_path(&self.storage.data(id, version))?)
                    .args(["-C", "port", "-c"])
                    .arg(format!("config_file={}", platform::runtime_path(path)?));
            }
            ServiceId::Php => {
                cmd = platform::command(package.join("php.exe"));
                cmd.arg("-c").arg(path).arg("-m");
            }
            ServiceId::Mailpit => {
                let config: MailpitConfig = serde_json::from_str(&fs::read_to_string(path)?)?;
                if config.max_messages == 0 {
                    return Err(invalid("maxMessages must be greater than zero"));
                }
                return Ok(());
            }
            ServiceId::Caddy => {
                cmd.arg("validate")
                    .arg("--config")
                    .arg(path)
                    .args(["--adapter", "caddyfile"]);
                self.caddy_env(&mut cmd);
            }
            ServiceId::Redis => return Ok(()), // Redis provides no offline validator; startup reports parser errors.
            ServiceId::Nodejs | ServiceId::Composer => {
                return Err(invalid("This tool has no service configuration file"));
            }
        }
        let output = platform::checked_output(cmd, Duration::from_secs(30)).await?;
        if id == ServiceId::Php && output.to_lowercase().contains("syntax error") {
            return Err(invalid(output));
        }
        Ok(())
    }
    pub async fn php_extensions(&self) -> Result<php_extensions::Inventory> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let settings = self.settings.read().await.clone();
        let version = self.active(&settings, ServiceId::Php)?;
        config::ensure(&self.storage, ServiceId::Php, version, &settings)?;
        let base = php_extensions::probe(&self.storage, version, None).await?;
        php_extensions::assert_clean(&base)?;
        let current = php_extensions::probe(
            &self.storage,
            version,
            Some(&self.storage.config(ServiceId::Php, version)),
        )
        .await?;
        php_extensions::inventory(&self.storage, version, &base, current)
    }

    pub async fn set_php_extension(
        &self,
        version: String,
        extension: String,
        enabled: bool,
        progress: impl Fn(installer::Progress) + Send + Sync,
    ) -> Result<php_extensions::Inventory> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let settings = self.settings.read().await.clone();
        if self.active(&settings, ServiceId::Php)? != version {
            return Err(invalid(
                "The active PHP version changed. Refresh the extensions list.",
            ));
        }
        let catalog = php_extensions::catalog()?;
        let definition = catalog
            .extensions
            .iter()
            .find(|entry| entry.id == extension)
            .ok_or_else(|| invalid("Unknown PHP extension"))?;
        config::ensure(&self.storage, ServiceId::Php, &version, &settings)?;
        let base = php_extensions::probe(&self.storage, &version, None).await?;
        php_extensions::assert_clean(&base)?;
        if base.runtime.version != version.split('-').next().unwrap_or_default() {
            return Err(invalid(
                "The installed PHP executable does not match its selected version",
            ));
        }
        if php_extensions::has_module(&base, definition) {
            return Err(invalid(
                "This extension is compiled into PHP and cannot be changed",
            ));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        *self
            .cancel
            .lock()
            .map_err(|_| invalid("Operation lock unavailable"))? =
            Some((ServiceId::Php, cancelled.clone()));
        let result = self
            .change_php_extension(
                &settings, &version, &catalog, definition, &base, enabled, cancelled, &progress,
            )
            .await;
        *self
            .cancel
            .lock()
            .map_err(|_| invalid("Operation lock unavailable"))? = None;
        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn change_php_extension(
        &self,
        settings: &Settings,
        version: &str,
        catalog: &php_extensions::Catalog,
        definition: &php_extensions::Definition,
        base: &php_extensions::Probe,
        enabled: bool,
        cancelled: Arc<AtomicBool>,
        progress: &(impl Fn(installer::Progress) + Send + Sync),
    ) -> Result<php_extensions::Inventory> {
        let path = self.storage.config(ServiceId::Php, version);
        let before = fs::read_to_string(&path)?;
        let current = php_extensions::probe(&self.storage, version, Some(&path)).await?;
        if !enabled {
            for dependent in &catalog.extensions {
                if dependent.dependencies.contains(&definition.id)
                    && php_extensions::has_module(&current, dependent)
                {
                    return Err(invalid(format!(
                        "Disable {} before disabling {}",
                        dependent.id, definition.id
                    )));
                }
            }
        }
        let mut content = before.clone();
        if enabled {
            // Dependencies here are bundled with the selected PHP archive; no implicit downloads.
            for dependency in &definition.dependencies {
                let dependency = catalog
                    .extensions
                    .iter()
                    .find(|entry| &entry.id == dependency)
                    .ok_or_else(|| invalid("Unknown extension dependency"))?;
                if !php_extensions::has_module(&current, dependency) {
                    if dependency.source != "bundled" {
                        return Err(invalid("Enable the required extension first"));
                    }
                    let dll = self
                        .storage
                        .package(ServiceId::Php, version)
                        .join("ext")
                        .join(php_extensions::dll(dependency));
                    if !dll.is_file() {
                        return Err(invalid("A required bundled PHP extension is missing"));
                    }
                    content = php_extensions::configure(&content, dependency, Some(&dll))?;
                }
            }
        }
        let dll = if enabled {
            if definition.source == "bundled" {
                let dll = self
                    .storage
                    .package(ServiceId::Php, version)
                    .join("ext")
                    .join(php_extensions::dll(definition));
                if !dll.is_file() {
                    return Err(invalid(
                        "This extension is not included with the selected PHP build",
                    ));
                }
                Some(dll)
            } else {
                let artifact = catalog.artifacts.iter().find(|artifact| artifact.id == definition.id && artifact.matches(&base.runtime)).ok_or_else(|| invalid("No verified extension build matches this PHP version, architecture, compiler and thread safety"))?;
                let directory = artifact.directory(&self.storage, version);
                if !directory.exists() {
                    installer::install_files(
                        &artifact.package(),
                        &self.storage.root.join(".staging"),
                        &directory,
                        cancelled.clone(),
                        progress,
                        &[php_extensions::dll(definition)],
                    )
                    .await?;
                }
                php_extensions::verify_cached(artifact, &directory, definition)?;
                Some(directory.join(php_extensions::dll(definition)))
            }
        } else {
            None
        };
        content = php_extensions::configure(&content, definition, dll.as_deref())?;
        let report = |phase: &str| {
            progress(installer::Progress {
                service: "php".into(),
                version: version.into(),
                phase: phase.into(),
                downloaded: 0,
                total: None,
            })
        };
        report("validating");
        let candidate = path.with_extension("extension-candidate.ini");
        atomic_write(&candidate, content.as_bytes())?;
        let validated = php_extensions::probe(&self.storage, version, Some(&candidate)).await;
        let _ = fs::remove_file(&candidate);
        let validated = validated?;
        php_extensions::assert_clean(&validated)?;
        if php_extensions::has_module(&validated, definition) != enabled {
            return Err(invalid("PHP did not apply the requested extension state"));
        }
        for module in current.modules.keys() {
            if !module.eq_ignore_ascii_case(&definition.module)
                && !validated.modules.contains_key(module)
            {
                return Err(invalid(format!(
                    "The extension change would unload {module}"
                )));
            }
        }
        if cancelled.load(Ordering::Relaxed) {
            return Err(invalid("Installation cancelled"));
        }
        if fs::read_to_string(&path)? != before {
            return Err(invalid(
                "php.ini changed during this operation. Refresh the extensions list and retry.",
            ));
        }
        // Validate before touching the live configuration or process. Gateways keep their listeners.
        let was_running = self
            .running
            .lock()
            .await
            .get_mut(&ServiceId::Php)
            .map(|running| running.process.alive())
            .transpose()?
            .unwrap_or(false);
        atomic_write(&path.with_extension("previous"), before.as_bytes())?;
        if was_running {
            self.stop(ServiceId::Php).await?;
            report("reloading");
        }
        if let Err(error) = self
            .commit_files(settings.clone(), vec![(path.clone(), content)])
            .await
        {
            report("restoring");
            if was_running && let Err(restart) = self.start(ServiceId::Php).await {
                self.failures
                    .write()
                    .await
                    .insert(ServiceId::Php, restart.to_string());
                return Err(invalid(format!("{error}; restoring PHP failed: {restart}")));
            }
            return Err(error);
        }
        if was_running && let Err(error) = self.start(ServiceId::Php).await {
            report("restoring");
            self.commit_files(settings.clone(), vec![(path, before)])
                .await
                .map_err(|rollback| {
                    invalid(format!(
                        "PHP restart failed: {error}; configuration rollback failed: {rollback}"
                    ))
                })?;
            if let Err(restart) = self.start(ServiceId::Php).await {
                self.failures
                    .write()
                    .await
                    .insert(ServiceId::Php, restart.to_string());
                return Err(invalid(format!(
                    "Configuration restored, but PHP could not restart: {restart}; original failure: {error}"
                )));
            }
            return Err(invalid(format!(
                "PHP restart failed; the previous configuration and service were restored: {error}"
            )));
        }
        self.failures.write().await.remove(&ServiceId::Php);
        php_extensions::inventory(&self.storage, version, base, validated)
    }

    pub async fn save_preferences(&self, language: String, theme: String) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        if !["auto", "en", "pt-BR", "es", "zh-CN", "hi"].contains(&language.as_str())
            || !["system", "light", "dark"].contains(&theme.as_str())
        {
            return Err(invalid("Unsupported preference"));
        }
        let mut settings = self.settings.read().await.clone();
        settings.language = language;
        settings.theme = theme;
        self.commit(settings).await
    }
    pub async fn save_site(&self, mut site: Site) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        site.hostname = validate_hostname(&site.hostname)?;
        if site.mode != SiteMode::Proxy {
            site.root = fs::canonicalize(&site.root)
                .map_err(|_| invalid("Choose an existing project directory"))?;
            if !site.root.is_dir() || slash(&site.root).contains(['\n', '\r']) {
                return Err(invalid("Invalid project directory"));
            }
        }
        let mut settings = self.settings.read().await.clone();
        // Rules have their own bounded, validated IPC. A form or toggle cannot
        // overwrite a newer editor save with a stale snapshot.
        site.nginx_directives = settings
            .sites
            .iter()
            .find(|s| s.id == site.id)
            .and_then(|s| s.nginx_directives.clone());
        if let Some(previous) = settings.sites.iter().find(|s| s.id == site.id)
            && previous.mode != site.mode
            && site.nginx_directives.is_some()
        {
            return Err(invalid(
                "Reset custom Nginx rules before changing the project type",
            ));
        }
        if settings
            .sites
            .iter()
            .any(|s| s.hostname == site.hostname && s.id != site.id)
        {
            return Err(invalid("This hostname is already registered"));
        }
        if site.mode == SiteMode::Proxy
            && (site.upstream_port.is_none()
                || site.upstream_port == Some(0)
                || settings.services[&ServiceId::Caddy]
                    .ports
                    .contains(&site.upstream_port.unwrap_or(0))
                || (site.server == SiteServer::Nginx
                    && site.upstream_port == Some(settings.services[&ServiceId::Nginx].ports[0])))
        {
            return Err(invalid(
                "Choose a valid upstream port, different from the gateway ports",
            ));
        }
        if site.id.is_empty() {
            site.id = uuid::Uuid::new_v4().to_string();
        } else if !settings.sites.iter().any(|s| s.id == site.id) {
            return Err(invalid("Domain does not exist"));
        }
        settings.sites.retain(|s| s.id != site.id);
        settings.sites.push(site);
        sites::validate_upstreams(&settings)?;
        self.commit_sites(settings).await
    }
    pub async fn read_site_config(&self, id: String) -> Result<sites::SiteConfig> {
        let settings = self.settings.read().await;
        let site = settings
            .sites
            .iter()
            .find(|s| s.id == id && s.server == SiteServer::Nginx)
            .ok_or_else(|| invalid("Nginx domain does not exist"))?;
        sites::render(&self.storage, site, &settings)
    }
    pub async fn preview_site_config(
        &self,
        id: String,
        content: Option<String>,
    ) -> Result<sites::SiteConfig> {
        let settings = self.settings.read().await;
        let mut site = settings
            .sites
            .iter()
            .find(|s| s.id == id && s.server == SiteServer::Nginx)
            .cloned()
            .ok_or_else(|| invalid("Nginx domain does not exist"))?;
        site.nginx_directives = content;
        sites::render(&self.storage, &site, &settings)
    }
    pub async fn save_site_config(&self, id: String, content: Option<String>) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let mut settings = self.settings.read().await.clone();
        let site = settings
            .sites
            .iter_mut()
            .find(|s| s.id == id && s.server == SiteServer::Nginx)
            .ok_or_else(|| invalid("Nginx domain does not exist"))?;
        site.nginx_directives = content;
        let site = site.clone();
        if settings.services[&ServiceId::Nginx]
            .installations
            .is_empty()
        {
            return Err(invalid(
                "Install Nginx to validate and save custom site rules",
            ));
        }
        sites::render(&self.storage, &site, &settings)?;
        self.commit_sites(settings).await
    }
    pub async fn remove_site(&self, id: String) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let mut settings = self.settings.read().await.clone();
        settings.sites.retain(|site| site.id != id);
        self.commit_sites(settings).await
    }
    async fn commit_sites(&self, settings: Settings) -> Result<()> {
        let current = self.settings.read().await.clone();
        let mut changes = self.nginx_site_changes(&settings).await?;
        let mut reload = None;
        if let Some(version) = current.services[&ServiceId::Caddy]
            .active_version
            .as_deref()
        {
            let path = self.storage.config(ServiceId::Caddy, version);
            let content = config::generate(&self.storage, ServiceId::Caddy, version, &settings)?;
            let candidate = path.with_extension("candidate");
            atomic_write(&candidate, content.as_bytes())?;
            let result = self
                .validate_config(ServiceId::Caddy, version, &candidate)
                .await;
            let _ = fs::remove_file(candidate);
            result?;
            changes.push((path.clone(), content));
            reload = Some((version.to_owned(), path));
        }
        let rollback: Vec<_> = changes
            .iter()
            .map(|(path, _)| (path.clone(), fs::read_to_string(path).unwrap_or_default()))
            .collect();
        for (path, _) in &changes {
            if path.exists() {
                fs::copy(path, path.with_extension("previous"))?;
            }
        }
        self.commit_files(settings.clone(), changes).await?;
        if let Some((version, path)) = reload
            && self.running.lock().await.contains_key(&ServiceId::Caddy)
            && let Err(error) = self.reload_gateway(&settings, &version, &path).await
        {
            self.commit_files(current.clone(), rollback).await?;
            if let Err(rollback) = self.reload_gateway(&current, &version, &path).await {
                return Err(invalid(format!(
                    "Gateway reload failed: {error}. Rollback failed: {rollback}"
                )));
            }
            return Err(error);
        }
        Ok(())
    }
    async fn nginx_site_changes(&self, settings: &Settings) -> Result<Vec<(PathBuf, String)>> {
        let mut changes = sites::files(&self.storage, settings)?;
        for install in &settings.services[&ServiceId::Nginx].installations {
            let path = self.storage.config(ServiceId::Nginx, &install.version);
            let main = config::read(&self.storage, ServiceId::Nginx, &install.version)?;
            changes.push((path, sites::attach(&main, &self.storage)?));
        }
        changes.retain(|(path, content)| fs::read_to_string(path).ok().as_ref() != Some(content));
        if changes.is_empty() {
            return Ok(changes);
        }
        if self.running.lock().await.contains_key(&ServiceId::Nginx) {
            return Err(invalid(
                "Stop Nginx before changing local site configuration",
            ));
        }
        for install in &settings.services[&ServiceId::Nginx].installations {
            self.initialize(ServiceId::Nginx, &install.version).await?;
            let main = config::read(&self.storage, ServiceId::Nginx, &install.version)?;
            let candidate = self
                .storage
                .config_dir(ServiceId::Nginx, &install.version)
                .join("sites.candidate");
            atomic_write(
                &candidate,
                sites::inline(&main, &self.storage, settings)?.as_bytes(),
            )?;
            let result = self
                .validate_config(ServiceId::Nginx, &install.version, &candidate)
                .await;
            let _ = fs::remove_file(candidate);
            result?;
        }
        Ok(changes)
    }
    async fn reload_gateway(&self, settings: &Settings, version: &str, path: &Path) -> Result<()> {
        let mut cmd = platform::command(
            self.storage
                .package(ServiceId::Caddy, version)
                .join("caddy.exe"),
        );
        cmd.args(["reload", "--config"]).arg(path).args([
            "--adapter",
            "caddyfile",
            "--address",
            &format!(
                "127.0.0.1:{}",
                settings.services[&ServiceId::Caddy].ports[2]
            ),
        ]);
        self.caddy_env(&mut cmd);
        platform::checked_output(cmd, Duration::from_secs(20)).await?;
        self.wait_gateway_tls(settings).await
    }
    async fn wait_gateway_tls(&self, settings: &Settings) -> Result<()> {
        use rustls::{
            ClientConfig, RootCertStore,
            pki_types::{CertificateDer, ServerName},
        };
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(self.certificate_der()?))
            .map_err(|e| invalid(e.to_string()))?;
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let config = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| invalid(e.to_string()))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let mut names: Vec<String> = settings
            .sites
            .iter()
            .filter(|site| site.enabled)
            .map(|site| site.hostname.clone())
            .collect();
        if names.is_empty() {
            names.push("localhost".into());
        }
        let port = settings.services[&ServiceId::Caddy].ports[1];
        // Complete only a TLS handshake. Never make a request that could execute project code.
        tokio::time::timeout(Duration::from_secs(15), async {
            for name in names {
                let name = ServerName::try_from(name).map_err(|e| invalid(e.to_string()))?;
                loop {
                    if let Ok(socket) = tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await
                        && tokio::time::timeout(Duration::from_secs(2), connector.connect(name.clone(), socket)).await.is_ok_and(|result| result.is_ok()) { break; }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
            Ok(())
        }).await.map_err(|_| invalid("The gateway has not finished provisioning its local certificates. Inspect the Caddy log."))?
    }
    pub async fn apply_hosts(&self) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let settings = self.settings.read().await;
        crate::hosts::apply(
            &settings
                .sites
                .iter()
                .filter(|s| s.enabled)
                .map(|s| s.hostname.clone())
                .collect::<Vec<_>>(),
        )
        .await
    }
    pub fn certificate_path(&self) -> PathBuf {
        self.storage
            .root
            .join("data/tls/caddy/pki/authorities/local/root.crt")
    }
    fn certificate_der(&self) -> Result<Vec<u8>> {
        use base64::Engine as _;
        let pem = fs::read_to_string(self.certificate_path())?;
        let encoded: String = pem
            .lines()
            .filter(|line| !line.starts_with("-----"))
            .collect();
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid("Invalid local CA certificate"))
    }
    fn certificate_fingerprint(&self) -> Result<String> {
        use sha1::{Digest, Sha1};
        Ok(hex::encode_upper(Sha1::digest(self.certificate_der()?)))
    }
    fn certificate_status(&self) -> CertificateStatus {
        let fingerprint = self.certificate_fingerprint().ok();
        #[cfg(windows)]
        let trusted = fingerprint.as_ref().is_some_and(|fingerprint| {
            winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
                .open_subkey_with_flags(
                    format!(
                        "Software\\Microsoft\\SystemCertificates\\Root\\Certificates\\{fingerprint}"
                    ),
                    winreg::enums::KEY_READ,
                )
                .is_ok()
        });
        #[cfg(not(windows))]
        let trusted = false;
        CertificateStatus {
            available: fingerprint.is_some(),
            trusted,
            fingerprint,
        }
    }
    pub async fn certificate_action(&self, action: &str) -> Result<String> {
        let _operation = self
            .operation
            .try_lock()
            .map_err(|_| invalid("Another workspace operation is in progress"))?;
        let cert = self.certificate_path();
        if !cert.is_file() {
            return Err(invalid(
                "Install and start the HTTPS gateway first to generate the local certificate authority",
            ));
        }
        let mut cmd = platform::command("certutil.exe");
        // Certutil otherwise writes its localized Windows code page into redirected stdout.
        cmd.arg("-Unicode");
        match action {
            "trust" => {
                cmd.args(["-user", "-addstore", "Root"]).arg(cert);
            }
            "inspect" => {
                cmd.arg("-dump").arg(cert);
            }
            "untrust" => {
                let fingerprint = self.certificate_fingerprint()?;
                cmd.args(["-user", "-delstore", "Root", &fingerprint]);
            }
            _ => return Err(invalid("Unknown certificate action")),
        }
        platform::checked_output_utf16le(cmd, Duration::from_secs(120)).await
    }
    pub async fn open_location(&self, location: &str, id: Option<ServiceId>) -> Result<()> {
        let settings = self.settings.read().await;
        let path = match location {
            "workspace" => self.storage.root.clone(),
            "projects" => self.storage.root.join("projects"),
            "logs" | "config" | "package" => {
                let id = id.ok_or_else(|| invalid("Missing service"))?;
                let version = self.active(&settings, id)?;
                match location {
                    "logs" => self.storage.logs(id, version),
                    "config" => self.storage.config_dir(id, version),
                    _ => self.storage.package(id, version),
                }
            }
            _ => return Err(invalid("Unknown location")),
        };
        platform::open_path(&path)
    }
    pub async fn open_service(&self, id: ServiceId) -> Result<()> {
        let settings = self.settings.read().await;
        let ports = &settings.services[&id].ports;
        let port = match id {
            ServiceId::Apache | ServiceId::Nginx => ports[0],
            ServiceId::Mailpit => ports[1],
            _ => return Err(invalid("This service has no web interface")),
        };
        platform::open_path(Path::new(&format!("http://127.0.0.1:{port}")))
    }
    pub async fn open_site(&self, id: String) -> Result<()> {
        let settings = self.settings.read().await;
        let site = settings
            .sites
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| invalid("Domain does not exist"))?;
        let port = settings.services[&ServiceId::Caddy].ports[1];
        let url = if port == 443 {
            format!("https://{}", site.hostname)
        } else {
            format!("https://{}:{port}", site.hostname)
        };
        platform::open_path(Path::new(&url))
    }
}
