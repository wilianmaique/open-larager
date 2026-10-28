pub mod catalog;
pub mod config;
pub mod engine;
pub mod error;
pub mod hosts;
pub mod installer;
pub mod model;
pub mod php_extensions;
pub mod platform;
pub mod sites;
pub mod storage;
pub mod toolchain;
pub mod user_path;

use engine::Engine;
use error::Result;
use model::{ServiceId, Site};
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager, State};

#[tauri::command]
async fn get_snapshot(engine: State<'_, Arc<Engine>>) -> Result<engine::Snapshot> {
    engine.snapshot().await
}
#[tauri::command]
fn get_catalog() -> Result<Vec<model::Package>> {
    catalog::packages()
}
#[tauri::command]
async fn install_package(
    engine: State<'_, Arc<Engine>>,
    app: tauri::AppHandle,
    id: ServiceId,
    version: String,
    add_to_path: Option<bool>,
) -> Result<()> {
    engine
        .install_with_path(id, version, add_to_path.unwrap_or(false), move |progress| {
            let _ = app.emit("install-progress", progress);
        })
        .await
}
#[tauri::command]
async fn get_path_status(
    engine: State<'_, Arc<Engine>>,
    id: ServiceId,
) -> Result<user_path::PathStatus> {
    engine.path_status(id).await
}
#[tauri::command]
async fn set_user_path(engine: State<'_, Arc<Engine>>, id: ServiceId, enabled: bool) -> Result<()> {
    engine.set_user_path(id, enabled).await
}
#[tauri::command]
fn cancel_install(engine: State<'_, Arc<Engine>>, id: ServiceId) -> Result<()> {
    engine.cancel_install(id)
}
#[tauri::command]
async fn service_action(
    engine: State<'_, Arc<Engine>>,
    id: ServiceId,
    action: String,
) -> Result<()> {
    engine.service_action(id, &action).await
}
#[tauri::command]
async fn stack_action(engine: State<'_, Arc<Engine>>, action: String) -> Result<()> {
    engine.stack_action(&action).await
}
#[tauri::command]
async fn save_service(
    engine: State<'_, Arc<Engine>>,
    id: ServiceId,
    version: Option<String>,
    ports: Vec<u16>,
    auto_start: bool,
) -> Result<()> {
    engine.save_service(id, version, ports, auto_start).await
}
#[tauri::command]
async fn remove_package(
    engine: State<'_, Arc<Engine>>,
    id: ServiceId,
    version: String,
) -> Result<()> {
    engine.remove_package(id, version).await
}
#[tauri::command]
async fn read_config(engine: State<'_, Arc<Engine>>, id: ServiceId) -> Result<String> {
    engine.read_config(id).await
}
#[tauri::command]
async fn save_config(engine: State<'_, Arc<Engine>>, id: ServiceId, content: String) -> Result<()> {
    engine.save_config(id, content).await
}
#[tauri::command]
async fn get_php_extensions(engine: State<'_, Arc<Engine>>) -> Result<php_extensions::Inventory> {
    engine.php_extensions().await
}
#[tauri::command]
async fn set_php_extension(
    engine: State<'_, Arc<Engine>>,
    app: tauri::AppHandle,
    version: String,
    extension: String,
    enabled: bool,
) -> Result<php_extensions::Inventory> {
    engine
        .set_php_extension(version, extension, enabled, move |progress| {
            let _ = app.emit("install-progress", progress);
        })
        .await
}
#[tauri::command]
async fn read_logs(engine: State<'_, Arc<Engine>>, id: ServiceId) -> Result<String> {
    engine.logs(id).await
}
#[tauri::command]
async fn reveal_credentials(
    engine: State<'_, Arc<Engine>>,
    id: ServiceId,
) -> Result<engine::Credentials> {
    let settings = engine.settings.read().await;
    let version = settings.services[&id]
        .active_version
        .as_deref()
        .ok_or_else(|| error::invalid("Install a version first"))?;
    engine.credentials(id, version)
}
#[tauri::command]
async fn save_preferences(
    engine: State<'_, Arc<Engine>>,
    language: String,
    theme: String,
) -> Result<()> {
    engine.save_preferences(language, theme).await
}
#[tauri::command]
async fn save_site(engine: State<'_, Arc<Engine>>, site: Site) -> Result<()> {
    engine.save_site(site).await
}
#[tauri::command]
async fn read_site_config(engine: State<'_, Arc<Engine>>, id: String) -> Result<sites::SiteConfig> {
    engine.read_site_config(id).await
}
#[tauri::command]
async fn preview_site_config(
    engine: State<'_, Arc<Engine>>,
    id: String,
    content: Option<String>,
) -> Result<sites::SiteConfig> {
    engine.preview_site_config(id, content).await
}
#[tauri::command]
async fn save_site_config(
    engine: State<'_, Arc<Engine>>,
    id: String,
    content: Option<String>,
) -> Result<()> {
    engine.save_site_config(id, content).await
}
#[tauri::command]
async fn remove_site(engine: State<'_, Arc<Engine>>, id: String) -> Result<()> {
    engine.remove_site(id).await
}
#[tauri::command]
async fn apply_hosts(engine: State<'_, Arc<Engine>>) -> Result<()> {
    engine.apply_hosts().await
}
#[tauri::command]
async fn certificate_action(engine: State<'_, Arc<Engine>>, action: String) -> Result<String> {
    engine.certificate_action(&action).await
}
#[tauri::command]
async fn open_location(
    engine: State<'_, Arc<Engine>>,
    location: String,
    id: Option<ServiceId>,
) -> Result<()> {
    engine.open_location(&location, id).await
}
#[tauri::command]
async fn open_service(engine: State<'_, Arc<Engine>>, id: ServiceId) -> Result<()> {
    engine.open_service(id).await
}
#[tauri::command]
async fn open_site(engine: State<'_, Arc<Engine>>, id: String) -> Result<()> {
    engine.open_site(id).await
}
#[tauri::command]
async fn choose_folder(app: tauri::AppHandle) -> Result<Option<PathBuf>> {
    use tauri_plugin_dialog::DialogExt;
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .map(|path| path.into_path().map_err(|e| error::invalid(e.to_string())))
            .transpose()
    })
    .await
    .map_err(|e| error::invalid(e.to_string()))?
}

pub fn run() {
    if std::env::args().nth(1).as_deref() == Some("--apply-hosts") {
        let hosts: Vec<String> = std::env::args().skip(2).collect();
        match hosts::run_helper(&hosts) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
    }
    #[cfg(windows)]
    match platform::relaunch_unelevated() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let exe = std::env::current_exe()?;
            let parent = exe.parent().ok_or("Executable has no parent directory")?;
            let root = if parent.join("larager.portable").is_file() {
                parent.join("larager-data")
            } else {
                app.path().app_local_data_dir()?
            };
            let engine = Arc::new(Engine::open(root)?);
            app.manage(engine.clone());
            tauri::async_runtime::spawn(async move {
                engine.autostart().await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            get_catalog,
            install_package,
            get_path_status,
            set_user_path,
            cancel_install,
            service_action,
            stack_action,
            save_service,
            remove_package,
            read_config,
            save_config,
            get_php_extensions,
            set_php_extension,
            read_logs,
            reveal_credentials,
            save_preferences,
            save_site,
            read_site_config,
            preview_site_config,
            save_site_config,
            remove_site,
            apply_hosts,
            certificate_action,
            open_location,
            open_service,
            open_site,
            choose_folder
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let engine = window.state::<Arc<Engine>>().inner().clone();
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    engine.shutdown().await;
                    app.exit(0);
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("Unable to start Open Larager");
}
