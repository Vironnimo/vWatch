//! The Watchtower window: a Tauri shell around `ui/` that reaches the service through commands.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

use crate::service::{Watchtower, WatchtowerError};
use crate::system;

struct AppState {
    service: Arc<Watchtower>,
    /// Counts the window's own write commands, which the database's data
    /// version does not see.
    writes: Arc<AtomicU64>,
}

/// Identifies what the window has seen: other connections' commits (agents'
/// CLI calls) and the window's own write commands.
fn version(data: i64, writes: u64) -> Value {
    json!(format!("{data}.{writes}"))
}

/// The window state with the version it reflects. Both counters are read
/// before the state, so a write the state misses always changes the version.
fn versioned_state(
    s: &Watchtower,
    writes: &AtomicU64,
    visit: bool,
) -> Result<Value, WatchtowerError> {
    let own = writes.load(Ordering::SeqCst);
    let (mut state, data) = s.versioned_state(visit)?;
    state["data_version"] = version(data, own);
    Ok(state)
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

/// Run a write command. Afterwards the window's version changes even when the
/// command failed, so the page reloads unless it already loaded a newer state.
async fn write<F>(state: &State<'_, AppState>, f: F) -> Reply
where
    F: FnOnce(&Watchtower) -> Result<Value, WatchtowerError> + Send + 'static,
{
    let result = work(state, f).await;
    state.writes.fetch_add(1, Ordering::SeqCst);
    result
}

#[tauri::command(rename_all = "snake_case")]
async fn state(app: State<'_, AppState>, visit: bool) -> Reply {
    let writes = app.writes.clone();
    work(&app, move |s| versioned_state(s, &writes, visit)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn statistics(
    app: State<'_, AppState>,
    project: Option<String>,
    from: Option<String>,
    until: Option<String>,
) -> Reply {
    work(&app, move |s| {
        s.statistics(project.as_deref(), from.as_deref(), until.as_deref())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn history(
    app: State<'_, AppState>,
    project: Option<String>,
    entity: Option<String>,
    id: Option<String>,
    before: Option<i64>,
) -> Reply {
    work(&app, move |s| {
        s.history(project.as_deref(), entity.as_deref(), id.as_deref(), before)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn history_event(app: State<'_, AppState>, id: i64) -> Reply {
    work(&app, move |s| s.history_event(id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn document_versions(app: State<'_, AppState>, project: Option<String>) -> Reply {
    work(&app, move |s| match project {
        Some(id) => s.card_history(&id),
        None => s.portfolio_history(),
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn compare_documents(
    app: State<'_, AppState>,
    project: Option<String>,
    from: i64,
    to: i64,
) -> Reply {
    work(&app, move |s| {
        s.compare_documents(project.as_deref(), from, to)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn refresh(app: State<'_, AppState>) -> Reply {
    let writes = app.writes.clone();
    work(&app, move |s| {
        let measured = s.refresh_facts(None, true);
        writes.fetch_add(1, Ordering::SeqCst);
        measured?;
        versioned_state(s, &writes, false)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_feedback(
    app: State<'_, AppState>,
    item_id: String,
    value: Option<String>,
    note: Option<String>,
    until: Option<String>,
) -> Reply {
    write(&app, move |s| {
        s.set_feedback_until(
            &item_id,
            value.as_deref(),
            note.as_deref(),
            until.as_deref(),
        )
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn create_task(app: State<'_, AppState>, payload: Value) -> Reply {
    write(&app, move |s| s.create_task(payload)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn restore_feedback(
    app: State<'_, AppState>,
    item_id: String,
    snapshot: Option<Value>,
) -> Reply {
    write(&app, move |s| {
        s.restore_feedback(&item_id, snapshot.as_ref())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn undo(app: State<'_, AppState>, token: String) -> Reply {
    write(&app, move |s| s.undo(&token)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_task_status(
    app: State<'_, AppState>,
    id: i64,
    status: String,
    revision: i64,
) -> Reply {
    write(&app, move |s| {
        s.set_task_status_guarded(id, &status, revision)
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_warning_ack(
    app: State<'_, AppState>,
    project: String,
    key: String,
    note: Option<String>,
) -> Reply {
    write(&app, move |s| {
        s.set_warning_ack(&project, &key, note.as_deref())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn mark_changes_read(app: State<'_, AppState>, token: String) -> Reply {
    write(&app, move |s| s.mark_changes_read(&token)).await
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
    write(&app, move |s| s.set_status(&id, &status, note.as_deref())).await
}

#[tauri::command(rename_all = "snake_case")]
async fn add_idea(app: State<'_, AppState>, text: String) -> Reply {
    write(&app, move |s| s.add_idea(&text)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_idea_status(
    app: State<'_, AppState>,
    id: String,
    status: String,
    revision: String,
) -> Reply {
    write(&app, move |s| s.set_idea_status(&id, &status, &revision)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn update_idea(app: State<'_, AppState>, id: i64, text: String, revision: i64) -> Reply {
    write(&app, move |s| s.update_idea(id, &text, revision)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn remove_idea(app: State<'_, AppState>, id: i64) -> Reply {
    write(&app, move |s| s.remove_idea(id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn restore_idea(app: State<'_, AppState>, id: i64) -> Reply {
    write(&app, move |s| s.restore_idea(id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn set_goal(
    app: State<'_, AppState>,
    id: String,
    goal: String,
    due: Option<String>,
) -> Reply {
    write(&app, move |s| s.set_goal(&id, &goal, due.as_deref())).await
}

#[tauri::command(rename_all = "snake_case")]
async fn rename_project(app: State<'_, AppState>, id: String, name: String) -> Reply {
    write(&app, move |s| s.rename_project(&id, &name)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn update_project(
    app: State<'_, AppState>,
    id: String,
    name: Option<String>,
    path: Option<String>,
) -> Reply {
    write(&app, move |s| {
        s.update_project(&id, name.as_deref(), path.as_deref())
    })
    .await
}

#[tauri::command(rename_all = "snake_case")]
async fn remove_project(app: State<'_, AppState>, id: String) -> Reply {
    write(&app, move |s| s.remove_project(&id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn restore_project(app: State<'_, AppState>, id: String) -> Reply {
    write(&app, move |s| s.restore_project(&id)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn discover(app: State<'_, AppState>, root: String) -> Reply {
    work(&app, move |s| s.discover(&root)).await
}

#[tauri::command(rename_all = "snake_case")]
async fn add_projects(app: State<'_, AppState>, paths: Vec<String>) -> Reply {
    write(&app, move |s| s.add_projects(&paths)).await
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

/// Open a web link in the default browser, or a folder or file in Explorer. Card paths may use
/// forward slashes and `%VARIABLES%`.
#[tauri::command(rename_all = "snake_case")]
fn open_external(target: String) -> Reply {
    let is_url = target.starts_with("https://") || target.starts_with("http://");
    let target = if is_url { target } else { expand_path(&target) };
    let path = std::path::Path::new(&target);
    let mut cmd = std::process::Command::new(if cfg!(windows) {
        "explorer"
    } else {
        "xdg-open"
    });
    if is_url || path.is_dir() {
        cmd.arg(&target);
    } else if path.is_file() && cfg!(windows) {
        cmd.arg(format!("/select,{target}"));
    } else {
        return Err(json!({"code": "not_openable", "message": format!("Cannot open {target}.")}));
    }
    cmd.spawn().map_err(internal)?;
    Ok(json!({}))
}

/// Replaces `%NAME%` with the environment variable and, on Windows, `/` with `\`.
fn expand_path(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('%') {
        let Some(len) = rest[start + 1..].find('%') else {
            break;
        };
        let name = &rest[start + 1..start + 1 + len];
        out.push_str(&rest[..start]);
        match std::env::var(name) {
            Ok(value) if !name.is_empty() => out.push_str(&value),
            _ => out.push_str(&rest[start..start + len + 2]),
        }
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    if cfg!(windows) {
        out.replace('/', "\\")
    } else {
        out
    }
}

/// Changes whenever an agent's CLI call committed a write or the window ran a
/// write command; compare it with the `data_version` of the state shown.
/// Visits, opening a project and display preferences leave it unchanged.
#[tauri::command(rename_all = "snake_case")]
async fn data_version(app: State<'_, AppState>) -> Reply {
    let writes = app.writes.clone();
    work(&app, move |s| {
        let own = writes.load(Ordering::SeqCst);
        Ok(version(s.data_version()?, own))
    })
    .await
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
    let _ = system::export_skill();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app)
        }))
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            service: Arc::new(service),
            writes: Arc::new(AtomicU64::new(0)),
        })
        .invoke_handler(tauri::generate_handler![
            state,
            statistics,
            history,
            history_event,
            document_versions,
            compare_documents,
            refresh,
            set_feedback,
            create_task,
            restore_feedback,
            undo,
            set_task_status,
            set_warning_ack,
            mark_changes_read,
            open_project,
            set_pref,
            set_status,
            add_idea,
            set_idea_status,
            update_idea,
            remove_idea,
            restore_idea,
            set_goal,
            rename_project,
            update_project,
            remove_project,
            restore_project,
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
