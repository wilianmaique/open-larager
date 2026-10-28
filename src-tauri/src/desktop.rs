use crate::{
    error::{Result, invalid},
    model::CloseBehavior,
    workspace::Workspace,
};
use serde::Deserialize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tauri::{
    AppHandle, Emitter, Manager, State,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tokio::sync::Mutex;

const TRAY_ID: &str = "main-tray";
const CLOSE_EVENT: &str = "app-close-requested";

#[derive(Default)]
pub struct Desktop {
    pending: AtomicBool,
    exiting: AtomicBool,
    operation: Mutex<()>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrayLabels {
    open: String,
    exit: String,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloseAction {
    Tray,
    Exit,
}

impl From<CloseAction> for CloseBehavior {
    fn from(action: CloseAction) -> Self {
        match action {
            CloseAction::Tray => Self::Tray,
            CloseAction::Exit => Self::Exit,
        }
    }
}

fn restore(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.show()?;
        window.unminimize()?;
        window.set_focus()?;
    }
    Ok(())
}

fn hide(app: &AppHandle) -> Result<()> {
    // Never hide the only window without a working way to restore it.
    if app.tray_by_id(TRAY_ID).is_none() {
        return Err(invalid("System tray is unavailable"));
    }
    app.get_webview_window("main")
        .ok_or_else(|| invalid("Main window is unavailable"))?
        .hide()
        .map_err(|error| invalid(error.to_string()))
}

fn prompt(app: &AppHandle, desktop: &Desktop) {
    desktop.pending.store(true, Ordering::SeqCst);
    let _ = restore(app);
    let _ = app.emit_to("main", CLOSE_EVENT, ());
}

async fn exit(app: &AppHandle, desktop: &Desktop) {
    if desktop.exiting.swap(true, Ordering::SeqCst) {
        return;
    }
    desktop.pending.store(false, Ordering::SeqCst);
    let workspace = app.state::<Arc<Workspace>>();
    // Wait for relocation and IPC work before stopping services and exiting.
    workspace.engine.write().await.shutdown().await;
    app.exit(0);
}

pub fn request_close(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let desktop = app.state::<Desktop>();
        let _operation = desktop.operation.lock().await;
        if desktop.exiting.load(Ordering::SeqCst) {
            return;
        }
        let behavior = {
            let workspace = app.state::<Arc<Workspace>>();
            let engine = workspace.engine.read().await;
            engine.settings.read().await.close_behavior
        };
        match behavior {
            CloseBehavior::Ask => prompt(&app, &desktop),
            CloseBehavior::Tray => {
                if hide(&app).is_err() {
                    prompt(&app, &desktop);
                }
            }
            CloseBehavior::Exit => exit(&app, &desktop).await,
        }
    });
}

#[tauri::command]
pub async fn configure_tray(app: AppHandle, labels: TrayLabels) -> Result<()> {
    let desktop = app.state::<Desktop>();
    let _operation = desktop.operation.lock().await;
    let create = || -> tauri::Result<()> {
        let open = MenuItem::with_id(&app, "tray-open", labels.open, true, None::<&str>)?;
        let exit_item = MenuItem::with_id(&app, "tray-exit", labels.exit, true, None::<&str>)?;
        let menu = Menu::with_items(&app, &[&open, &exit_item])?;
        if let Some(tray) = app.tray_by_id(TRAY_ID) {
            tray.set_menu(Some(menu))?;
        } else {
            let icon = app
                .default_window_icon()
                .ok_or_else(|| tauri::Error::AssetNotFound("Application icon".into()))?;
            TrayIconBuilder::with_id(TRAY_ID)
                .icon(icon.clone())
                .tooltip("Open Larager")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        let _ = restore(tray.app_handle());
                    }
                })
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "tray-open" => {
                        let _ = restore(app);
                    }
                    "tray-exit" => {
                        let app = app.clone();
                        tauri::async_runtime::spawn(async move {
                            let desktop = app.state::<Desktop>();
                            let _operation = desktop.operation.lock().await;
                            exit(&app, &desktop).await;
                        });
                    }
                    _ => {}
                })
                .build(&app)?;
        }
        Ok(())
    };
    create().map_err(|error| invalid(error.to_string()))
}

#[tauri::command]
pub fn get_close_request(desktop: State<'_, Desktop>) -> bool {
    desktop.pending.load(Ordering::SeqCst)
}

#[tauri::command]
pub async fn resolve_close_request(
    app: AppHandle,
    action: Option<CloseAction>,
    remember: bool,
) -> Result<()> {
    let desktop = app.state::<Desktop>();
    let _operation = desktop.operation.lock().await;
    if !desktop.pending.load(Ordering::SeqCst) || desktop.exiting.load(Ordering::SeqCst) {
        return Ok(());
    }
    if let Some(action) = action {
        if matches!(action, CloseAction::Tray) && app.tray_by_id(TRAY_ID).is_none() {
            return Err(invalid("System tray is unavailable"));
        }
        if remember {
            let workspace = app.state::<Arc<Workspace>>();
            let engine = workspace.engine.read().await;
            engine.save_close_behavior(action.into()).await?;
        }
        match action {
            CloseAction::Tray => hide(&app)?,
            CloseAction::Exit => exit(&app, &desktop).await,
        }
    }
    desktop.pending.store(false, Ordering::SeqCst);
    Ok(())
}

#[tauri::command]
pub async fn save_close_behavior(
    workspace: State<'_, Arc<Workspace>>,
    behavior: CloseBehavior,
) -> Result<()> {
    workspace
        .engine
        .read()
        .await
        .save_close_behavior(behavior)
        .await
}
