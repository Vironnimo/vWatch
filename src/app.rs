//! The Watchtower window: a Tauri shell around `ui/` that reaches the service through commands.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

use crate::service::{Watchtower, WatchtowerError};
use crate::store::ChangeWatch;
use crate::system;

struct AppState {
    service: Arc<Watchtower>,
    watch: ChangeWatch,
}

type Reply = Result<Value, Value>;

fn internal(e: impl std::fmt::Display) -> Value {
    json!({"code": "internal", "message": e.to_string()})
}

/// Run service work off the UI thread.
async fn work<F>(state: &State<'_, AppState>, f: F) -> Reply
where
    F: FnOnce(&Watchtower) -> Result<Value, WatchtowerError> + Send + 'static,
{
    let svc = state.service.clone();
    tauri::async_runtime::spawn_blocking(move || f(&svc))
        .await
        .map_err(internal)?
        .map_err(|e| e.to_json())
}

#[tauri::command(rename_all = "snake_case")]
async fn state(app: State<'_, AppState>, visit: bool) -> Reply {
    work(&app, move |s| s.state(visit)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn refresh(app: State<'_, AppState>) -> Reply {
    work(&app, |s| {
        s.refresh_facts(None, true)?;
        s.state(false)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_feedback(
    app: State<'_, AppState>,
    item_id: String,
    value: Option<String>,
    note: Option<String>,
) -> Reply {
    work(&app, move |s| {
        s.set_feedback(&item_id, value.as_deref(), note.as_deref())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn open_project(app: State<'_, AppState>, id: String) -> Reply {
    work(&app, move |s| s.open_project(&id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_pref(app: State<'_, AppState>, key: String, value: Value) -> Reply {
    work(&app, move |s| s.set_pref(&key, &value).map(|_| json!({}))).await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_status(
    app: State<'_, AppState>,
    id: String,
    status: String,
    note: Option<String>,
) -> Reply {
    work(&app, move |s| s.set_status(&id, &status, note.as_deref())).await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_goal(
    app: State<'_, AppState>,
    id: String,
    goal: String,
    due: Option<String>,
) -> Reply {
    work(&app, move |s| s.set_goal(&id, &goal, due.as_deref())).await
}

#[tauri::command(rename_all = "snake_case")]
async fn rename_project(app: State<'_, AppState>, id: String, name: String) -> Reply {
    work(&app, move |s| s.rename_project(&id, &name)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn remove_project(app: State<'_, AppState>, id: String) -> Reply {
    work(&app, move |s| s.remove_project(&id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn discover(app: State<'_, AppState>, root: String) -> Reply {
    work(&app, move |s| s.discover(&root)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn add_projects(app: State<'_, AppState>, paths: Vec<String>) -> Reply {
    work(&app, move |s| s.add_projects(&paths)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn pick_folder(app: AppHandle) -> Reply {
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("Ordner oder Projekt wählen")
            .blocking_pick_folder()
    })
    .await
    .map_err(internal)?;
    Ok(json!(picked.map(|p| p.to_string())))
}

#[tauri::command(rename_all = "snake_case")]
fn settings() -> Value {
    system::settings()
}

#[tauri::command(rename_all = "snake_case")]
fn set_autostart(on: bool) -> Reply {
    system::set_autostart(on).map_err(internal)?;
    Ok(system::settings())
}

#[tauri::command(rename_all = "snake_case")]
fn set_agent_access(on: bool) -> Reply {
    system::set_agent_access(on).map_err(internal)?;
    Ok(system::settings())
}

/// Open a web link in the default browser or a folder in Explorer.
#[tauri::command(rename_all = "snake_case")]
fn open_external(target: String) -> Reply {
    let is_url = target.starts_with("https://") || target.starts_with("http://");
    if !is_url && !std::path::Path::new(&target).is_dir() {
        return Err(json!({"code": "not_openable", "message": format!("Cannot open {target}.")}));
    }
    std::process::Command::new(if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    })
    .arg(&target)
    .spawn()
    .map_err(internal)?;
    Ok(json!({}))
}

/// Changes whenever anything (the window's own work or an agent's CLI call) wrote to the database.
#[tauri::command(rename_all = "snake_case")]
fn data_version(app: State<'_, AppState>) -> Reply {
    app.watch.version().map(|v| json!(v)).map_err(internal)
}

/// The page calls this after its first render, so the window never shows a blank frame.
#[tauri::command(rename_all = "snake_case")]
fn ready(app: AppHandle) {
    show_main(&app);
}

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

pub fn run() {
    let data = system::data_dir();
    let db = system::db_path(&data);
    let service = Watchtower::open(&db).expect("Watchtower cannot open its database");
    let watch = ChangeWatch::open(&db).expect("Watchtower cannot open its database");
    let _ = system::export_skill();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            service: Arc::new(service),
            watch,
        })
        .invoke_handler(tauri::generate_handler![
            state,
            refresh,
            set_feedback,
            open_project,
            set_pref,
            set_status,
            set_goal,
            rename_project,
            remove_project,
            discover,
            add_projects,
            pick_folder,
            settings,
            set_autostart,
            set_agent_access,
            open_external,
            data_version,
            ready
        ])
        .setup(move |app| {
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("Watchtower")
                .inner_size(1280.0, 880.0)
                .min_inner_size(720.0, 520.0)
                .center()
                .visible(false)
                .data_directory(data.join("webview"))
                .build()?;
            // Show the window even if the page never reports ready.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs(4));
                if let Some(w) = handle.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(true) {
                        let _ = w.show();
                    }
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Watchtower could not start its window");
}
