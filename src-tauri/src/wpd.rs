use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use windows::Win32::Devices::PortableDevices::*;
use windows::Win32::System::Com::*;
use windows::core::{PCWSTR, PWSTR};

/// Information identifying an enumerated Windows Portable Device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// Plug and Play device identifier.
    pub id: String,
    /// Friendly name of the device (e.g., "Apple iPhone").
    pub name: String,
    /// Manufacturer name string reported by WPD.
    pub manufacturer: String,
    /// Description of the device reported by WPD.
    pub description: String,
    /// Whether this device was identified as an iOS / iPhone device.
    pub is_iphone: bool,
}

/// Metadata representation of a file scanned on a portable device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFile {
    /// Internal WPD object ID used for streaming.
    pub object_id: String,
    /// Normalized relative path under the device's storage root.
    pub relative_path: String,
    /// Clean file name without directional Unicode markers.
    pub filename: String,
    /// Exact file size in bytes.
    pub size: u64,
}

/// Strip invisible Unicode directional markers (e.g. \u{200E}, \u{200F}, \u{202A}..=\u{202E})
pub fn clean_unicode_markers(input: &str) -> String {
    input
        .chars()
        .filter(|&c| !matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}'))
        .collect()
}

/// Compute standard path with .part extension
pub fn get_part_path(dest_path: &Path) -> std::path::PathBuf {
    let mut s = dest_path.as_os_str().to_os_string();
    s.push(".part");
    std::path::PathBuf::from(s)
}

/// RAII guard ensuring COM library is initialized for the calling thread and uninitialized on drop.
pub struct ComGuard;

impl ComGuard {
    /// Initializes COM multithreaded runtime for the current thread.
    pub fn new() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        ComGuard
    }
}

impl Default for ComGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

/// Discovers connected Windows Portable Devices and identifies any connected iPhones.
pub fn get_connected_devices() -> Result<Vec<DeviceInfo>, String> {
    let _com = ComGuard::new();
    unsafe {
        let manager: IPortableDeviceManager =
            CoCreateInstance(&PortableDeviceManager, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to create IPortableDeviceManager: {}", e))?;

        let mut count: u32 = 0;
        manager
            .GetDevices(std::ptr::null_mut(), &mut count)
            .map_err(|e| format!("Failed to get device count: {}", e))?;

        if count == 0 {
            return Ok(Vec::new());
        }

        let mut device_ids = vec![PWSTR::null(); count as usize];
        manager
            .GetDevices(device_ids.as_mut_ptr(), &mut count)
            .map_err(|e| format!("Failed to enumerate device IDs: {}", e))?;

        let mut result = Vec::new();

        for pnp_id in device_ids {
            if pnp_id.is_null() {
                continue;
            }
            let id_str = pnp_id.to_string().unwrap_or_default();
            let pc_id = PCWSTR(pnp_id.as_ptr());

            let name =
                read_string_property(|buf, len| manager.GetDeviceFriendlyName(pc_id, buf, len));
            let manufacturer =
                read_string_property(|buf, len| manager.GetDeviceManufacturer(pc_id, buf, len));
            let description =
                read_string_property(|buf, len| manager.GetDeviceDescription(pc_id, buf, len));

            CoTaskMemFree(Some(pnp_id.as_ptr() as *const _));

            let clean_name = clean_unicode_markers(&name);
            let clean_manuf = clean_unicode_markers(&manufacturer);
            let clean_desc = clean_unicode_markers(&description);

            let is_iphone = clean_name.to_lowercase().contains("iphone")
                || clean_desc.to_lowercase().contains("iphone")
                || clean_manuf.to_lowercase().contains("apple");

            result.push(DeviceInfo {
                id: id_str,
                name: if clean_name.is_empty() {
                    "Apple iPhone".to_string()
                } else {
                    clean_name
                },
                manufacturer: clean_manuf,
                description: clean_desc,
                is_iphone,
            });
        }

        Ok(result)
    }
}

pub struct WpdDeviceGuard {
    device: IPortableDevice,
}

impl WpdDeviceGuard {
    pub fn new(device: IPortableDevice) -> Self {
        Self { device }
    }

    pub fn device(&self) -> &IPortableDevice {
        &self.device
    }
}

impl std::ops::Deref for WpdDeviceGuard {
    type Target = IPortableDevice;
    fn deref(&self) -> &Self::Target {
        &self.device
    }
}

impl Drop for WpdDeviceGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.Close();
        }
    }
}

pub fn open_device(pnp_id: &str) -> Result<WpdDeviceGuard, String> {
    unsafe {
        let device: IPortableDevice =
            match CoCreateInstance(&PortableDeviceFTM, None, CLSCTX_INPROC_SERVER) {
                Ok(d) => d,
                Err(_) => CoCreateInstance(&PortableDevice, None, CLSCTX_INPROC_SERVER)
                    .map_err(|e| format!("Failed to create IPortableDevice: {}", e))?,
            };

        let client_info: IPortableDeviceValues =
            CoCreateInstance(&PortableDeviceValues, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("Failed to create IPortableDeviceValues: {}", e))?;

        let app_name: Vec<u16> = "iPhone Backup Sync\0".encode_utf16().collect();
        let _ = client_info.SetStringValue(&WPD_CLIENT_NAME, PCWSTR(app_name.as_ptr()));
        let _ = client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MAJOR_VERSION, 1);
        let _ = client_info.SetUnsignedIntegerValue(&WPD_CLIENT_MINOR_VERSION, 0);
        let _ = client_info.SetUnsignedIntegerValue(&WPD_CLIENT_REVISION, 0);

        let pnp_id_u16: Vec<u16> = pnp_id.encode_utf16().chain(std::iter::once(0)).collect();
        device
            .Open(PCWSTR(pnp_id_u16.as_ptr()), &client_info)
            .map_err(|e| format!("Failed to open WPD device: {}. Please ensure your iPhone is unlocked and trusted.", e))?;

        Ok(WpdDeviceGuard::new(device))
    }
}

pub fn scan_files<F>(
    device: &IPortableDevice,
    cancel_flag: &AtomicBool,
    mut on_progress: F,
) -> Result<Vec<DeviceFile>, String>
where
    F: FnMut(&str, Option<&DeviceFile>),
{
    unsafe {
        let content = device
            .Content()
            .map_err(|e| format!("Failed to get device content: {}", e))?;
        let properties = content
            .Properties()
            .map_err(|e| format!("Failed to get content properties: {}", e))?;

        let mut all_files = Vec::new();
        // Traverse starting from root "DEVICE"
        traverse_object(
            &content,
            &properties,
            "DEVICE",
            "",
            &mut all_files,
            cancel_flag,
            &mut on_progress,
        )?;

        Ok(all_files)
    }
}

unsafe fn traverse_object<F>(
    content: &IPortableDeviceContent,
    properties: &IPortableDeviceProperties,
    parent_id: &str,
    current_rel_path: &str,
    collected: &mut Vec<DeviceFile>,
    cancel_flag: &AtomicBool,
    on_progress: &mut F,
) -> Result<(), String>
where
    F: FnMut(&str, Option<&DeviceFile>),
{
    if cancel_flag.load(Ordering::Relaxed) {
        return Err("Scan cancelled by user".to_string());
    }

    unsafe {
        let parent_id_u16: Vec<u16> = parent_id.encode_utf16().chain(std::iter::once(0)).collect();
        let enum_objects: IEnumPortableDeviceObjectIDs =
            match content.EnumObjects(0, PCWSTR(parent_id_u16.as_ptr()), None) {
                Ok(e) => e,
                Err(_) => return Ok(()), // Some objects cannot be enumerated, ignore
            };

        loop {
            if cancel_flag.load(Ordering::Relaxed) {
                return Err("Scan cancelled by user".to_string());
            }

            let mut object_ids: [PWSTR; 32] = [PWSTR::null(); 32];
            let mut fetched: u32 = 0;
            let hr = enum_objects.Next(&mut object_ids, &mut fetched);
            if hr.is_err() || fetched == 0 {
                break;
            }

            for &obj_pwstr in object_ids.iter().take(fetched as usize) {
                if cancel_flag.load(Ordering::Relaxed) {
                    return Err("Scan cancelled by user".to_string());
                }

                if obj_pwstr.is_null() {
                    continue;
                }
                let obj_id = obj_pwstr.to_string().unwrap_or_default();
                CoTaskMemFree(Some(obj_pwstr.as_ptr() as *const _));

                if obj_id.is_empty() {
                    continue;
                }

                let obj_u16: Vec<u16> = obj_id.encode_utf16().chain(std::iter::once(0)).collect();
                let pc_obj = PCWSTR(obj_u16.as_ptr());

                let values = match properties.GetValues(pc_obj, None) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Get content type
                let content_type = values
                    .GetGuidValue(&WPD_OBJECT_CONTENT_TYPE)
                    .unwrap_or_default();

                // Get name: try ORIGINAL_FILE_NAME first, then fallback to OBJECT_NAME
                let raw_name = match values.GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME) {
                    Ok(name_pwstr) => {
                        let s = name_pwstr.to_string().unwrap_or_default();
                        CoTaskMemFree(Some(name_pwstr.as_ptr() as *const _));
                        s
                    }
                    Err(_) => match values.GetStringValue(&WPD_OBJECT_NAME) {
                        Ok(name_pwstr) => {
                            let s = name_pwstr.to_string().unwrap_or_default();
                            CoTaskMemFree(Some(name_pwstr.as_ptr() as *const _));
                            s
                        }
                        Err(_) => String::new(),
                    },
                };

                let clean_name = clean_unicode_markers(&raw_name);
                if clean_name.is_empty() {
                    continue;
                }

                let is_folder = content_type == WPD_CONTENT_TYPE_FOLDER
                    || content_type == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT;

                if is_folder {
                    // Do not prepend the device functional/storage root (e.g. "Internal Storage",
                    // localized names like "Almacenamiento interno", or top-level containers under "DEVICE")
                    let is_storage_root = parent_id == "DEVICE"
                        || content_type == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT
                        || clean_name.eq_ignore_ascii_case("Internal Storage");

                    let next_rel_path = if is_storage_root {
                        current_rel_path.to_string()
                    } else if current_rel_path.is_empty() {
                        clean_name.clone()
                    } else {
                        format!("{}/{}", current_rel_path, clean_name)
                    };

                    if !next_rel_path.is_empty() {
                        on_progress(&next_rel_path, None);
                    }

                    traverse_object(
                        content,
                        properties,
                        &obj_id,
                        &next_rel_path,
                        collected,
                        cancel_flag,
                        on_progress,
                    )?;
                } else {
                    // It is a file
                    let file_size: u64 = values
                        .GetUnsignedLargeIntegerValue(&WPD_OBJECT_SIZE)
                        .unwrap_or(0);
                    let rel_path = if current_rel_path.is_empty() {
                        clean_name.clone()
                    } else {
                        format!("{}/{}", current_rel_path, clean_name)
                    };

                    let item = DeviceFile {
                        object_id: obj_id,
                        relative_path: rel_path.clone(),
                        filename: clean_name,
                        size: file_size,
                    };
                    on_progress(&rel_path, Some(&item));
                    collected.push(item);
                }
            }

            if hr == windows::core::HRESULT(1) {
                // S_FALSE indicates end of enumeration
                break;
            }
        }

        Ok(())
    }
}

pub fn stream_file_to_disk<F>(
    device: &IPortableDevice,
    object_id: &str,
    dest_path: &Path,
    expected_size: u64,
    abort_flag: &AtomicBool,
    mut progress_cb: F,
) -> Result<(), String>
where
    F: FnMut(u64, u64, f64) -> Result<(), String>,
{
    use std::fs::File;
    use std::io::Write;

    unsafe {
        let content = device
            .Content()
            .map_err(|e| format!("Failed to get device content: {}", e))?;
        let resources = content
            .Transfer()
            .map_err(|e| format!("Failed to get IPortableDeviceResources: {}", e))?;

        let obj_u16: Vec<u16> = object_id.encode_utf16().chain(std::iter::once(0)).collect();
        let mut optimal_buffer_size: u32 = 0;
        let mut stream: Option<IStream> = None;

        resources
            .GetStream(
                PCWSTR(obj_u16.as_ptr()),
                &WPD_RESOURCE_DEFAULT,
                0, // STGM_READ
                &mut optimal_buffer_size,
                &mut stream,
            )
            .map_err(|e| format!("Failed to get file stream for {}: {}", object_id, e))?;

        let stream = stream.ok_or_else(|| "GetStream returned null stream".to_string())?;

        // Create parent folder if not present
        if let Some(parent) = dest_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {:?}: {}", parent, e))?;
        }

        // Write to temporary .part file first to protect against incomplete files
        let part_path = get_part_path(dest_path);

        let mut file = File::create(&part_path)
            .map_err(|e| format!("Failed to create temporary file {:?}: {}", part_path, e))?;

        let buf_size = (optimal_buffer_size as usize).clamp(64 * 1024, 256 * 1024);
        let mut buffer = vec![0u8; buf_size];

        let mut bytes_copied: u64 = 0;
        let start_time = Instant::now();
        let mut last_progress_time = Instant::now();

        loop {
            if abort_flag.load(Ordering::Relaxed) {
                drop(file);
                let _ = std::fs::remove_file(&part_path);
                return Err("Aborted by user".to_string());
            }

            let mut bytes_read: u32 = 0;
            let hr = stream.Read(
                buffer.as_mut_ptr() as *mut _,
                buf_size as u32,
                Some(&mut bytes_read),
            );

            if hr.is_err() {
                drop(file);
                let _ = std::fs::remove_file(&part_path);
                return Err(format!("Stream read error: {:?}", hr));
            }

            if bytes_read == 0 {
                break;
            }

            file.write_all(&buffer[..bytes_read as usize])
                .map_err(|e| format!("Disk write error: {}", e))?;

            bytes_copied += bytes_read as u64;

            let now = Instant::now();
            let elapsed_since_last = now.duration_since(last_progress_time).as_secs_f64();
            if elapsed_since_last >= 0.1 || (expected_size > 0 && bytes_copied == expected_size) {
                let total_elapsed = now.duration_since(start_time).as_secs_f64();
                let speed_bps = if total_elapsed > 0.0 {
                    bytes_copied as f64 / total_elapsed
                } else {
                    0.0
                };
                last_progress_time = now;
                progress_cb(bytes_copied, expected_size, speed_bps)?;
            }
        }

        file.flush()
            .map_err(|e| format!("Failed to flush file: {}", e))?;
        drop(file);

        // Crucial validation: ensure complete file transfer!
        if expected_size > 0 && bytes_copied != expected_size {
            let _ = std::fs::remove_file(&part_path);
            return Err(format!(
                "Incomplete transfer: received {} of {} bytes (stream terminated prematurely)",
                bytes_copied, expected_size
            ));
        }

        // Rename .part to final destination
        if dest_path.exists() {
            let _ = std::fs::remove_file(dest_path);
        }
        std::fs::rename(&part_path, dest_path)
            .map_err(|e| format!("Failed to rename {:?} to {:?}: {}", part_path, dest_path, e))?;

        Ok(())
    }
}

/// Look up an object's current WPD object ID by its relative path (e.g. "DCIM/100APPLE/IMG_1234.JPG").
/// This acts as a reliable fallback if cached object IDs become stale between scan and transfer sessions.
pub fn find_object_id_by_path(
    device: &IPortableDevice,
    rel_path: &str,
) -> Result<Option<String>, String> {
    let clean_rel = clean_unicode_markers(rel_path).replace('\\', "/");
    let parts: Vec<&str> = clean_rel
        .split('/')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if parts.is_empty() {
        return Ok(None);
    }

    unsafe {
        let content = device
            .Content()
            .map_err(|e| format!("Failed to get device content: {}", e))?;
        let properties = content
            .Properties()
            .map_err(|e| format!("Failed to get content properties: {}", e))?;

        // 1. Locate the storage container under "DEVICE"
        let parent_id_u16: Vec<u16> = "DEVICE".encode_utf16().chain(std::iter::once(0)).collect();
        let enum_objects: IEnumPortableDeviceObjectIDs = content
            .EnumObjects(0, PCWSTR(parent_id_u16.as_ptr()), None)
            .map_err(|e| format!("Failed to enumerate DEVICE objects: {}", e))?;

        let mut storage_id: Option<String> = None;
        let mut object_ids: [PWSTR; 16] = [PWSTR::null(); 16];
        let mut fetched: u32 = 0;
        let hr = enum_objects.Next(&mut object_ids, &mut fetched);
        if hr.is_ok() && fetched > 0 {
            for &obj_pwstr in object_ids.iter().take(fetched as usize) {
                if !obj_pwstr.is_null() {
                    let id_str = obj_pwstr.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(obj_pwstr.as_ptr() as *const _));
                    if storage_id.is_none() && !id_str.is_empty() {
                        storage_id = Some(id_str);
                    }
                }
            }
        }

        let storage_id = match storage_id {
            Some(id) => id,
            None => return Ok(None),
        };

        // 2. Traverse down through path segments (e.g. "DCIM" -> "100APPLE" -> "IMG_1234.JPG")
        let mut current_parent = storage_id;
        for (i, &segment) in parts.iter().enumerate() {
            let is_last = i == parts.len() - 1;
            let current_parent_u16: Vec<u16> = current_parent
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();

            let enum_sub: IEnumPortableDeviceObjectIDs =
                match content.EnumObjects(0, PCWSTR(current_parent_u16.as_ptr()), None) {
                    Ok(e) => e,
                    Err(_) => return Ok(None),
                };

            let mut matched_child: Option<(String, bool)> = None;

            loop {
                let mut sub_ids: [PWSTR; 32] = [PWSTR::null(); 32];
                let mut sub_fetched: u32 = 0;
                let sub_hr = enum_sub.Next(&mut sub_ids, &mut sub_fetched);
                if sub_hr.is_err() || sub_fetched == 0 {
                    break;
                }

                for &child_pwstr in sub_ids.iter().take(sub_fetched as usize) {
                    if child_pwstr.is_null() {
                        continue;
                    }
                    let child_id = child_pwstr.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(child_pwstr.as_ptr() as *const _));

                    if child_id.is_empty() || matched_child.is_some() {
                        continue;
                    }

                    let child_u16: Vec<u16> =
                        child_id.encode_utf16().chain(std::iter::once(0)).collect();
                    let pc_child = PCWSTR(child_u16.as_ptr());

                    if let Ok(values) = properties.GetValues(pc_child, None) {
                        let raw_name = match values.GetStringValue(&WPD_OBJECT_ORIGINAL_FILE_NAME) {
                            Ok(name_pwstr) => {
                                let s = name_pwstr.to_string().unwrap_or_default();
                                CoTaskMemFree(Some(name_pwstr.as_ptr() as *const _));
                                s
                            }
                            Err(_) => match values.GetStringValue(&WPD_OBJECT_NAME) {
                                Ok(name_pwstr) => {
                                    let s = name_pwstr.to_string().unwrap_or_default();
                                    CoTaskMemFree(Some(name_pwstr.as_ptr() as *const _));
                                    s
                                }
                                Err(_) => String::new(),
                            },
                        };

                        let clean_child_name = clean_unicode_markers(&raw_name);
                        if clean_child_name.eq_ignore_ascii_case(segment) {
                            let content_type = values
                                .GetGuidValue(&WPD_OBJECT_CONTENT_TYPE)
                                .unwrap_or_default();
                            let is_folder = content_type == WPD_CONTENT_TYPE_FOLDER
                                || content_type == WPD_CONTENT_TYPE_FUNCTIONAL_OBJECT;
                            matched_child = Some((child_id, is_folder));
                        }
                    }
                }

                if matched_child.is_some() || sub_hr == windows::core::HRESULT(1) {
                    break;
                }
            }

            match matched_child {
                Some((child_id, is_folder)) => {
                    if is_last {
                        return Ok(Some(child_id));
                    }
                    if !is_folder {
                        return Ok(None);
                    }
                    current_parent = child_id;
                }
                None => return Ok(None),
            }
        }

        Ok(None)
    }
}

unsafe fn read_string_property<F>(mut fetch: F) -> String
where
    F: FnMut(PWSTR, *mut u32) -> windows::core::Result<()>,
{
    let mut len: u32 = 0;
    if fetch(PWSTR::null(), &mut len).is_ok() && len > 0 {
        let mut buf = vec![0u16; len as usize];
        if fetch(PWSTR(buf.as_mut_ptr()), &mut len).is_ok() {
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            return String::from_utf16_lossy(&buf[..end]);
        }
    }
    String::new()
}
