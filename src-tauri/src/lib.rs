pub mod catalog;
pub mod config;
pub mod engine;
pub mod error;
pub mod file_diagnostics;
pub mod hosts;
pub mod installer;
pub mod model;
pub mod php_extensions;
pub mod platform;
pub mod sites;
pub mod storage;
pub mod toolchain;
pub mod user_path;
pub mod webview_profile;
pub mod workspace;

use error::Result;
use model::{ServiceId, Site};
use std::{path::PathBuf, sync::Arc};
use tauri::{Emitter, Manager, State};
use workspace::Workspace;

#[tauri::command]
async fn get_snapshot(engine: State<'_, Arc<Workspace>>) -> Result<engine::Snapshot> {
    let current = engine.engine.read().await;
    let mut snapshot = current.snapshot().await?;
    snapshot.environment_move = engine.status()?;
    Ok(snapshot)
}
#[tauri::command]
fn get_catalog() -> Result<Vec<model::Package>> {
    catalog::packages()
}
#[tauri::command]
async fn install_package(
    engine: State<'_, Arc<Workspace>>,
    app: tauri::AppHandle,
    id: ServiceId,
    version: String,
    add_to_path: Option<bool>,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine
        .install_with_path(id, version, add_to_path.unwrap_or(false), move |progress| {
            let _ = app.emit("install-progress", progress);
        })
        .await
}
#[tauri::command]
async fn get_path_status(
    engine: State<'_, Arc<Workspace>>,
    id: ServiceId,
) -> Result<user_path::PathStatus> {
    let engine = engine.engine.read().await;
    engine.path_status(id).await
}
#[tauri::command]
async fn set_user_path(
    engine: State<'_, Arc<Workspace>>,
    id: ServiceId,
    enabled: bool,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.set_user_path(id, enabled).await
}
#[tauri::command]
async fn cancel_install(engine: State<'_, Arc<Workspace>>, id: ServiceId) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.cancel_install(id)
}
#[tauri::command]
async fn service_action(
    engine: State<'_, Arc<Workspace>>,
    id: ServiceId,
    action: String,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.service_action(id, &action).await
}
#[tauri::command]
async fn stack_action(engine: State<'_, Arc<Workspace>>, action: String) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.stack_action(&action).await
}
#[tauri::command]
async fn save_service(
    engine: State<'_, Arc<Workspace>>,
    id: ServiceId,
    version: Option<String>,
    ports: Vec<u16>,
    auto_start: bool,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.save_service(id, version, ports, auto_start).await
}
#[tauri::command]
async fn remove_package(
    engine: State<'_, Arc<Workspace>>,
    id: ServiceId,
    version: String,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.remove_package(id, version).await
}
#[tauri::command]
async fn read_config(engine: State<'_, Arc<Workspace>>, id: ServiceId) -> Result<String> {
    let engine = engine.engine.read().await;
    engine.read_config(id).await
}
#[tauri::command]
async fn save_config(
    engine: State<'_, Arc<Workspace>>,
    id: ServiceId,
    content: String,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.save_config(id, content).await
}
#[tauri::command]
async fn get_php_extensions(
    engine: State<'_, Arc<Workspace>>,
) -> Result<php_extensions::Inventory> {
    let engine = engine.engine.read().await;
    engine.php_extensions().await
}
#[tauri::command]
async fn set_php_extension(
    engine: State<'_, Arc<Workspace>>,
    app: tauri::AppHandle,
    version: String,
    extension: String,
    enabled: bool,
) -> Result<php_extensions::Inventory> {
    let engine = engine.engine.read().await;
    engine
        .set_php_extension(version, extension, enabled, move |progress| {
            let _ = app.emit("install-progress", progress);
        })
        .await
}
#[tauri::command]
async fn read_logs(engine: State<'_, Arc<Workspace>>, id: ServiceId) -> Result<String> {
    let engine = engine.engine.read().await;
    engine.logs(id).await
}
#[tauri::command]
async fn reveal_credentials(
    engine: State<'_, Arc<Workspace>>,
    id: ServiceId,
) -> Result<engine::Credentials> {
    let engine = engine.engine.read().await;
    let settings = engine.settings.read().await;
    let version = settings.services[&id]
        .active_version
        .as_deref()
        .ok_or_else(|| error::invalid("Install a version first"))?;
    engine.credentials(id, version)
}
#[tauri::command]
async fn save_preferences(
    engine: State<'_, Arc<Workspace>>,
    language: String,
    theme: String,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.save_preferences(language, theme).await
}
#[tauri::command]
async fn save_site(engine: State<'_, Arc<Workspace>>, site: Site) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.save_site(site).await
}
#[tauri::command]
async fn read_site_config(
    engine: State<'_, Arc<Workspace>>,
    id: String,
) -> Result<sites::SiteConfig> {
    let engine = engine.engine.read().await;
    engine.read_site_config(id).await
}
#[tauri::command]
async fn preview_site_config(
    engine: State<'_, Arc<Workspace>>,
    id: String,
    content: Option<String>,
) -> Result<sites::SiteConfig> {
    let engine = engine.engine.read().await;
    engine.preview_site_config(id, content).await
}
#[tauri::command]
async fn save_site_config(
    engine: State<'_, Arc<Workspace>>,
    id: String,
    content: Option<String>,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.save_site_config(id, content).await
}
#[tauri::command]
async fn remove_site(engine: State<'_, Arc<Workspace>>, id: String) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.remove_site(id).await
}
#[tauri::command]
async fn apply_hosts(engine: State<'_, Arc<Workspace>>) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.apply_hosts().await
}
#[tauri::command]
async fn certificate_action(engine: State<'_, Arc<Workspace>>, action: String) -> Result<String> {
    let engine = engine.engine.read().await;
    engine.certificate_action(&action).await
}
#[tauri::command]
async fn open_location(
    engine: State<'_, Arc<Workspace>>,
    location: String,
    id: Option<ServiceId>,
) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.open_location(&location, id).await
}
#[tauri::command]
async fn open_service(engine: State<'_, Arc<Workspace>>, id: ServiceId) -> Result<()> {
    let engine = engine.engine.read().await;
    engine.open_service(id).await
}
#[tauri::command]
async fn open_site(engine: State<'_, Arc<Workspace>>, id: String) -> Result<()> {
    let engine = engine.engine.read().await;
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

#[tauri::command]
async fn choose_environment_folder(
    engine: State<'_, Arc<Workspace>>,
    app: tauri::AppHandle,
) -> Result<Option<workspace::FolderSelection>> {
    let path = choose_folder(app).await?;
    engine.select(path).await
}

#[tauri::command]
async fn move_environment(
    engine: State<'_, Arc<Workspace>>,
    token: String,
) -> Result<workspace::MoveStatus> {
    let workspace = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        tauri::async_runtime::block_on(workspace.relocate(token))
    })
    .await
    .map_err(|e| error::invalid(e.to_string()))?
}

#[tauri::command]
async fn cleanup_environment(engine: State<'_, Arc<Workspace>>) -> Result<workspace::MoveStatus> {
    let workspace = engine.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        tauri::async_runtime::block_on(workspace.finish_pending())
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
            let app_data = app.path().app_local_data_dir()?;
            let root = if parent.join("larager.portable").is_file() {
                parent.join("larager-data")
            } else {
                app_data.clone()
            };
            let preference = if parent.join("larager.portable").is_file() {
                parent.join("larager-environment.json")
            } else {
                app.path()
                    .app_config_dir()?
                    .join("environment-location.json")
            };
            let engine = Arc::new(Workspace::open(root, preference)?);
            app.manage(engine.clone());
            let profile = if parent.join("larager.portable").is_file() {
                webview_profile::portable(parent)?
            } else {
                webview_profile::prepare(&app_data)?
            };
            if let Some(error) = profile.legacy_error {
                eprintln!("Legacy WebView profile preserved: {error}");
            }
            let window = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .ok_or("Missing main window configuration")?;
            tauri::WebviewWindowBuilder::from_config(app, window)?
                .data_directory(profile.directory)
                .build()?;
            tauri::async_runtime::spawn(async move {
                let workspace = engine.clone();
                let status = tauri::async_runtime::spawn_blocking(move || {
                    tauri::async_runtime::block_on(workspace.finish_pending())
                })
                .await;
                if matches!(
                    status,
                    Ok(Ok(workspace::MoveStatus {
                        cleanup_source: None,
                        ..
                    }))
                ) {
                    engine.engine.read().await.autostart().await;
                }
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
            choose_folder,
            choose_environment_folder,
            move_environment,
            cleanup_environment
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let engine = window.state::<Arc<Workspace>>().inner().clone();
                let app = window.app_handle().clone();
                tauri::async_runtime::spawn(async move {
                    engine.engine.read().await.shutdown().await;
                    app.exit(0);
                });
            }
        })
        .run(tauri::generate_context!())
        .expect("Unable to start Open Larager");
}
