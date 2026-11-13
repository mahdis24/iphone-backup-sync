pub mod sync;
pub mod wpd;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::sync::{FileClassification, ScanResult, ScannedFile, SyncController};
use crate::wpd::DeviceInfo;

/// Shared application state managed by Tauri.
pub struct AppState {
    /// Controller holding thread-safe flags for skipping or cancelling transfers.
    pub controller: SyncController,
    /// Atomic flag indicating whether a synchronization batch is currently active.
    pub is_syncing: Arc<AtomicBool>,
    /// Atomic flag indicating whether an MTP device scan is currently in progress.
    pub is_scanning: Arc<AtomicBool>,
}

fn get_storage_dir(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Enumerates connected Windows Portable Devices (WPD) and returns metadata for any connected iPhone.
#[tauri::command]
async fn get_devices() -> Result<Vec<DeviceInfo>, String> {
    tokio::task::spawn_blocking(wpd::get_connected_devices)
        .await
        .map_err(|e| format!("Task join error: {}", e))?
}

/// Scans the selected device recursively and computes diff classification against the backup folder.
#[tauri::command]
async fn scan_device(
    app: AppHandle,
    state: State<'_, AppState>,
    device_id: String,
    backup_dir: String,
) -> Result<ScanResult, String> {
    if state.is_scanning.load(Ordering::Relaxed) {
        return Err("A scan is already in progress".to_string());
    }

    state.is_scanning.store(true, Ordering::Relaxed);
    let is_scanning = state.is_scanning.clone();
    let storage_dir = get_storage_dir(&app);
    let backup_path = PathBuf::from(&backup_dir);

    let res = tokio::task::spawn_blocking(move || {
        let _com = wpd::ComGuard::new();
        let device = wpd::open_device(&device_id)?;
        let files = wpd::scan_files(&device, |_| {})?;
        let scan_result = sync::diff_and_classify_files(&files, &backup_path, &storage_dir);
        Ok(scan_result)
    })
    .await;

    is_scanning.store(false, Ordering::Relaxed);

    match res {
        Ok(inner_res) => inner_res,
        Err(e) => Err(format!("Scan task failed: {}", e)),
    }
}

/// Starts background synchronization for the pending files.
#[tauri::command]
async fn start_sync(
    app: AppHandle,
    state: State<'_, AppState>,
    device_id: String,
    backup_dir: String,
    files: Vec<ScannedFile>,
) -> Result<(), String> {
    if state.is_syncing.load(Ordering::Relaxed) {
        return Err("Sync is already running".to_string());
    }

    state.is_syncing.store(true, Ordering::Relaxed);
    state
        .controller
        .cancel_sync_flag
        .store(false, Ordering::Relaxed);
    state
        .controller
        .abort_or_skip_flag
        .store(false, Ordering::Relaxed);

    let is_syncing = state.is_syncing.clone();
    let controller = SyncController {
        abort_or_skip_flag: state.controller.abort_or_skip_flag.clone(),
        cancel_sync_flag: state.controller.cancel_sync_flag.clone(),
    };
    let storage_dir = get_storage_dir(&app);
    let backup_path = PathBuf::from(&backup_dir);
    let app_handle = app.clone();

    // Filter to files that actually need to be transferred (NEW and MISMATCH)
    let pending_files: Vec<ScannedFile> = files
        .into_iter()
        .filter(|f| f.status == FileClassification::New || f.status == FileClassification::Mismatch)
        .collect();

    if pending_files.is_empty() {
        state.is_syncing.store(false, Ordering::Relaxed);
        return Ok(());
    }

    tokio::task::spawn_blocking(move || {
        let _com = wpd::ComGuard::new();
        let _ = sync::execute_sync(
            &device_id,
            &backup_path,
            &storage_dir,
            &pending_files,
            &controller,
            |progress| {
                let _ = app_handle.emit("sync-progress", progress);
            },
        );
        is_syncing.store(false, Ordering::Relaxed);
    });

    Ok(())
}

/// Signals the active transfer loop to immediately skip the current file.
#[tauri::command]
fn skip_current_file(state: State<'_, AppState>) -> Result<(), String> {
    state
        .controller
        .abort_or_skip_flag
        .store(true, Ordering::Relaxed);
    Ok(())
}

/// Cancels the entire synchronization process.
#[tauri::command]
fn cancel_sync(state: State<'_, AppState>) -> Result<(), String> {
    state
        .controller
        .cancel_sync_flag
        .store(true, Ordering::Relaxed);
    state
        .controller
        .abort_or_skip_flag
        .store(true, Ordering::Relaxed);
    Ok(())
}

/// Retrieves the persisted blacklist/skip list.
#[tauri::command]
fn get_skip_list(app: AppHandle) -> Result<Vec<String>, String> {
    let storage_dir = get_storage_dir(&app);
    let set = sync::load_skip_list(&storage_dir);
    let mut list: Vec<String> = set.into_iter().collect();
    list.sort();
    Ok(list)
}

/// Removes a specific relative path from the blacklist/skip list.
#[tauri::command]
fn remove_from_skip_list(app: AppHandle, relative_path: String) -> Result<Vec<String>, String> {
    let storage_dir = get_storage_dir(&app);
    let mut set = sync::load_skip_list(&storage_dir);
    let norm = sync::normalize_rel_path(&relative_path);
    set.remove(&norm);
    sync::save_skip_list(&storage_dir, &set)?;
    let mut list: Vec<String> = set.into_iter().collect();
    list.sort();
    Ok(list)
}

/// Clears all entries from the blacklist/skip list.
#[tauri::command]
fn clear_skip_list(app: AppHandle) -> Result<Vec<String>, String> {
    let storage_dir = get_storage_dir(&app);
    sync::save_skip_list(&storage_dir, &std::collections::HashSet::new())?;
    Ok(Vec::new())
}

/// Application entry point configuring Tauri plugins, state, and handlers.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState {
        controller: SyncController::new(),
        is_syncing: Arc::new(AtomicBool::new(false)),
        is_scanning: Arc::new(AtomicBool::new(false)),
    };

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_devices,
            scan_device,
            start_sync,
            skip_current_file,
            cancel_sync,
            get_skip_list,
            remove_from_skip_list,
            clear_skip_list,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
