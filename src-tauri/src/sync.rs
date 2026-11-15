use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use walkdir::WalkDir;

use crate::wpd::{DeviceFile, stream_file_to_disk};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileClassification {
    #[serde(rename = "NEW")]
    New,
    #[serde(rename = "MISMATCH")]
    Mismatch,
    #[serde(rename = "VERIFIED_IGNORED")]
    VerifiedIgnored,
    #[serde(rename = "SKIPPED")]
    Skipped,
    #[serde(rename = "ERROR")]
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedFile {
    pub object_id: String,
    pub relative_path: String,
    pub filename: String,
    pub size: u64,
    pub status: FileClassification,
    pub target_relative_path: String,
    pub existing_size: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanSummary {
    pub total_iphone_files: usize,
    pub total_backup_files: usize,
    pub new_count: usize,
    pub mismatch_count: usize,
    pub verified_ignored_count: usize,
    pub skipped_count: usize,
    #[serde(default)]
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgressPayload {
    pub stage: String,
    pub message: String,
    pub current: usize,
    pub total: usize,
    pub percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub files: Vec<ScannedFile>,
    pub summary: ScanSummary,
    pub skip_list: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncProgressPayload {
    pub current_file_index: usize,
    pub total_files: usize,
    pub filename: String,
    pub relative_path: String,
    pub bytes_copied: u64,
    pub total_bytes: u64,
    pub status: String, // "copying", "completed", "skipped", "error", "aborted"
    pub speed_bytes_sec: f64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CopyingLockInfo {
    pub relative_path: String,
    pub target_path: String,
    pub timestamp: u64,
}

pub fn normalize_rel_path(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches('/').to_string()
}

pub fn get_skip_list_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("skip_list.json")
}

pub fn get_lock_file_path(storage_dir: &Path) -> PathBuf {
    storage_dir.join("current_copying.tmp")
}

pub fn load_skip_list(storage_dir: &Path) -> HashSet<String> {
    let path = get_skip_list_path(storage_dir);
    let list: Option<Vec<String>> = fs::read_to_string(&path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok());

    if let Some(entries) = list {
        entries
            .into_iter()
            .map(|s| normalize_rel_path(&s))
            .collect()
    } else {
        HashSet::new()
    }
}

pub fn save_skip_list(storage_dir: &Path, list: &HashSet<String>) -> Result<(), String> {
    let path = get_skip_list_path(storage_dir);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut vec: Vec<String> = list.iter().cloned().collect();
    vec.sort();
    let json = serde_json::to_string_pretty(&vec).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| format!("Failed to write skip_list.json: {}", e))
}

pub fn handle_crash_recovery(storage_dir: &Path) -> Option<String> {
    let lock_path = get_lock_file_path(storage_dir);
    if let Ok(content) = fs::read_to_string(&lock_path) {
        if let Ok(info) = serde_json::from_str::<CopyingLockInfo>(&content) {
            let norm_path = normalize_rel_path(&info.relative_path);
            // Add to skip list
            let mut list = load_skip_list(storage_dir);
            list.insert(norm_path.clone());
            let _ = save_skip_list(storage_dir, &list);

            // Clean up any incomplete partial file
            let target = PathBuf::from(&info.target_path);
            let part_path = target.with_extension(format!(
                "{}.part",
                target.extension().and_then(|s| s.to_str()).unwrap_or("")
            ));
            if part_path.exists() {
                let _ = fs::remove_file(&part_path);
            }
            let alt_part = PathBuf::from(format!("{}.part", info.target_path));
            if alt_part.exists() {
                let _ = fs::remove_file(&alt_part);
            }

            // Remove the lock file
            let _ = fs::remove_file(&lock_path);
            return Some(norm_path);
        }
        let _ = fs::remove_file(&lock_path);
    }
    None
}

/// Compute `_new` variant for a relative path, e.g.:
/// "DCIM/100APPLE/IMG_1234.HEIC" -> "DCIM/100APPLE/IMG_1234_new.HEIC"
pub fn get_new_variant_rel_path(rel_path: &str) -> String {
    let path = Path::new(rel_path);
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let file_stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let extension = path.extension().and_then(|s| s.to_str());

    let new_filename = match extension {
        Some(ext) => format!("{}_new.{}", file_stem, ext),
        None => format!("{}_new", file_stem),
    };

    if parent.as_os_str().is_empty() {
        new_filename
    } else {
        format!(
            "{}/{}",
            normalize_rel_path(&parent.to_string_lossy()),
            new_filename
        )
    }
}

pub fn index_backup_directory<F>(
    backup_dir: &Path,
    cancel_flag: &AtomicBool,
    mut on_progress: F,
) -> Result<HashMap<String, u64>, String>
where
    F: FnMut(usize),
{
    let mut map = HashMap::new();
    if !backup_dir.exists() {
        return Ok(map);
    }

    let mut count = 0;
    for entry in WalkDir::new(backup_dir).into_iter().filter_map(|e| e.ok()) {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err("Scan cancelled by user".to_string());
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if let (Ok(rel), Ok(meta)) = (entry.path().strip_prefix(backup_dir), entry.metadata()) {
            let norm = normalize_rel_path(&rel.to_string_lossy());
            map.insert(norm, meta.len());
            count += 1;
            if count % 200 == 0 {
                on_progress(count);
            }
        }
    }
    on_progress(count);

    Ok(map)
}

pub fn diff_and_classify_files<F>(
    iphone_files: &[DeviceFile],
    backup_index: &HashMap<String, u64>,
    storage_dir: &Path,
    cancel_flag: &AtomicBool,
    mut on_progress: F,
) -> Result<ScanResult, String>
where
    F: FnMut(usize, usize),
{
    // Check crash recovery first
    let _ = handle_crash_recovery(storage_dir);

    let skip_set = load_skip_list(storage_dir);

    let mut scanned_files = Vec::with_capacity(iphone_files.len());
    let mut summary = ScanSummary {
        total_iphone_files: iphone_files.len(),
        total_backup_files: backup_index.len(),
        ..Default::default()
    };

    let total = iphone_files.len();

    for (idx, item) in iphone_files.iter().enumerate() {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err("Scan cancelled by user".to_string());
        }

        if (idx + 1) % 100 == 0 || idx + 1 == total {
            on_progress(idx + 1, total);
        }

        let norm_rel = normalize_rel_path(&item.relative_path);
        let in_skip = skip_set.contains(&norm_rel);

        let existing_size = backup_index.get(&norm_rel).copied();

        let (status, target_rel_path) = match existing_size {
            None => {
                // Case 1: Relative path does not exist in backup -> NEW
                if in_skip {
                    summary.skipped_count += 1;
                    (FileClassification::Skipped, norm_rel.clone())
                } else {
                    summary.new_count += 1;
                    (FileClassification::New, norm_rel.clone())
                }
            }
            Some(ex_size) => {
                if ex_size == item.size {
                    // Exact same size already exists in backup
                    summary.verified_ignored_count += 1;
                    (FileClassification::VerifiedIgnored, norm_rel.clone())
                } else {
                    // Size differs.
                    // Crucial Rule: Check if a `_new` variant (e.g. IMG_1234_new.HEIC)
                    // already exists in the same backup folder with the EXACT same byte size as the iPhone file.
                    let new_variant_rel = get_new_variant_rel_path(&norm_rel);
                    let variant_size = backup_index.get(&new_variant_rel).copied();

                    if variant_size == Some(item.size) {
                        // Verified _new variant already exists with exact same size -> ignore/skip completely
                        summary.verified_ignored_count += 1;
                        (FileClassification::VerifiedIgnored, new_variant_rel)
                    } else {
                        // Mark as MISMATCH and write directly to <filename>_new.<ext>
                        if in_skip {
                            summary.skipped_count += 1;
                            (FileClassification::Skipped, new_variant_rel)
                        } else {
                            summary.mismatch_count += 1;
                            (FileClassification::Mismatch, new_variant_rel)
                        }
                    }
                }
            }
        };

        scanned_files.push(ScannedFile {
            object_id: item.object_id.clone(),
            relative_path: norm_rel,
            filename: item.filename.clone(),
            size: item.size,
            status,
            target_relative_path: target_rel_path,
            existing_size,
        });
    }

    let mut skip_list: Vec<String> = skip_set.into_iter().collect();
    skip_list.sort();

    Ok(ScanResult {
        files: scanned_files,
        summary,
        skip_list,
    })
}

pub struct SyncController {
    pub abort_or_skip_flag: Arc<AtomicBool>,
    pub cancel_sync_flag: Arc<AtomicBool>,
}

impl SyncController {
    pub fn new() -> Self {
        Self {
            abort_or_skip_flag: Arc::new(AtomicBool::new(false)),
            cancel_sync_flag: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Default for SyncController {
    fn default() -> Self {
        Self::new()
    }
}

pub fn execute_sync<F>(
    device_pnp_id: &str,
    backup_dir: &Path,
    storage_dir: &Path,
    files_to_sync: &[ScannedFile],
    controller: &SyncController,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(SyncProgressPayload),
{
    // Connect to device
    let device = crate::wpd::open_device(device_pnp_id)?;

    let total_files = files_to_sync.len();
    let lock_path = get_lock_file_path(storage_dir);
    let mut was_cancelled = false;

    for (index, file_item) in files_to_sync.iter().enumerate() {
        if controller.cancel_sync_flag.load(Ordering::Relaxed) {
            was_cancelled = true;
            on_progress(SyncProgressPayload {
                current_file_index: index,
                total_files,
                filename: file_item.filename.clone(),
                relative_path: file_item.relative_path.clone(),
                bytes_copied: 0,
                total_bytes: file_item.size,
                status: "aborted".to_string(),
                speed_bytes_sec: 0.0,
                error: Some("Sync stopped by user".to_string()),
            });
            break;
        }

        // Reset the per-file skip flag
        controller
            .abort_or_skip_flag
            .store(false, Ordering::Relaxed);

        let dest_path = backup_dir.join(&file_item.target_relative_path);

        // Write current_copying.tmp lock file
        let lock_info = CopyingLockInfo {
            relative_path: file_item.relative_path.clone(),
            target_path: dest_path.to_string_lossy().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        if let Ok(lock_json) = serde_json::to_string(&lock_info) {
            let _ = fs::write(&lock_path, lock_json);
        }

        // Emit starting progress
        on_progress(SyncProgressPayload {
            current_file_index: index + 1,
            total_files,
            filename: file_item.filename.clone(),
            relative_path: file_item.relative_path.clone(),
            bytes_copied: 0,
            total_bytes: file_item.size,
            status: "copying".to_string(),
            speed_bytes_sec: 0.0,
            error: None,
        });

        let stream_res = stream_file_to_disk(
            &device,
            &file_item.object_id,
            &dest_path,
            file_item.size,
            &controller.abort_or_skip_flag,
            |copied, total, speed| {
                on_progress(SyncProgressPayload {
                    current_file_index: index + 1,
                    total_files,
                    filename: file_item.filename.clone(),
                    relative_path: file_item.relative_path.clone(),
                    bytes_copied: copied,
                    total_bytes: total,
                    status: "copying".to_string(),
                    speed_bytes_sec: speed,
                    error: None,
                });
                Ok(())
            },
        );

        // Always clean up lock file after file finishes, fails, or cancels
        let _ = fs::remove_file(&lock_path);

        // Helper to remove any leftover partial file for this target
        let clean_part_file = || {
            let part_path = dest_path.with_extension(format!(
                "{}.part",
                dest_path.extension().and_then(|s| s.to_str()).unwrap_or("")
            ));
            if part_path.exists() {
                let _ = fs::remove_file(&part_path);
            }
            let alt_part = PathBuf::from(format!("{}.part", dest_path.to_string_lossy()));
            if alt_part.exists() {
                let _ = fs::remove_file(&alt_part);
            }
        };

        // Check if user requested cancellation of the whole sync
        if controller.cancel_sync_flag.load(Ordering::Relaxed) {
            was_cancelled = true;
            clean_part_file();
            // DO NOT add to skip list! This was a stop/cancel, not a skip.
            on_progress(SyncProgressPayload {
                current_file_index: index + 1,
                total_files,
                filename: file_item.filename.clone(),
                relative_path: file_item.relative_path.clone(),
                bytes_copied: 0,
                total_bytes: file_item.size,
                status: "aborted".to_string(),
                speed_bytes_sec: 0.0,
                error: Some("Sync stopped by user".to_string()),
            });
            break;
        }

        match stream_res {
            Ok(()) => {
                on_progress(SyncProgressPayload {
                    current_file_index: index + 1,
                    total_files,
                    filename: file_item.filename.clone(),
                    relative_path: file_item.relative_path.clone(),
                    bytes_copied: file_item.size,
                    total_bytes: file_item.size,
                    status: "completed".to_string(),
                    speed_bytes_sec: 0.0,
                    error: None,
                });
            }
            Err(err) => {
                clean_part_file();

                let is_skipped = err.contains("Aborted")
                    || controller.abort_or_skip_flag.load(Ordering::Relaxed);
                if is_skipped {
                    // User explicitly skipped this file (via shortcut or Skip Current button)
                    let mut list = load_skip_list(storage_dir);
                    list.insert(normalize_rel_path(&file_item.relative_path));
                    let _ = save_skip_list(storage_dir, &list);

                    on_progress(SyncProgressPayload {
                        current_file_index: index + 1,
                        total_files,
                        filename: file_item.filename.clone(),
                        relative_path: file_item.relative_path.clone(),
                        bytes_copied: 0,
                        total_bytes: file_item.size,
                        status: "skipped".to_string(),
                        speed_bytes_sec: 0.0,
                        error: Some("Skipped by user".to_string()),
                    });
                } else {
                    on_progress(SyncProgressPayload {
                        current_file_index: index + 1,
                        total_files,
                        filename: file_item.filename.clone(),
                        relative_path: file_item.relative_path.clone(),
                        bytes_copied: 0,
                        total_bytes: file_item.size,
                        status: "error".to_string(),
                        speed_bytes_sec: 0.0,
                        error: Some(err),
                    });
                }
            }
        }
    }

    // Always emit a terminal batch event so the frontend knows the process has definitely finished
    on_progress(SyncProgressPayload {
        current_file_index: total_files,
        total_files,
        filename: String::new(),
        relative_path: String::new(),
        bytes_copied: 0,
        total_bytes: 0,
        status: if was_cancelled {
            "aborted".to_string()
        } else {
            "finished".to_string()
        },
        speed_bytes_sec: 0.0,
        error: None,
    });

    Ok(())
}
