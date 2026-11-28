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

pub fn remove_part_file(target_path: &Path) {
    let part = crate::wpd::get_part_path(target_path);
    if part.exists() {
        let _ = fs::remove_file(&part);
    }
    let legacy_part = target_path.with_extension(format!(
        "{}.part",
        target_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
    ));
    if legacy_part != part && legacy_part.exists() {
        let _ = fs::remove_file(&legacy_part);
    }
}

pub fn handle_crash_recovery(storage_dir: &Path) -> Option<String> {
    let lock_path = get_lock_file_path(storage_dir);
    if let Ok(content) = fs::read_to_string(&lock_path) {
        if let Ok(info) = serde_json::from_str::<CopyingLockInfo>(&content) {
            let norm_path = normalize_rel_path(&info.relative_path);

            // Clean up any incomplete partial file
            let target = PathBuf::from(&info.target_path);
            remove_part_file(&target);

            // Remove the lock file
            let _ = fs::remove_file(&lock_path);

            // Note: DO NOT add to skip_list! An interrupted transfer should NOT
            // blacklist the user's photo from future syncs.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFileInfo {
    pub actual_relative_path: String,
    pub size: u64,
}

pub fn index_backup_directory<F>(
    backup_dir: &Path,
    cancel_flag: &AtomicBool,
    mut on_progress: F,
) -> Result<HashMap<String, BackupFileInfo>, String>
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
            // Ignore incomplete .part fragments
            if norm.ends_with(".part") {
                continue;
            }
            let lower = norm.to_lowercase();
            map.insert(
                lower,
                BackupFileInfo {
                    actual_relative_path: norm,
                    size: meta.len(),
                },
            );
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
    backup_index: &HashMap<String, BackupFileInfo>,
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
        let in_skip = skip_set.contains(&norm_rel) || skip_set.contains(&norm_rel.to_lowercase());

        let lower_rel = norm_rel.to_lowercase();
        let existing_entry = backup_index.get(&lower_rel);
        let existing_size = existing_entry.map(|e| e.size);

        let (status, target_rel_path) = match existing_entry {
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
            Some(existing) => {
                if existing.size == item.size {
                    // Exact same size already exists in backup (case-insensitive match)
                    summary.verified_ignored_count += 1;
                    (
                        FileClassification::VerifiedIgnored,
                        existing.actual_relative_path.clone(),
                    )
                } else {
                    // Size differs.
                    // Check if a `_new` variant (e.g. IMG_1234_new.HEIC)
                    // already exists in the same backup folder with the EXACT same byte size as the iPhone file.
                    let new_variant_rel = get_new_variant_rel_path(&norm_rel);
                    let lower_variant = new_variant_rel.to_lowercase();
                    let variant_entry = backup_index.get(&lower_variant);

                    if let Some(v_info) = variant_entry {
                        if v_info.size == item.size {
                            // Verified _new variant already exists with exact same size -> ignore/skip completely
                            summary.verified_ignored_count += 1;
                            (
                                FileClassification::VerifiedIgnored,
                                v_info.actual_relative_path.clone(),
                            )
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
                    } else {
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

const SESSION_REOPEN_DELAY_MS: u64 = 250;
const MAX_EAGER_SESSION_REOPENS: usize = 3;
const PERIODIC_REOPEN_INTERVAL: usize = 25;

/// Returns `true` when a destination filesystem error indicates out-of-space
/// or a fatal storage condition where retrying remaining files cannot succeed.
pub fn is_fatal_disk_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("disk full")
        || lower.contains("os error 112")
        || lower.contains("os error 39")
        || lower.contains("not enough space")
}

/// Returns `true` when a WPD/MTP error may stem from a stale object ID,
/// wedged MTP session, or transient device communication fault.
pub fn is_recoverable_wpd_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("0x8007065d")
        || lower.contains("0x8007001f")
        || lower.contains("0x80070015")
        || lower.contains("0x8007001e")
        || lower.contains("0x800700aa")
        || lower.contains("0x80070490")
        || lower.contains("failed to get device content")
        || lower.contains("failed to get iportabledeviceresources")
        || lower.contains("stream read error")
        || lower.contains("incomplete transfer")
        || lower.contains("device session is unavailable")
}

/// Limits full session reopens when many consecutive files fail identically,
/// recovering immediately for transient glitches without stalling huge queues.
pub fn should_reopen_session(consecutive_errors: usize) -> bool {
    consecutive_errors > 0
        && (consecutive_errors <= MAX_EAGER_SESSION_REOPENS
            || consecutive_errors.is_multiple_of(PERIODIC_REOPEN_INTERVAL))
}

fn interruptible_sleep(duration_ms: u64, controller: &SyncController) {
    let step_ms = 50;
    let mut elapsed = 0;
    while elapsed < duration_ms {
        if controller.abort_or_skip_flag.load(Ordering::Relaxed)
            || controller.cancel_sync_flag.load(Ordering::Relaxed)
        {
            break;
        }
        let chunk = std::cmp::min(step_ms, duration_ms - elapsed);
        std::thread::sleep(std::time::Duration::from_millis(chunk));
        elapsed += chunk;
    }
}

#[allow(clippy::too_many_arguments)]
fn stream_with_progress<F>(
    device: &crate::wpd::WpdDeviceGuard,
    object_id: &str,
    dest_path: &Path,
    file_item: &ScannedFile,
    index: usize,
    total_files: usize,
    abort_flag: &AtomicBool,
    last_copy_emit: &mut Option<std::time::Instant>,
    on_progress: &mut F,
) -> Result<(), String>
where
    F: FnMut(SyncProgressPayload),
{
    stream_file_to_disk(
        device,
        object_id,
        dest_path,
        file_item.size,
        abort_flag,
        |copied, total, speed| {
            *last_copy_emit = Some(std::time::Instant::now());
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
    )
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
    let mut device: Option<crate::wpd::WpdDeviceGuard> =
        Some(crate::wpd::open_device(device_pnp_id)?);

    let total_files = files_to_sync.len();
    let lock_path = get_lock_file_path(storage_dir);
    let mut was_cancelled = false;
    let mut fatal_abort_error: Option<String> = None;
    let mut consecutive_session_errors: usize = 0;
    let mut last_copy_emit: Option<std::time::Instant> = None;

    for (index, file_item) in files_to_sync.iter().enumerate() {
        if controller.cancel_sync_flag.load(Ordering::Relaxed) {
            was_cancelled = true;
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

        // Throttle initial "copying" events across rapid small files so IPC queues never back up
        if last_copy_emit.is_none_or(|t| t.elapsed().as_millis() >= 100) {
            last_copy_emit = Some(std::time::Instant::now());
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
        }

        let mut active_obj_id = file_item.object_id.clone();
        let mut stream_res = match device.as_ref() {
            Some(dev) => stream_with_progress(
                dev,
                &active_obj_id,
                &dest_path,
                file_item,
                index,
                total_files,
                &controller.abort_or_skip_flag,
                &mut last_copy_emit,
                &mut on_progress,
            ),
            None => Err("Device session is unavailable".to_string()),
        };

        // If transfer failed due to a recoverable WPD/MTP error, reset the WPD session
        // (closing the old handle first) and/or resolve a fresh object ID before retrying.
        if let Err(ref err) = stream_res
            && !controller.abort_or_skip_flag.load(Ordering::Relaxed)
            && !controller.cancel_sync_flag.load(Ordering::Relaxed)
            && is_recoverable_wpd_error(err)
        {
            consecutive_session_errors += 1;
            let mut reopened = false;

            if should_reopen_session(consecutive_session_errors) {
                drop(device.take());
                interruptible_sleep(SESSION_REOPEN_DELAY_MS, controller);

                if !controller.abort_or_skip_flag.load(Ordering::Relaxed)
                    && !controller.cancel_sync_flag.load(Ordering::Relaxed)
                    && let Ok(new_dev) = crate::wpd::open_device(device_pnp_id)
                {
                    device = Some(new_dev);
                    reopened = true;
                }
            }

            if let Some(ref dev) = device
                && !controller.abort_or_skip_flag.load(Ordering::Relaxed)
                && !controller.cancel_sync_flag.load(Ordering::Relaxed)
            {
                let mut resolved_new_id = false;
                if (reopened || consecutive_session_errors <= MAX_EAGER_SESSION_REOPENS)
                    && let Ok(Some(fresh_id)) = crate::wpd::find_object_id_by_path(
                        dev,
                        &file_item.relative_path,
                        &controller.abort_or_skip_flag,
                    )
                    && fresh_id != active_obj_id
                {
                    active_obj_id = fresh_id;
                    resolved_new_id = true;
                }

                if reopened || resolved_new_id {
                    stream_res = stream_with_progress(
                        dev,
                        &active_obj_id,
                        &dest_path,
                        file_item,
                        index,
                        total_files,
                        &controller.abort_or_skip_flag,
                        &mut last_copy_emit,
                        &mut on_progress,
                    );
                }
            }
        }

        // Always clean up lock file after file finishes, fails, or cancels
        let _ = fs::remove_file(&lock_path);

        // Check if user requested cancellation of the whole sync
        if controller.cancel_sync_flag.load(Ordering::Relaxed) {
            was_cancelled = true;
            remove_part_file(&dest_path);
            // DO NOT add to skip list! This was a stop/cancel, not a skip.
            break;
        }

        match stream_res {
            Ok(()) => {
                consecutive_session_errors = 0;
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
                remove_part_file(&dest_path);

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
                    let is_disk_full = is_fatal_disk_error(&err);
                    let final_err = if consecutive_session_errors >= MAX_EAGER_SESSION_REOPENS
                        && err.to_ascii_lowercase().contains("0x8007065d")
                        && !err.contains("hint:")
                    {
                        format!(
                            "{} (hint: ensure iCloud 'Download and Keep Originals' is enabled, 'Transfer to Mac or PC' is 'Keep Originals', and try a direct USB port)",
                            err
                        )
                    } else {
                        err
                    };

                    on_progress(SyncProgressPayload {
                        current_file_index: index + 1,
                        total_files,
                        filename: file_item.filename.clone(),
                        relative_path: file_item.relative_path.clone(),
                        bytes_copied: 0,
                        total_bytes: file_item.size,
                        status: "error".to_string(),
                        speed_bytes_sec: 0.0,
                        error: Some(final_err.clone()),
                    });

                    if is_disk_full {
                        fatal_abort_error = Some(format!(
                            "{} (Sync stopped automatically because the destination disk is full)",
                            final_err
                        ));
                        break;
                    }
                }
            }
        }
    }

    // Always emit a terminal batch event so the frontend knows the process has definitely finished
    let (terminal_status, terminal_error) = if let Some(fatal_err) = fatal_abort_error {
        ("aborted".to_string(), Some(fatal_err))
    } else if was_cancelled {
        (
            "aborted".to_string(),
            Some("Sync stopped by user".to_string()),
        )
    } else {
        ("finished".to_string(), None)
    };

    on_progress(SyncProgressPayload {
        current_file_index: total_files,
        total_files,
        filename: String::new(),
        relative_path: String::new(),
        bytes_copied: 0,
        total_bytes: 0,
        status: terminal_status,
        speed_bytes_sec: 0.0,
        error: terminal_error,
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_recoverable_wpd_error() {
        assert!(is_recoverable_wpd_error(
            "Failed to get file stream for o54CA: Die angegebenen Daten haben den falschen Typ. (0x8007065D)"
        ));
        assert!(is_recoverable_wpd_error("some error 0x8007065d"));
        assert!(is_recoverable_wpd_error("Device fault 0x8007001F"));
        assert!(is_recoverable_wpd_error("Not ready 0x80070015"));
        assert!(is_recoverable_wpd_error("Stream read error: E_FAIL"));
        assert!(is_recoverable_wpd_error(
            "Incomplete transfer: received 10 of 20 bytes"
        ));

        assert!(!is_recoverable_wpd_error("Disk write error: Access denied"));
        assert!(!is_recoverable_wpd_error("Aborted by user"));
    }

    #[test]
    fn test_is_fatal_disk_error() {
        assert!(is_fatal_disk_error(
            "Disk full (Disk write error): There is not enough space on the disk. (os error 112)"
        ));
        assert!(is_fatal_disk_error("Write failed (os error 39)"));
        assert!(is_fatal_disk_error(
            "Disk write error: There is not enough space on the disk."
        ));
        assert!(!is_fatal_disk_error("Stream read error: 0x8007065D"));
        assert!(!is_fatal_disk_error("Aborted by user"));
    }

    #[test]
    fn test_should_reopen_session_backoff_schedule() {
        assert!(!should_reopen_session(0));
        assert!(should_reopen_session(1));
        assert!(should_reopen_session(2));
        assert!(should_reopen_session(3));
        assert!(!should_reopen_session(4));
        assert!(!should_reopen_session(24));
        assert!(should_reopen_session(25));
        assert!(!should_reopen_session(26));
        assert!(should_reopen_session(50));
    }

    #[test]
    fn test_new_variant_and_normalization() {
        assert_eq!(
            normalize_rel_path("\\DCIM\\100APPLE\\IMG_1234.HEIC"),
            "DCIM/100APPLE/IMG_1234.HEIC"
        );
        assert_eq!(
            get_new_variant_rel_path("DCIM/100APPLE/IMG_1234.HEIC"),
            "DCIM/100APPLE/IMG_1234_new.HEIC"
        );
    }

    #[test]
    fn test_diff_and_classify_files() {
        let temp_dir =
            std::env::temp_dir().join(format!("iphone_backup_sync_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).unwrap();

        let iphone_files = vec![
            DeviceFile {
                object_id: "o1".to_string(),
                relative_path: "DCIM/100APPLE/IMG_0001.JPG".to_string(),
                filename: "IMG_0001.JPG".to_string(),
                size: 100,
            },
            DeviceFile {
                object_id: "o2".to_string(),
                relative_path: "DCIM/100APPLE/IMG_0002.JPG".to_string(),
                filename: "IMG_0002.JPG".to_string(),
                size: 200,
            },
            DeviceFile {
                object_id: "o3".to_string(),
                relative_path: "DCIM/100APPLE/IMG_0003.JPG".to_string(),
                filename: "IMG_0003.JPG".to_string(),
                size: 300,
            },
        ];

        let mut backup_index = HashMap::new();
        backup_index.insert(
            "dcim/100apple/img_0001.jpg".to_string(),
            BackupFileInfo {
                actual_relative_path: "DCIM/100APPLE/IMG_0001.JPG".to_string(),
                size: 100,
            },
        );
        backup_index.insert(
            "dcim/100apple/img_0002.jpg".to_string(),
            BackupFileInfo {
                actual_relative_path: "DCIM/100APPLE/IMG_0002.JPG".to_string(),
                size: 150,
            },
        );

        let cancel = AtomicBool::new(false);
        let result =
            diff_and_classify_files(&iphone_files, &backup_index, &temp_dir, &cancel, |_, _| {})
                .unwrap();

        assert_eq!(result.summary.verified_ignored_count, 1);
        assert_eq!(result.summary.mismatch_count, 1);
        assert_eq!(result.summary.new_count, 1);
        assert_eq!(
            result.files[1].target_relative_path,
            "DCIM/100APPLE/IMG_0002_new.JPG"
        );

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
