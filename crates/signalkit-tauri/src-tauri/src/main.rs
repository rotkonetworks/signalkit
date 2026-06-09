#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Arc;

use signalkit_core::{default_signal_dir, Chat, DesktopBundle, MessageRow};
use tauri::{Emitter, Manager};
use tokio::io::AsyncBufReadExt;
use tokio::sync::{Mutex, Notify};

type BundleResult = Result<DesktopBundle, String>;

#[derive(Default)]
struct AppState {
    bundle: Arc<Mutex<Option<BundleResult>>>,
    ready: Arc<Notify>,
}

#[derive(Default)]
struct RecvState {
    child: Mutex<Option<tokio::process::Child>>,
}

impl AppState {
    async fn wait_for_bundle<R, F>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&DesktopBundle) -> Result<R, String>,
    {
        loop {
            {
                let guard = self.bundle.lock().await;
                if let Some(result) = guard.as_ref() {
                    return match result {
                        Ok(b) => f(b),
                        Err(e) => Err(e.clone()),
                    };
                }
            }
            self.ready.notified().await;
        }
    }
}

#[tauri::command]
async fn open_bundle(state: tauri::State<'_, AppState>) -> Result<(), String> {
    state.wait_for_bundle(|_| Ok(())).await
}

#[tauri::command]
async fn list_chats(state: tauri::State<'_, AppState>) -> Result<Vec<Chat>, String> {
    state
        .wait_for_bundle(|b| b.db.list_chats(false).map_err(|e| e.to_string()))
        .await
}

#[tauri::command]
async fn read_chat(
    state: tauri::State<'_, AppState>,
    chat_id: String,
    limit: u32,
    offset: u32,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Result<Vec<MessageRow>, String> {
    state
        .wait_for_bundle(|b| {
            b.db.get_messages(&chat_id, Some(limit), offset, from_ms, to_ms)
                .map_err(|e| e.to_string())
        })
        .await
}

fn find_cli_bin() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("signalkit");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("signalkit")
}

#[tauri::command]
async fn signal_send(to: String, body: String) -> Result<i64, String> {
    let bin = find_cli_bin();
    let output = tokio::process::Command::new(&bin)
        .arg("send")
        .arg(&to)
        .arg(&body)
        .output()
        .await
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        return Err(if stderr.is_empty() {
            format!("signalkit send exited {}", output.status)
        } else {
            stderr
        });
    }
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|e| format!("bad json from send: {e}"))?;
    parsed
        .get("timestamp")
        .and_then(|t| t.as_i64())
        .ok_or_else(|| "no timestamp in send response".to_string())
}

#[tauri::command]
async fn signal_recv_start(
    state: tauri::State<'_, RecvState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let mut guard = state.child.lock().await;
    if guard.is_some() {
        return Ok(());
    }
    let bin = find_cli_bin();
    let mut child = tokio::process::Command::new(&bin)
        .arg("recv")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;
    let stdout = child.stdout.take().ok_or_else(|| "no stdout".to_string())?;
    let stderr = child.stderr.take().ok_or_else(|| "no stderr".to_string())?;

    let app_msg = app.clone();
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            match serde_json::from_str::<serde_json::Value>(&line) {
                Ok(v) => {
                    let _ = app_msg.emit("signal-message", v);
                }
                Err(_) => {
                    let _ = app_msg.emit("signal-recv-log", line);
                }
            }
        }
        let _ = app_msg.emit("signal-recv-stopped", "stdout closed");
    });

    let app_err = app.clone();
    tokio::spawn(async move {
        let mut lines = tokio::io::BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = app_err.emit("signal-recv-log", line);
        }
    });

    *guard = Some(child);
    Ok(())
}

#[tauri::command]
async fn signal_recv_stop(state: tauri::State<'_, RecvState>) -> Result<(), String> {
    let mut guard = state.child.lock().await;
    if let Some(mut c) = guard.take() {
        let _ = c.kill().await;
    }
    Ok(())
}

#[tauri::command]
async fn signal_whoami() -> Result<serde_json::Value, String> {
    let bin = find_cli_bin();
    let output = tokio::process::Command::new(&bin)
        .arg("whoami")
        .output()
        .await
        .map_err(|e| format!("failed to spawn {}: {e}", bin.display()))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|e| format!("bad json: {e}"))
}

#[tauri::command]
async fn search(
    state: tauri::State<'_, AppState>,
    query: String,
    chat_id: Option<String>,
    limit: u32,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Result<Vec<MessageRow>, String> {
    state
        .wait_for_bundle(|b| {
            b.db.search_messages(chat_id.as_deref(), &query, Some(limit), from_ms, to_ms)
                .map_err(|e| e.to_string())
        })
        .await
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .manage(AppState::default())
        .manage(RecvState::default())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Kick off Signal Desktop DB open immediately — before the webview's JS
            // has even parsed. By the time `open_bundle` is invoked from JS, this
            // usually resolves instantly.
            let state = app.state::<AppState>();
            let bundle_slot = state.bundle.clone();
            let ready = state.ready.clone();
            tauri::async_runtime::spawn(async move {
                let result = match default_signal_dir() {
                    Ok(dir) => DesktopBundle::open(dir).await.map_err(|e| e.to_string()),
                    Err(e) => Err(e.to_string()),
                };
                *bundle_slot.lock().await = Some(result);
                ready.notify_waiters();
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_bundle,
            list_chats,
            read_chat,
            search,
            signal_send,
            signal_whoami,
            signal_recv_start,
            signal_recv_stop,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
