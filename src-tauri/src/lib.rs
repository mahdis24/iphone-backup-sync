pub mod sync;
pub mod wpd;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::sync::{FileClassification, ScanProgressPayload, ScanResult, ScannedFile, SyncController};
use crate::wpd::DeviceInfo;

/// Shared application state managed by Tauri.
pub struct AppState {
    /// Controller holding thread-safe flags for skipping or cancelling transfers.
    pub controller: SyncController,
    /// Atomic flag indicating whether a synchronization batch is currently active.
    pub is_syncing: Arc<AtomicBool>,
    /// Atomic flag indicating whether an MTP device scan is currently in progress.
    pub is_scanning: Arc<AtomicBool>,
    /// Atomic flag to request cancellation of the current device scan.
    pub cancel_scan_flag: Arc<AtomicBool>,
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
    state.cancel_scan_flag.store(false, Ordering::Relaxed);
    let is_scanning = state.is_scanning.clone();
    let cancel_scan_flag = state.cancel_scan_flag.clone();
    let storage_dir = get_storage_dir(&app);
    let backup_path = PathBuf::from(&backup_dir);
    let app_handle = app.clone();

    let res = tokio::task::spawn_blocking(move || {
        let _com = wpd::ComGuard::new();

        let _ = app_handle.emit("scan-progress", ScanProgressPayload {
            stage: "connecting".to_string(),
            message: "Opening session with iPhone...".to_string(),
            current: 0,
            total: 0,
            percent: 5,
        });

        let _ = app_handle.emit("scan-progress", ScanProgressPayload {
            stage: "connecting".to_string(),
            message: "Opening session with iPhone...".to_string(),
            current: 0,
            total: 0,
            percent: 5,
        });

        let device_guard = wpd::open_device(&device_id)?;

        if cancel_scan_flag.load(Ordering::Relaxed) {
            return Err("Scan cancelled by user".to_string());
        }

        let _ = app_handle.emit("scan-progress", ScanProgressPayload {
            stage: "enumerating".to_string(),
            message: "Scanning iPhone media storage...".to_string(),
            current: 0,
            total: 0,
            percent: 10,
        });

        let mut count = 0;
        let mut last_emit = std::time::Instant::now();

        let files = wpd::scan_files(&device_guard, &cancel_scan_flag, |folder_or_path, maybe_file| {
            if maybe_file.is_some() {
                count += 1;
            }
            if last_emit.elapsed().as_millis() >= 60 || count % 20 == 0 {
                last_emit = std::time::Instant::now();
                let msg = match maybe_file {
                    Some(_) => format!("Scanning iPhone media... ({} files found)", count),
                    None => format!("Scanning folder: {} ({} files found)", folder_or_path, count),
                };
                let pct = 10 + std::cmp::min(25, (count as f64 / 150.0).round() as u8);
                let _ = app_handle.emit("scan-progress", ScanProgressPayload {
                    stage: "enumerating".to_string(),
                    message: msg,
                    current: count,
                    total: 0,
                    percent: pct,
                });
            }
        })?;

        if cancel_scan_flag.load(Ordering::Relaxed) {
            return Err("Scan cancelled by user".to_string());
        }

        let _ = app_handle.emit("scan-progress", ScanProgressPayload {
            stage: "indexing".to_string(),
            message: "Indexing local backup directory...".to_string(),
            current: 0,
            total: 0,
            percent: 35,
        });

        let mut last_idx_emit = std::time::Instant::now();
        let backup_index = sync::index_backup_directory(&backup_path, &cancel_scan_flag, |idx_count| {
            if last_idx_emit.elapsed().as_millis() >= 60 {
                last_idx_emit = std::time::Instant::now();
                let pct = 35 + std::cmp::min(28, (idx_count as f64 / 800.0).round() as u8);
                let _ = app_handle.emit("scan-progress", ScanProgressPayload {
                    stage: "indexing".to_string(),
                    message: format!("Indexing local backup directory ({} files scanned)...", idx_count),
                    current: idx_count,
                    total: 0,
                    percent: pct,
                });
            }
        })?;

        if cancel_scan_flag.load(Ordering::Relaxed) {
            return Err("Scan cancelled by user".to_string());
        }

        let total_iphone = files.len();
        let _ = app_handle.emit("scan-progress", ScanProgressPayload {
            stage: "classifying".to_string(),
            message: format!("Comparing {} iPhone files with backup...", total_iphone),
            current: 0,
            total: total_iphone,
            percent: 65,
        });

        let mut last_cls_emit = std::time::Instant::now();
        let scan_result = sync::diff_and_classify_files(
            &files,
            &backup_index,
            &storage_dir,
            &cancel_scan_flag,
            |curr, tot| {
                if last_cls_emit.elapsed().as_millis() >= 60 || curr == tot {
                    last_cls_emit = std::time::Instant::now();
                    let pct = if tot > 0 {
                        65 + ((curr as f64 / tot as f64) * 33.0).round() as u8
                    } else {
                        98
                    };
                    let _ = app_handle.emit("scan-progress", ScanProgressPayload {
                        stage: "classifying".to_string(),
                        message: format!("Comparing files: {} / {} ({}%)", curr, tot, pct),
                        current: curr,
                        total: tot,
                        percent: pct,
                    });
                }
            },
        )?;

        let _ = app_handle.emit("scan-progress", ScanProgressPayload {
            stage: "completed".to_string(),
            message: format!("Scan complete: {} files discovered.", files.len()),
            current: files.len(),
            total: files.len(),
            percent: 100,
        });

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

    // Filter to files that actually need to be transferred (NEW, MISMATCH, or retry ERROR)
    let pending_files: Vec<ScannedFile> = files
        .into_iter()
        .filter(|f| {
            f.status == FileClassification::New
                || f.status == FileClassification::Mismatch
                || f.status == FileClassification::Error
        })
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

/// Adds a specific relative path to the blacklist/skip list.
#[tauri::command]
fn add_to_skip_list(app: AppHandle, relative_path: String) -> Result<Vec<String>, String> {
    let storage_dir = get_storage_dir(&app);
    let mut set = sync::load_skip_list(&storage_dir);
    let norm = sync::normalize_rel_path(&relative_path);
    set.insert(norm);
    sync::save_skip_list(&storage_dir, &set)?;
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

/// Signals the active scan task to immediately cancel.
#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) -> Result<(), String> {
    state.cancel_scan_flag.store(true, Ordering::Relaxed);
    Ok(())
}

/// Application entry point configuring Tauri plugins, state, and handlers.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = AppState {
        controller: SyncController::new(),
        is_syncing: Arc::new(AtomicBool::new(false)),
        is_scanning: Arc::new(AtomicBool::new(false)),
        cancel_scan_flag: Arc::new(AtomicBool::new(false)),
    };

    tauri::Builder::default()
        .manage(app_state)
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_devices,
            scan_device,
            cancel_scan,
            start_sync,
            skip_current_file,
            cancel_sync,
            get_skip_list,
            add_to_skip_list,
            remove_from_skip_list,
            clear_skip_list,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
