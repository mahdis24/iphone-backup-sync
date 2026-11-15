<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import type {
  DeviceInfo,
  ScannedFile,
  ScanProgressPayload,
  ScanResult,
  ScanSummary,
  SyncProgressPayload,
} from './types';

// App state & persistence
const STORAGE_KEY_BACKUP_PATH = 'iphone_backup_sync_destination';
const savedPath = typeof window !== 'undefined' ? localStorage.getItem(STORAGE_KEY_BACKUP_PATH) : null;
const backupPath = ref<string>(savedPath || 'C:\\iPhoneBackup');

watch(backupPath, (val) => {
  if (typeof window !== 'undefined' && val) {
    localStorage.setItem(STORAGE_KEY_BACKUP_PATH, val);
  }
});

const devices = ref<DeviceInfo[]>([]);
const selectedDevice = ref<DeviceInfo | null>(null);
const isRefreshingDevices = ref(false);

// Scan & File state
const isScanning = ref(false);
const scanProgress = ref<ScanProgressPayload | null>(null);
const scannedFiles = ref<ScannedFile[]>([]);
const skipList = ref<string[]>([]);
const summary = ref<ScanSummary>({
  total_iphone_files: 0,
  total_backup_files: 0,
  new_count: 0,
  mismatch_count: 0,
  verified_ignored_count: 0,
  skipped_count: 0,
  error_count: 0,
});

// Sync state
const isSyncing = ref(false);
const currentProgress = ref<SyncProgressPayload | null>(null);
const syncError = ref<string | null>(null);
const syncNotice = ref<string | null>(null);

// UI Tabs & Filters
type TabType = 'pending' | 'new' | 'mismatch' | 'errors' | 'skiplist' | 'all';
const activeTab = ref<TabType>('pending');
const searchQuery = ref('');

// Custom Context Menu state
const contextMenuVisible = ref(false);
const contextMenuX = ref(0);
const contextMenuY = ref(0);
const contextMenuFile = ref<ScannedFile | null>(null);

let unlistenProgress: UnlistenFn | null = null;
let unlistenScan: UnlistenFn | null = null;

// Load connected devices
async function refreshDevices() {
  isRefreshingDevices.value = true;
  try {
    const list = await invoke<DeviceInfo[]>('get_devices');
    devices.value = list;

    // Prefer iPhone if found
    const iphone = list.find((d) => d.is_iphone);
    if (iphone) {
      selectedDevice.value = iphone;
    } else if (list.length > 0) {
      selectedDevice.value = list[0];
    } else {
      selectedDevice.value = null;
    }
  } catch (err: unknown) {
    console.error('Failed to get devices:', err);
  } finally {
    isRefreshingDevices.value = false;
  }
}

function getErrorMessage(err: unknown, fallback = 'Operation failed'): string {
  if (typeof err === 'string') return err;
  if (err instanceof Error) return err.message;
  if (err && typeof err === 'object' && 'message' in err && typeof (err as { message: unknown }).message === 'string') {
    return (err as { message: string }).message;
  }
  return fallback;
}

// Select backup directory
async function browseBackupDir() {
  try {
    const selected = await open({
      directory: true,
      multiple: false,
      defaultPath: backupPath.value,
      title: 'Select Local iPhone Backup Directory',
    });
    if (selected && typeof selected === 'string') {
      backupPath.value = selected;
    }
  } catch (err: unknown) {
    console.error('Directory browse failed:', err);
  }
}

// Fetch skip list
async function refreshSkipList() {
  try {
    const list = await invoke<string[]>('get_skip_list');
    skipList.value = list;
  } catch (err: unknown) {
    console.error('Failed to load skip list:', err);
  }
}

// Add to skip list
async function addToSkipList(path: string) {
  try {
    const updated = await invoke<string[]>('add_to_skip_list', { relativePath: path });
    skipList.value = updated;
    const file = scannedFiles.value.find((f) => f.relative_path === path);
    if (file) {
      file.status = 'SKIPPED';
      file.error = undefined;
      updateSummaryCounts();
    }
    syncNotice.value = `File "${path.split('/').pop()}" added to skip list.`;
  } catch (err: unknown) {
    console.error('Failed to add to skip list:', err);
  }
}

// Remove from skip list
async function removeFromSkipList(path: string) {
  try {
    const updated = await invoke<string[]>('remove_from_skip_list', { relativePath: path });
    skipList.value = updated;
    // Update local state if file is in scanned list
    const file = scannedFiles.value.find((f) => f.relative_path === path);
    if (file && file.status === 'SKIPPED') {
      file.status = file.existing_size !== undefined ? 'MISMATCH' : 'NEW';
      file.error = undefined;
      updateSummaryCounts();
    }
    syncNotice.value = `File "${path.split('/').pop()}" removed from skip list.`;
  } catch (err: unknown) {
    console.error('Failed to remove from skip list:', err);
  }
}

// Clear all skip list
async function clearAllSkipList() {
  if (!confirm('Are you sure you want to clear the entire skip list?')) return;
  try {
    await invoke<string[]>('clear_skip_list');
    skipList.value = [];
    // Reset any skipped files in scan
    for (const f of scannedFiles.value) {
      if (f.status === 'SKIPPED') {
        f.status = f.existing_size !== undefined ? 'MISMATCH' : 'NEW';
      }
    }
    updateSummaryCounts();
    syncNotice.value = 'Skip list cleared.';
  } catch (err) {
    console.error('Failed to clear skip list:', err);
  }
}

// Computed titles for scanning
const scanPhaseTitle = computed(() => {
  if (!scanProgress.value) return 'Connecting to iPhone';
  switch (scanProgress.value.stage) {
    case 'connecting':
      return 'Opening iPhone Session';
    case 'enumerating':
      return 'Scanning iPhone Media Storage';
    case 'indexing':
      return 'Indexing Local Backup Directory';
    case 'classifying':
      return 'Comparing iPhone Files with Backup';
    case 'completed':
      return 'Scan Complete';
    default:
      return 'Scanning iPhone';
  }
});

const scanBadgeText = computed(() => {
  if (!scanProgress.value) return '';
  if (scanProgress.value.stage === 'enumerating') {
    return `${scanProgress.value.current.toLocaleString()} files found`;
  }
  if (scanProgress.value.stage === 'indexing') {
    return `${scanProgress.value.current.toLocaleString()} files indexed`;
  }
  if (scanProgress.value.stage === 'classifying' && scanProgress.value.total > 0) {
    return `${scanProgress.value.current.toLocaleString()} / ${scanProgress.value.total.toLocaleString()} (${scanProgress.value.percent}%)`;
  }
  if (scanProgress.value.current > 0) {
    return `${scanProgress.value.current.toLocaleString()} items`;
  }
  return '';
});

// Start device scan
async function scanDevice() {
  if (!selectedDevice.value) {
    alert('Please connect your iPhone and click Refresh Devices first.');
    return;
  }
  if (!backupPath.value.trim()) {
    alert('Please specify a backup directory.');
    return;
  }

  isScanning.value = true;
  scanProgress.value = {
    stage: 'connecting',
    message: 'Opening session with iPhone...',
    current: 0,
    total: 0,
    percent: 5,
  };
  syncNotice.value = null;
  syncError.value = null;

  try {
    const result = await invoke<ScanResult>('scan_device', {
      deviceId: selectedDevice.value.id,
      backupDir: backupPath.value,
    });
    scannedFiles.value = result.files;
    summary.value = {
      ...result.summary,
      error_count: 0,
    };
    skipList.value = result.skip_list;
    syncNotice.value = `Scan complete: ${result.files.length.toLocaleString()} files discovered (${result.summary.new_count} new, ${result.summary.mismatch_count} size mismatch).`;
  } catch (err: unknown) {
    const msg = getErrorMessage(err, 'Scan failed');
    if (msg.toLowerCase().includes('cancel')) {
      syncNotice.value = 'Scan cancelled by user.';
    } else {
      syncError.value = msg;
    }
  } finally {
    isScanning.value = false;
    scanProgress.value = null;
  }
}

// Cancel ongoing device scan
async function cancelScan() {
  if (!isScanning.value) return;
  try {
    await invoke('cancel_scan');
    syncNotice.value = 'Cancelling scan...';
  } catch (err: unknown) {
    console.error('Failed to cancel scan:', err);
  } finally {
    isScanning.value = false;
    scanProgress.value = null;
  }
}

// Start sync of all pending files
async function startSync() {
  if (!selectedDevice.value) {
    alert('No device selected');
    return;
  }
  if (isSyncing.value) return;

  const pending = scannedFiles.value.filter(
    (f) => f.status === 'NEW' || f.status === 'MISMATCH' || f.status === 'ERROR'
  );
  if (pending.length === 0) {
    alert('No pending files to sync! All files are already backed up or skipped.');
    return;
  }

  isSyncing.value = true;
  syncError.value = null;
  syncNotice.value = `Starting sync of ${pending.length} files...`;

  try {
    await invoke('start_sync', {
      deviceId: selectedDevice.value.id,
      backupDir: backupPath.value,
      files: scannedFiles.value,
    });
  } catch (err: unknown) {
    syncError.value = getErrorMessage(err, 'Sync failed to start');
    isSyncing.value = false;
    syncNotice.value = null;
  }
}

// Sync single file from row context menu
async function syncThisFile(file: ScannedFile) {
  closeContextMenu();
  if (isSyncing.value) {
    alert('A synchronization is currently active. Please wait or stop the current sync first.');
    return;
  }
  if (!selectedDevice.value) {
    alert('No device selected');
    return;
  }

  // Set file status to NEW or keep MISMATCH so backend transfers it
  const singleFile: ScannedFile = {
    ...file,
    status: file.status === 'VERIFIED_IGNORED' || file.status === 'SKIPPED' ? 'NEW' : file.status,
  };

  isSyncing.value = true;
  syncError.value = null;
  syncNotice.value = `Syncing "${file.filename}"...`;

  try {
    await invoke('start_sync', {
      deviceId: selectedDevice.value.id,
      backupDir: backupPath.value,
      files: [singleFile],
    });
  } catch (err: unknown) {
    syncError.value = getErrorMessage(err, 'Failed to start file sync');
    isSyncing.value = false;
    syncNotice.value = null;
  }
}

// Toggle skip status for row context menu
async function toggleSkipForFile(file: ScannedFile) {
  closeContextMenu();
  if (skipList.value.includes(file.relative_path) || file.status === 'SKIPPED') {
    await removeFromSkipList(file.relative_path);
  } else {
    await addToSkipList(file.relative_path);
  }
}

// Context menu open/close
function openRowContextMenu(e: MouseEvent, file: ScannedFile) {
  contextMenuFile.value = file;
  const menuWidth = 190;
  const menuHeight = 110;
  const x = Math.min(e.clientX, window.innerWidth - menuWidth - 10);
  const y = Math.min(e.clientY, window.innerHeight - menuHeight - 10);
  contextMenuX.value = Math.max(10, x);
  contextMenuY.value = Math.max(10, y);
  contextMenuVisible.value = true;
}

function closeContextMenu() {
  contextMenuVisible.value = false;
  contextMenuFile.value = null;
}

// Skip current file
async function skipCurrentFile() {
  if (!isSyncing.value) return;
  try {
    await invoke('skip_current_file');
    syncNotice.value = 'Skipping current file...';
  } catch (err: unknown) {
    console.error('Failed to skip current file:', err);
  }
}

// Stop whole sync
async function cancelSync() {
  if (!isSyncing.value) return;
  syncNotice.value = 'Stopping synchronization...';
  try {
    await invoke('cancel_sync');
  } catch (err: unknown) {
    console.error('Failed to cancel sync:', err);
    isSyncing.value = false;
  }
}

// Helper to recalculate summary numbers
function updateSummaryCounts() {
  let news = 0;
  let mismatches = 0;
  let verified = 0;
  let skipped = 0;
  let errors = 0;
  for (const f of scannedFiles.value) {
    if (f.status === 'NEW') news++;
    else if (f.status === 'MISMATCH') mismatches++;
    else if (f.status === 'VERIFIED_IGNORED') verified++;
    else if (f.status === 'SKIPPED') skipped++;
    else if (f.status === 'ERROR') errors++;
  }
  summary.value.new_count = news;
  summary.value.mismatch_count = mismatches;
  summary.value.verified_ignored_count = verified;
  summary.value.skipped_count = skipped;
  summary.value.error_count = errors;
}

// Formatter utilities
function formatBytes(bytes: number, decimals = 1): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const dm = decimals < 0 ? 0 : decimals;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(dm)) + ' ' + sizes[i];
}

function formatSpeed(bytesPerSec: number): string {
  if (bytesPerSec <= 0) return '0 KB/s';
  return `${formatBytes(bytesPerSec, 1)}/s`;
}

// Filtered file list based on active tab and search query
const filteredFiles = computed(() => {
  let list = scannedFiles.value;

  if (activeTab.value === 'pending') {
    list = list.filter((f) => f.status === 'NEW' || f.status === 'MISMATCH' || f.status === 'ERROR');
  } else if (activeTab.value === 'new') {
    list = list.filter((f) => f.status === 'NEW');
  } else if (activeTab.value === 'mismatch') {
    list = list.filter((f) => f.status === 'MISMATCH');
  } else if (activeTab.value === 'errors') {
    list = list.filter((f) => f.status === 'ERROR');
  } else if (activeTab.value === 'skiplist') {
    list = list.filter((f) => f.status === 'SKIPPED');
  }

  if (searchQuery.value.trim()) {
    const q = searchQuery.value.toLowerCase().trim();
    list = list.filter(
      (f) =>
        f.filename.toLowerCase().includes(q) ||
        f.relative_path.toLowerCase().includes(q) ||
        f.target_relative_path.toLowerCase().includes(q)
    );
  }

  return list;
});

// File progress percentages
const currentFilePercent = computed(() => {
  if (!currentProgress.value || currentProgress.value.total_bytes === 0) return 0;
  return Math.min(
    100,
    Math.round((currentProgress.value.bytes_copied / currentProgress.value.total_bytes) * 100)
  );
});

const overallBatchPercent = computed(() => {
  if (!currentProgress.value || currentProgress.value.total_files === 0) return 0;
  return Math.min(
    100,
    Math.round(
      ((currentProgress.value.current_file_index - 1) / currentProgress.value.total_files) * 100 +
        currentFilePercent.value / currentProgress.value.total_files
    )
  );
});

// Keyboard shortcuts
function handleKeyDown(e: KeyboardEvent) {
  if (e.key === 'Escape') {
    closeContextMenu();
    return;
  }
  if (e.key.toLowerCase() === 's') {
    const target = e.target as HTMLElement;
    if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA')) {
      return;
    }
    if (isSyncing.value) {
      e.preventDefault();
      skipCurrentFile();
    }
  }
}

// Global context menu disabler across the entire application
function handleGlobalContextMenu(e: MouseEvent) {
  e.preventDefault();
}

onMounted(async () => {
  window.addEventListener('contextmenu', handleGlobalContextMenu);
  window.addEventListener('click', closeContextMenu);
  window.addEventListener('scroll', closeContextMenu, true);
  window.addEventListener('keydown', handleKeyDown);

  // Listen to scan progress events from Rust
  unlistenScan = await listen<ScanProgressPayload>('scan-progress', (event) => {
    scanProgress.value = event.payload;
  });

  // Listen to real-time sync progress events from Rust
  unlistenProgress = await listen<SyncProgressPayload>('sync-progress', (event) => {
    currentProgress.value = event.payload;

    if (event.payload.status === 'copying') {
      isSyncing.value = true;
    } else if (event.payload.status === 'completed') {
      const f = scannedFiles.value.find((file) => file.relative_path === event.payload.relative_path);
      if (f) {
        f.status = 'VERIFIED_IGNORED';
        f.error = undefined;
        updateSummaryCounts();
      }
    } else if (event.payload.status === 'skipped') {
      const f = scannedFiles.value.find((file) => file.relative_path === event.payload.relative_path);
      if (f) {
        f.status = 'SKIPPED';
        if (!skipList.value.includes(event.payload.relative_path)) {
          skipList.value.push(event.payload.relative_path);
        }
        updateSummaryCounts();
      }
    } else if (event.payload.status === 'error') {
      const f = scannedFiles.value.find((file) => file.relative_path === event.payload.relative_path);
      if (f) {
        f.status = 'ERROR';
        f.error = event.payload.error;
        updateSummaryCounts();
      }
      if (event.payload.error) {
        syncError.value = event.payload.error;
      }
    } else if (event.payload.status === 'aborted') {
      isSyncing.value = false;
      syncNotice.value = 'Sync stopped by user. Incomplete files were removed.';
    } else if (event.payload.status === 'finished') {
      isSyncing.value = false;
      const errorCount = scannedFiles.value.filter((f) => f.status === 'ERROR').length;
      if (errorCount > 0) {
        syncNotice.value = `Synchronization finished with ${errorCount} error(s). Right-click failed files to retry or skip.`;
      } else {
        syncNotice.value = 'Synchronization complete! All files backed up successfully.';
        syncError.value = null;
      }
    }
  });

  await refreshDevices();
  await refreshSkipList();
});

onUnmounted(() => {
  window.removeEventListener('contextmenu', handleGlobalContextMenu);
  window.removeEventListener('click', closeContextMenu);
  window.removeEventListener('scroll', closeContextMenu, true);
  window.removeEventListener('keydown', handleKeyDown);
  if (unlistenProgress) {
    unlistenProgress();
  }
  if (unlistenScan) {
    unlistenScan();
  }
});
</script>

<template>
  <div class="app-shell">
    <!-- Header -->
    <header class="topbar">
      <div class="brand">
        <div class="brand-icon">
          <svg viewBox="0 0 24 24" width="22" height="22" stroke="currentColor" stroke-width="2" fill="none">
            <rect x="5" y="2" width="14" height="20" rx="3" ry="3"></rect>
            <line x1="12" y1="18" x2="12" y2="18.01"></line>
          </svg>
        </div>
        <div>
          <h1 class="brand-title">iPhone Backup Sync</h1>
          <p class="brand-sub">WPD / MTP Direct Media Synchronizer</p>
        </div>
      </div>

      <!-- Device Connection Badge -->
      <div class="device-panel">
        <div class="device-badge" :class="{ connected: selectedDevice, searching: isRefreshingDevices }">
          <span class="status-indicator"></span>
          <span v-if="selectedDevice" class="device-name">
            {{ selectedDevice.name }}
          </span>
          <span v-else-if="isRefreshingDevices" class="device-name">
            Scanning for iPhone...
          </span>
          <span v-else class="device-name not-connected">
            No Device Detected
          </span>
        </div>

        <button
          class="btn btn-secondary btn-icon"
          title="Refresh Connected Devices"
          :disabled="isRefreshingDevices || isSyncing"
          @click="refreshDevices"
        >
          <svg
            :class="{ spin: isRefreshingDevices }"
            viewBox="0 0 24 24"
            width="16"
            height="16"
            stroke="currentColor"
            stroke-width="2"
            fill="none"
          >
            <path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67" />
          </svg>
          <span>Refresh</span>
        </button>
      </div>
    </header>

    <!-- Main Content Area -->
    <main class="main-container">
      <!-- Folder & Primary Actions Bar -->
      <section class="control-card">
        <div class="folder-row">
          <div class="input-group">
            <label class="input-label">
              <svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="2" fill="none">
                <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
              </svg>
              <span>Backup Directory:</span>
            </label>
            <div class="input-with-button">
              <input
                v-model="backupPath"
                type="text"
                class="text-input"
                placeholder="C:\iPhoneBackup (Click Browse to select...)"
                :disabled="isSyncing || isScanning"
              />
              <button
                class="btn btn-secondary"
                :disabled="isSyncing || isScanning"
                @click="browseBackupDir"
              >
                Browse...
              </button>
            </div>
          </div>

          <div class="actions-group">
            <!-- Scan Button -->
            <button
              class="btn btn-primary"
              :disabled="!selectedDevice || isScanning || isSyncing"
              @click="scanDevice"
            >
              <svg
                v-if="!isScanning"
                viewBox="0 0 24 24"
                width="16"
                height="16"
                stroke="currentColor"
                stroke-width="2"
                fill="none"
              >
                <circle cx="11" cy="11" r="8"></circle>
                <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
              </svg>
              <span v-else class="spinner-small"></span>
              <span>{{ isScanning ? (scanProgress && scanProgress.current > 0 ? `Scanning (${scanProgress.current.toLocaleString()})...` : 'Scanning...') : 'Scan iPhone' }}</span>
            </button>

            <!-- Cancel Scan Button -->
            <button
              v-if="isScanning"
              class="btn btn-danger"
              title="Cancel the ongoing scan"
              @click="cancelScan"
            >
              <svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="2" fill="none">
                <circle cx="12" cy="12" r="10"></circle>
                <line x1="15" y1="9" x2="9" y2="15"></line>
                <line x1="9" y1="9" x2="15" y2="15"></line>
              </svg>
              <span>Cancel Scan</span>
            </button>

            <!-- Sync Button -->
            <button
              class="btn btn-success"
              :disabled="isSyncing || isScanning || summary.new_count + summary.mismatch_count + (summary.error_count || 0) === 0"
              @click="startSync"
            >
              <svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="2" fill="none">
                <polyline points="23 4 23 10 17 10"></polyline>
                <polyline points="1 20 1 14 7 14"></polyline>
                <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
              </svg>
              <span>{{ isSyncing ? 'Syncing...' : 'Start Sync' }}</span>
            </button>

            <!-- Skip Current File Button -->
            <button
              v-if="isSyncing"
              class="btn btn-warning"
              title="Skip this file and add to skip list (Shortcut: S)"
              @click="skipCurrentFile"
            >
              <svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="2" fill="none">
                <polygon points="5 4 15 12 5 20 5 4"></polygon>
                <line x1="19" y1="5" x2="19" y2="19"></line>
              </svg>
              <span>Skip Current</span>
              <kbd class="kbd-badge">S</kbd>
            </button>

            <!-- Stop Sync Button -->
            <button
              v-if="isSyncing"
              class="btn btn-danger"
              title="Stop synchronization immediately"
              @click="cancelSync"
            >
              <svg viewBox="0 0 24 24" width="16" height="16" stroke="currentColor" stroke-width="2" fill="none">
                <rect x="6" y="6" width="12" height="12" rx="2" fill="currentColor"></rect>
              </svg>
              <span>Stop Sync</span>
            </button>
          </div>
        </div>
      </section>

      <!-- Dedicated Scan Progress Card -->
      <section v-if="isScanning" class="scan-progress-card">
        <div class="scan-progress-header">
          <div class="scan-progress-title-block">
            <span class="scan-pulse-ring"></span>
            <div>
              <div class="scan-phase-title">{{ scanPhaseTitle }}</div>
              <div class="scan-phase-sub">
                <span class="scan-live-message">{{ scanProgress?.message || 'Communicating with iPhone...' }}</span>
              </div>
            </div>
          </div>
          <div class="scan-header-actions">
            <div v-if="scanBadgeText" class="scan-badge">
              {{ scanBadgeText }}
            </div>
            <button
              class="btn btn-secondary btn-xs"
              title="Cancel Scan"
              @click="cancelScan"
            >
              Cancel
            </button>
          </div>
        </div>
        <div class="scan-track">
          <div
            class="scan-fill-real"
            :style="{ width: `${scanProgress?.percent || 5}%` }"
          ></div>
        </div>
        <div class="scan-stats-footer">
          <span class="scan-footer-left">
            <span v-if="scanProgress?.stage === 'connecting'">Opening MTP session... Keep iPhone unlocked.</span>
            <span v-else-if="scanProgress?.stage === 'enumerating'">Enumerating camera roll and media folders...</span>
            <span v-else-if="scanProgress?.stage === 'indexing'">Indexing files in local backup folder...</span>
            <span v-else-if="scanProgress?.stage === 'classifying'">Comparing iPhone files against backup...</span>
            <span v-else>Scanning storage...</span>
          </span>
          <span class="scan-footer-right font-mono">{{ scanProgress?.percent || 5 }}%</span>
        </div>
      </section>

      <!-- Feedback Banners -->
      <div v-if="syncError" class="alert alert-error">
        <svg viewBox="0 0 24 24" width="18" height="18" stroke="currentColor" stroke-width="2" fill="none">
          <circle cx="12" cy="12" r="10"></circle>
          <line x1="12" y1="8" x2="12" y2="12"></line>
          <line x1="12" y1="16" x2="12.01" y2="16"></line>
        </svg>
        <span>{{ syncError }}</span>
      </div>

      <div v-if="syncNotice" class="alert alert-info">
        <svg viewBox="0 0 24 24" width="18" height="18" stroke="currentColor" stroke-width="2" fill="none">
          <circle cx="12" cy="12" r="10"></circle>
          <line x1="12" y1="16" x2="12" y2="12"></line>
          <line x1="12" y1="8" x2="12.01" y2="8"></line>
        </svg>
        <span>{{ syncNotice }}</span>
      </div>

      <!-- Real-Time Sync Progress Card -->
      <section v-if="currentProgress && (isSyncing || currentProgress.status === 'completed')" class="progress-card">
        <div class="progress-header">
          <div class="progress-title-block">
            <span class="pulse-indicator" :class="currentProgress.status"></span>
            <span class="file-name-label">
              {{ currentProgress.filename || 'Processing file...' }}
            </span>
            <span class="file-path-sub">
              {{ currentProgress.relative_path }}
            </span>
          </div>

          <div class="progress-stats">
            <span class="speed-pill">
              {{ formatSpeed(currentProgress.speed_bytes_sec) }}
            </span>
            <span class="counter-pill">
              {{ currentProgress.current_file_index }} / {{ currentProgress.total_files }} files
            </span>
          </div>
        </div>

        <!-- Current File Progress -->
        <div class="progress-bar-wrapper">
          <div class="bar-info">
            <span>Current File: {{ formatBytes(currentProgress.bytes_copied) }} / {{ formatBytes(currentProgress.total_bytes) }}</span>
            <span class="percent-label">{{ currentFilePercent }}%</span>
          </div>
          <div class="progress-track">
            <div
              class="progress-fill current-file"
              :style="{ width: `${currentFilePercent}%` }"
            ></div>
          </div>
        </div>

        <!-- Batch Progress -->
        <div class="progress-bar-wrapper">
          <div class="bar-info">
            <span>Overall Batch Progress</span>
            <span class="percent-label">{{ overallBatchPercent }}%</span>
          </div>
          <div class="progress-track">
            <div
              class="progress-fill batch"
              :style="{ width: `${overallBatchPercent}%` }"
            ></div>
          </div>
        </div>
      </section>

      <!-- Summary Metrics Bar (Clickable Section Filters) -->
      <section class="metrics-grid">
        <div
          class="metric-card clickable"
          :class="{ active: activeTab === 'all' }"
          title="Filter: All iPhone Files"
          @click="activeTab = 'all'"
        >
          <div class="metric-icon blue">
            <svg viewBox="0 0 24 24" width="20" height="20" stroke="currentColor" stroke-width="2" fill="none">
              <rect x="5" y="2" width="14" height="20" rx="2" ry="2"></rect>
              <line x1="12" y1="18" x2="12" y2="18.01"></line>
            </svg>
          </div>
          <div class="metric-content">
            <div class="metric-val">{{ summary.total_iphone_files }}</div>
            <div class="metric-lbl">Total on iPhone</div>
          </div>
        </div>

        <div
          class="metric-card clickable"
          :class="{ active: activeTab === 'all' }"
          title="Filter: Total in Backup"
          @click="activeTab = 'all'"
        >
          <div class="metric-icon purple">
            <svg viewBox="0 0 24 24" width="20" height="20" stroke="currentColor" stroke-width="2" fill="none">
              <ellipse cx="12" cy="5" rx="9" ry="3"></ellipse>
              <path d="M21 12c0 1.66-4 3-9 3s-9-1.34-9-3"></path>
              <path d="M3 5v14c0 1.66 4 3 9 3s9-1.34 9-3V5"></path>
            </svg>
          </div>
          <div class="metric-content">
            <div class="metric-val">{{ summary.total_backup_files }}</div>
            <div class="metric-lbl">Total in Backup</div>
          </div>
        </div>

        <div
          class="metric-card new-card clickable"
          :class="{ highlight: summary.new_count > 0, active: activeTab === 'new' }"
          title="Filter: NEW Files"
          @click="activeTab = 'new'"
        >
          <div class="metric-icon green">
            <svg viewBox="0 0 24 24" width="20" height="20" stroke="currentColor" stroke-width="2" fill="none">
              <path d="M12 5v14M5 12h14"></path>
            </svg>
          </div>
          <div class="metric-content">
            <div class="metric-val text-green">{{ summary.new_count }}</div>
            <div class="metric-lbl">NEW Files</div>
          </div>
        </div>

        <div
          class="metric-card mismatch-card clickable"
          :class="{ highlight: summary.mismatch_count > 0, active: activeTab === 'mismatch' }"
          title="Filter: Size Mismatch Files"
          @click="activeTab = 'mismatch'"
        >
          <div class="metric-icon amber">
            <svg viewBox="0 0 24 24" width="20" height="20" stroke="currentColor" stroke-width="2" fill="none">
              <path d="M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"></path>
              <line x1="12" y1="9" x2="12" y2="13"></line>
              <line x1="12" y1="17" x2="12.01" y2="17"></line>
            </svg>
          </div>
          <div class="metric-content">
            <div class="metric-val text-amber">{{ summary.mismatch_count }}</div>
            <div class="metric-lbl">Size Mismatch (_new)</div>
          </div>
        </div>

        <div
          class="metric-card clickable"
          :class="{ active: activeTab === 'all' }"
          title="Filter: Verified Backed-up Files"
          @click="activeTab = 'all'"
        >
          <div class="metric-icon cyan">
            <svg viewBox="0 0 24 24" width="20" height="20" stroke="currentColor" stroke-width="2" fill="none">
              <polyline points="20 6 9 17 4 12"></polyline>
            </svg>
          </div>
          <div class="metric-content">
            <div class="metric-val text-cyan">{{ summary.verified_ignored_count }}</div>
            <div class="metric-lbl">Verified _new Ignored</div>
          </div>
        </div>

        <div
          class="metric-card rose clickable"
          :class="{ active: activeTab === 'skiplist' }"
          title="Filter: Blacklisted Files"
          @click="activeTab = 'skiplist'"
        >
          <div class="metric-icon rose">
            <svg viewBox="0 0 24 24" width="20" height="20" stroke="currentColor" stroke-width="2" fill="none">
              <circle cx="12" cy="12" r="10"></circle>
              <line x1="4.93" y1="4.93" x2="19.07" y2="19.07"></line>
            </svg>
          </div>
          <div class="metric-content">
            <div class="metric-val text-rose">{{ summary.skipped_count }}</div>
            <div class="metric-lbl">Blacklisted (Skip List)</div>
          </div>
        </div>
      </section>

      <!-- Interactive Tabs & File Table -->
      <section class="table-section">
        <!-- Tabs and Filter Toolbar -->
        <div class="tab-toolbar">
          <div class="tabs-list">
            <button
              class="tab-btn"
              :class="{ active: activeTab === 'pending' }"
              @click="activeTab = 'pending'"
            >
              All Pending
              <span class="tab-badge green">{{ summary.new_count + summary.mismatch_count + (summary.error_count || 0) }}</span>
            </button>

            <button
              class="tab-btn"
              :class="{ active: activeTab === 'new' }"
              @click="activeTab = 'new'"
            >
              New Files
              <span class="tab-badge">{{ summary.new_count }}</span>
            </button>

            <button
              class="tab-btn"
              :class="{ active: activeTab === 'mismatch' }"
              @click="activeTab = 'mismatch'"
            >
              Size Mismatch (_new)
              <span class="tab-badge amber">{{ summary.mismatch_count }}</span>
            </button>

            <button
              v-if="(summary.error_count || 0) > 0"
              class="tab-btn"
              :class="{ active: activeTab === 'errors' }"
              @click="activeTab = 'errors'"
            >
              Failed
              <span class="tab-badge rose">{{ summary.error_count }}</span>
            </button>

            <button
              class="tab-btn"
              :class="{ active: activeTab === 'skiplist' }"
              @click="activeTab = 'skiplist'"
            >
              Skip List
              <span class="tab-badge rose">{{ skipList.length }}</span>
            </button>

            <button
              class="tab-btn"
              :class="{ active: activeTab === 'all' }"
              @click="activeTab = 'all'"
            >
              All Files
              <span class="tab-badge">{{ scannedFiles.length }}</span>
            </button>
          </div>

          <!-- Search Filter -->
          <div class="search-box">
            <svg viewBox="0 0 24 24" width="14" height="14" stroke="currentColor" stroke-width="2" fill="none">
              <circle cx="11" cy="11" r="8"></circle>
              <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
            </svg>
            <input
              v-model="searchQuery"
              type="text"
              placeholder="Filter by filename or path..."
              class="search-input"
            />
          </div>
        </div>

        <!-- Skip List Special View -->
        <div v-if="activeTab === 'skiplist'" class="skiplist-management">
          <div class="skiplist-header">
            <div>
              <h3>Excluded / Blacklisted Files</h3>
              <p>Files in this list are automatically skipped during sync to prevent repeated stalls or crashes.</p>
            </div>
            <button
              v-if="skipList.length > 0"
              class="btn btn-secondary btn-sm"
              @click="clearAllSkipList"
            >
              Clear Entire Skip List
            </button>
          </div>

          <div v-if="skipList.length === 0" class="empty-state">
            <svg viewBox="0 0 24 24" width="40" height="40" stroke="currentColor" stroke-width="1.5" fill="none">
              <polyline points="20 6 9 17 4 12"></polyline>
            </svg>
            <p>No files are currently blacklisted in the skip list.</p>
          </div>

          <div v-else class="table-container">
            <table class="data-table">
              <thead>
                <tr>
                  <th style="width: 60px;">#</th>
                  <th>Relative Path</th>
                  <th style="width: 140px; text-align: right;">Action</th>
                </tr>
              </thead>
              <tbody>
                <tr v-for="(path, idx) in skipList" :key="path">
                  <td class="col-idx">{{ idx + 1 }}</td>
                  <td class="col-path font-mono" :title="path">
                    <span class="col-text-truncate">{{ path }}</span>
                  </td>
                  <td style="text-align: right;">
                    <button
                      class="btn btn-secondary btn-xs"
                      title="Remove file from skip list"
                      @click="removeFromSkipList(path)"
                    >
                      Remove
                    </button>
                  </td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>

        <!-- Normal Files Table -->
        <div v-else class="table-container">
          <div v-if="scannedFiles.length === 0" class="empty-state">
            <svg viewBox="0 0 24 24" width="48" height="48" stroke="currentColor" stroke-width="1.5" fill="none">
              <rect x="5" y="2" width="14" height="20" rx="3" ry="3"></rect>
              <line x1="12" y1="18" x2="12" y2="18.01"></line>
            </svg>
            <h3>No Scanned Files Yet</h3>
            <p>Ensure your iPhone is connected via USB and click <strong>Scan iPhone</strong> above to inspect and classify media.</p>
          </div>

          <div v-else-if="filteredFiles.length === 0" class="empty-state">
            <svg viewBox="0 0 24 24" width="40" height="40" stroke="currentColor" stroke-width="1.5" fill="none">
              <circle cx="11" cy="11" r="8"></circle>
              <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
            </svg>
            <p>No files matched your filter criteria.</p>
          </div>

          <table v-else class="data-table">
            <thead>
              <tr>
                <th style="width: 50px;">#</th>
                <th style="width: 200px;">Filename</th>
                <th>Path on iPhone</th>
                <th style="width: 100px;">Size</th>
                <th style="width: 240px;">Target Path in Backup</th>
                <th style="width: 130px; text-align: center;">Status</th>
              </tr>
            </thead>
            <tbody>
              <tr
                v-for="(file, idx) in filteredFiles"
                :key="file.object_id"
                :class="{
                  'row-new': file.status === 'NEW',
                  'row-mismatch': file.status === 'MISMATCH',
                  'row-skipped': file.status === 'SKIPPED',
                  'row-error': file.status === 'ERROR',
                  'row-context-active': contextMenuVisible && contextMenuFile?.object_id === file.object_id,
                }"
                @contextmenu.prevent.stop="openRowContextMenu($event, file)"
              >
                <td class="col-idx">{{ idx + 1 }}</td>
                <td class="col-name font-semibold" :title="file.filename">
                  <span class="file-ext-icon">{{ file.filename.split('.').pop()?.toUpperCase() }}</span>
                  <span class="col-text-truncate">{{ file.filename }}</span>
                </td>
                <td class="col-path font-mono text-muted" :title="file.relative_path">
                  <span class="col-text-truncate">{{ file.relative_path }}</span>
                </td>
                <td class="col-size">
                  <div>{{ formatBytes(file.size) }}</div>
                  <div v-if="file.existing_size !== undefined && file.status === 'MISMATCH'" class="existing-sub">
                    was {{ formatBytes(file.existing_size) }}
                  </div>
                </td>
                <td class="col-target font-mono" :title="file.target_relative_path">
                  <span class="col-text-truncate" :class="{ 'text-amber': file.status === 'MISMATCH' }">
                    {{ file.target_relative_path }}
                  </span>
                </td>
                <td class="col-status text-center">
                  <span
                    class="status-badge"
                    :class="file.status.toLowerCase()"
                    :title="file.error || undefined"
                  >
                    {{ file.status === 'MISMATCH' ? 'MISMATCH (_new)' : file.status }}
                  </span>
                  <div v-if="file.error" class="file-error-sub" :title="file.error">
                    {{ file.error }}
                  </div>
                </td>
              </tr>
            </tbody>
          </table>
        </div>
      </section>
    </main>

    <!-- Custom Row Context Menu (Teleported to body) -->
    <Teleport to="body">
      <div
        v-if="contextMenuVisible && contextMenuFile"
        class="custom-context-menu"
        :style="{ top: `${contextMenuY}px`, left: `${contextMenuX}px` }"
        @click.stop
      >
        <div class="context-menu-header">
          <span class="context-menu-filename" :title="contextMenuFile.filename">
            {{ contextMenuFile.filename }}
          </span>
        </div>
        <div class="context-menu-divider"></div>
        <button
          class="context-menu-item"
          :disabled="isSyncing"
          @click="syncThisFile(contextMenuFile)"
        >
          <svg viewBox="0 0 24 24" width="14" height="14" stroke="currentColor" stroke-width="2" fill="none">
            <polyline points="23 4 23 10 17 10"></polyline>
            <polyline points="1 20 1 14 7 14"></polyline>
            <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15"></path>
          </svg>
          <span>Sync this file</span>
        </button>
        <button
          class="context-menu-item"
          @click="toggleSkipForFile(contextMenuFile)"
        >
          <svg viewBox="0 0 24 24" width="14" height="14" stroke="currentColor" stroke-width="2" fill="none">
            <circle cx="12" cy="12" r="10"></circle>
            <line x1="4.93" y1="4.93" x2="19.07" y2="19.07"></line>
          </svg>
          <span>{{ skipList.includes(contextMenuFile.relative_path) || contextMenuFile.status === 'SKIPPED' ? 'Remove from skip list' : 'Skip file' }}</span>
        </button>
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
/* Dark Mode Modern Theme */
.app-shell {
  display: flex;
  flex-direction: column;
  height: 100vh;
  max-height: 100vh;
  width: 100%;
  max-width: 100%;
  background-color: #0f1117;
  color: #e2e8f0;
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  overflow: hidden;
  overflow-x: hidden;
  user-select: none;
}

/* Topbar */
.topbar {
  flex-shrink: 0;
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 10px 24px;
  background: #161922;
  border-bottom: 1px solid #232838;
}

.brand {
  display: flex;
  align-items: center;
  gap: 12px;
}

.brand-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border-radius: 10px;
  background: linear-gradient(135deg, #3b82f6, #6366f1);
  color: white;
  box-shadow: 0 4px 12px rgba(59, 130, 246, 0.3);
}

.brand-title {
  margin: 0;
  font-size: 1.15rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  color: #f8fafc;
}

.brand-sub {
  margin: 0;
  font-size: 0.75rem;
  color: #94a3b8;
}

.device-panel {
  display: flex;
  align-items: center;
  gap: 10px;
}

.device-badge {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 14px;
  border-radius: 20px;
  background: #1e2230;
  border: 1px solid #2d3448;
  font-size: 0.85rem;
}

.status-indicator {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background-color: #64748b;
}

.device-badge.connected .status-indicator {
  background-color: #10b981;
  box-shadow: 0 0 8px #10b981;
}

.device-badge.searching .status-indicator {
  background-color: #f59e0b;
  animation: pulse-dot 1.2s infinite ease-in-out;
}

.device-name {
  font-weight: 500;
  color: #f1f5f9;
}

.device-name.not-connected {
  color: #94a3b8;
}

/* Main Container */
.main-container {
  flex: 1;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 14px 24px;
  gap: 12px;
  overflow: hidden;
  overflow-x: hidden;
}

/* Control Card */
.control-card {
  flex-shrink: 0;
  background: #161922;
  border: 1px solid #232838;
  border-radius: 12px;
  padding: 12px 18px;
}

.folder-row {
  display: flex;
  justify-content: space-between;
  align-items: flex-end;
  gap: 20px;
  flex-wrap: wrap;
}

.input-group {
  flex: 1;
  min-width: 320px;
}

.input-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.8rem;
  font-weight: 600;
  color: #94a3b8;
  margin-bottom: 6px;
}

.input-with-button {
  display: flex;
  gap: 8px;
}

.text-input {
  flex: 1;
  background: #0f1117;
  border: 1px solid #2d3448;
  border-radius: 8px;
  padding: 8px 12px;
  color: #f8fafc;
  font-size: 0.875rem;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  outline: none;
  transition: border-color 0.2s;
}

.text-input:focus {
  border-color: #3b82f6;
}

.actions-group {
  display: flex;
  gap: 8px;
  align-items: center;
}

/* Buttons */
.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 8px 14px;
  font-size: 0.85rem;
  font-weight: 600;
  border-radius: 8px;
  border: 1px solid transparent;
  cursor: pointer;
  transition: all 0.15s ease;
  white-space: nowrap;
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: #2563eb;
  color: white;
}
.btn-primary:hover:not(:disabled) {
  background: #1d4ed8;
}

.btn-success {
  background: #059669;
  color: white;
}
.btn-success:hover:not(:disabled) {
  background: #047857;
}

.btn-warning {
  background: #d97706;
  color: white;
}
.btn-warning:hover:not(:disabled) {
  background: #b45309;
}

.btn-danger {
  background: #dc2626;
  color: white;
}
.btn-danger:hover:not(:disabled) {
  background: #b91c1c;
}

.btn-secondary {
  background: #1e2230;
  border-color: #2d3448;
  color: #cbd5e1;
}
.btn-secondary:hover:not(:disabled) {
  background: #272d40;
  color: white;
}

.btn-sm {
  padding: 4px 10px;
  font-size: 0.75rem;
}

.btn-xs {
  padding: 2px 8px;
  font-size: 0.7rem;
}

.kbd-badge {
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 4px;
  padding: 1px 5px;
  font-size: 0.7rem;
  font-family: inherit;
}

/* Alerts */
.alert {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 14px;
  border-radius: 8px;
  font-size: 0.85rem;
}

.alert-error {
  background: rgba(220, 38, 38, 0.15);
  border: 1px solid rgba(220, 38, 38, 0.3);
  color: #fca5a5;
}

.alert-info {
  background: rgba(59, 130, 246, 0.15);
  border: 1px solid rgba(59, 130, 246, 0.3);
  color: #93c5fd;
}

/* Progress Card */
.progress-card {
  flex-shrink: 0;
  background: #161922;
  border: 1px solid #3b82f6;
  box-shadow: 0 0 15px rgba(59, 130, 246, 0.15);
  border-radius: 12px;
  padding: 12px 18px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.progress-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.progress-title-block {
  display: flex;
  align-items: center;
  gap: 8px;
  overflow: hidden;
}

.pulse-indicator {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: #3b82f6;
  animation: pulse-dot 1s infinite alternate;
}

.file-name-label {
  font-weight: 600;
  font-size: 0.95rem;
  color: #f8fafc;
}

.file-path-sub {
  font-size: 0.75rem;
  color: #64748b;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
}

.progress-stats {
  display: flex;
  gap: 8px;
}

.speed-pill,
.counter-pill {
  font-size: 0.75rem;
  padding: 3px 8px;
  border-radius: 12px;
  background: #1e2230;
  border: 1px solid #2d3448;
}

.speed-pill {
  color: #38bdf8;
  font-weight: 600;
}

.progress-bar-wrapper {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.bar-info {
  display: flex;
  justify-content: space-between;
  font-size: 0.75rem;
  color: #94a3b8;
}

.percent-label {
  font-weight: 700;
  color: #f1f5f9;
}

.progress-track {
  height: 8px;
  background: #0f1117;
  border-radius: 4px;
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  border-radius: 4px;
  transition: width 0.15s ease-out;
}

.progress-fill.current-file {
  background: linear-gradient(90deg, #3b82f6, #60a5fa);
}

.progress-fill.batch {
  background: linear-gradient(90deg, #10b981, #34d399);
}

/* Metrics Grid */
.metrics-grid {
  flex-shrink: 0;
  display: grid;
  grid-template-columns: repeat(6, minmax(0, 1fr));
  gap: 10px;
}

.metric-card {
  background: #161922;
  border: 1px solid #232838;
  border-radius: 10px;
  padding: 10px 14px;
  display: flex;
  align-items: center;
  gap: 12px;
  transition: all 0.2s;
}

.metric-card.highlight {
  border-color: #3b82f6;
  background: #191e2b;
}

.metric-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 36px;
  height: 36px;
  border-radius: 8px;
}

.metric-icon.blue { background: rgba(59, 130, 246, 0.15); color: #60a5fa; }
.metric-icon.purple { background: rgba(168, 85, 247, 0.15); color: #c084fc; }
.metric-icon.green { background: rgba(16, 185, 129, 0.15); color: #34d399; }
.metric-icon.amber { background: rgba(245, 158, 11, 0.15); color: #fbbf24; }
.metric-icon.cyan { background: rgba(6, 182, 212, 0.15); color: #22d3ee; }
.metric-icon.rose { background: rgba(244, 63, 94, 0.15); color: #fb7185; }

.metric-content {
  display: flex;
  flex-direction: column;
}

.metric-val {
  font-size: 1.25rem;
  font-weight: 700;
  line-height: 1.2;
}

.metric-lbl {
  font-size: 0.7rem;
  color: #94a3b8;
  white-space: nowrap;
}

.text-green { color: #34d399; }
.text-amber { color: #fbbf24; }
.text-cyan { color: #22d3ee; }
.text-rose { color: #fb7185; }

/* Table Section */
.table-section {
  flex: 1;
  min-height: 0;
  background: #161922;
  border: 1px solid #232838;
  border-radius: 12px;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

.tab-toolbar {
  flex-shrink: 0;
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 16px;
  border-bottom: 1px solid #232838;
  background: #191c26;
  gap: 12px;
}

.tabs-list {
  display: flex;
  gap: 6px;
}

.tab-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border-radius: 6px;
  border: 1px solid transparent;
  background: transparent;
  color: #94a3b8;
  font-size: 0.8rem;
  font-weight: 500;
  cursor: pointer;
}

.tab-btn:hover {
  color: #f1f5f9;
  background: #202433;
}

.tab-btn.active {
  background: #242938;
  color: #38bdf8;
  border-color: #333a4f;
  font-weight: 600;
}

.tab-badge {
  font-size: 0.7rem;
  padding: 1px 6px;
  border-radius: 10px;
  background: #2d3448;
  color: #cbd5e1;
}

.tab-badge.green { background: rgba(16, 185, 129, 0.2); color: #34d399; }
.tab-badge.amber { background: rgba(245, 158, 11, 0.2); color: #fbbf24; }
.tab-badge.rose { background: rgba(244, 63, 94, 0.2); color: #fb7185; }

.search-box {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 10px;
  background: #0f1117;
  border: 1px solid #2d3448;
  border-radius: 6px;
  width: 240px;
  color: #94a3b8;
}

.search-input {
  border: none;
  background: transparent;
  color: #f8fafc;
  font-size: 0.8rem;
  width: 100%;
  outline: none;
}

/* Data Table */
.table-container {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  overflow-x: hidden;
  scrollbar-width: thin;
  scrollbar-color: rgba(255, 255, 255, 0.18) transparent;
}

.table-container::-webkit-scrollbar {
  width: 6px;
  height: 6px;
}

.table-container::-webkit-scrollbar-track {
  background: transparent;
}

.table-container::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.18);
  border-radius: 9999px;
  transition: background-color 0.2s ease;
}

.table-container::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.32);
}

.data-table {
  width: 100%;
  table-layout: fixed;
  border-collapse: collapse;
  font-size: 0.8rem;
}

.col-text-truncate {
  display: inline-block;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  vertical-align: middle;
}

.data-table th {
  position: sticky;
  top: 0;
  background: #191c26;
  border-bottom: 1px solid #232838;
  padding: 8px 12px;
  text-align: left;
  font-weight: 600;
  color: #94a3b8;
  z-index: 1;
}

.data-table td {
  padding: 8px 12px;
  border-bottom: 1px solid #1c202d;
  color: #cbd5e1;
}

.data-table tbody tr:hover {
  background: #1b1f2b;
}

.row-new {
  background: rgba(16, 185, 129, 0.03);
}

.row-mismatch {
  background: rgba(245, 158, 11, 0.03);
}

.row-skipped {
  opacity: 0.6;
}

.col-idx {
  color: #64748b;
  text-align: center;
}

.col-name {
  color: #f8fafc;
  display: flex;
  align-items: center;
  gap: 6px;
}

.file-ext-icon {
  font-size: 0.65rem;
  font-weight: 700;
  padding: 1px 4px;
  border-radius: 3px;
  background: #2d3448;
  color: #94a3b8;
}

.existing-sub {
  font-size: 0.7rem;
  color: #f59e0b;
}

.status-badge {
  display: inline-block;
  padding: 2px 8px;
  border-radius: 12px;
  font-size: 0.7rem;
  font-weight: 600;
}

.status-badge.new {
  background: rgba(16, 185, 129, 0.15);
  color: #34d399;
  border: 1px solid rgba(16, 185, 129, 0.3);
}

.status-badge.mismatch {
  background: rgba(245, 158, 11, 0.15);
  color: #fbbf24;
  border: 1px solid rgba(245, 158, 11, 0.3);
}

.status-badge.verified_ignored {
  background: rgba(100, 116, 139, 0.15);
  color: #94a3b8;
  border: 1px solid rgba(100, 116, 139, 0.3);
}

.status-badge.skipped {
  background: rgba(244, 63, 94, 0.15);
  color: #fb7185;
  border: 1px solid rgba(244, 63, 94, 0.3);
}

/* Empty State */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  height: 280px;
  color: #64748b;
  gap: 12px;
  text-align: center;
  padding: 20px;
}

.empty-state h3 {
  color: #e2e8f0;
  margin: 0;
  font-size: 1.05rem;
}

.empty-state p {
  margin: 0;
  font-size: 0.85rem;
  max-width: 450px;
  line-height: 1.5;
}

/* Skip list view */
.skiplist-management {
  flex: 1;
  min-height: 0;
  padding: 14px 16px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  overflow: hidden;
}

.skiplist-header {
  flex-shrink: 0;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.skiplist-header h3 {
  margin: 0 0 4px 0;
  color: #f8fafc;
  font-size: 0.95rem;
}

.skiplist-header p {
  margin: 0;
  font-size: 0.75rem;
  color: #94a3b8;
}

/* Animations */
@keyframes pulse-dot {
  0% { transform: scale(0.9); opacity: 0.7; }
  100% { transform: scale(1.15); opacity: 1; }
}

.spin {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

.spinner-small {
  width: 14px;
  height: 14px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: white;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

/* Scan Progress Card */
.scan-progress-card {
  flex-shrink: 0;
  background: #161922;
  border: 1px solid #3b82f6;
  box-shadow: 0 0 15px rgba(59, 130, 246, 0.15);
  border-radius: 12px;
  padding: 12px 18px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.scan-progress-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.scan-progress-title-block {
  display: flex;
  align-items: center;
  gap: 12px;
}

.scan-pulse-ring {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: #3b82f6;
  box-shadow: 0 0 0 rgba(59, 130, 246, 0.7);
  animation: pulse-ring 1.4s infinite cubic-bezier(0.66, 0, 0, 1);
}

@keyframes pulse-ring {
  0% {
    box-shadow: 0 0 0 0 rgba(59, 130, 246, 0.7);
  }
  70% {
    box-shadow: 0 0 0 8px rgba(59, 130, 246, 0);
  }
  100% {
    box-shadow: 0 0 0 0 rgba(59, 130, 246, 0);
  }
}

.scan-phase-title {
  font-weight: 600;
  font-size: 0.95rem;
  color: #f8fafc;
}

.scan-phase-sub {
  font-size: 0.75rem;
  color: #94a3b8;
  margin-top: 2px;
}

.scan-live-message {
  color: #94a3b8;
}

.scan-header-actions {
  display: flex;
  align-items: center;
  gap: 10px;
}

.scan-badge {
  font-size: 0.75rem;
  padding: 4px 10px;
  border-radius: 12px;
  background: rgba(59, 130, 246, 0.15);
  border: 1px solid rgba(59, 130, 246, 0.3);
  color: #60a5fa;
  font-weight: 600;
}

.scan-track {
  height: 8px;
  background: #0f1117;
  border-radius: 4px;
  overflow: hidden;
  position: relative;
}

.scan-fill-real {
  height: 100%;
  background: linear-gradient(90deg, #3b82f6, #60a5fa);
  border-radius: 4px;
  transition: width 0.25s ease-out;
}

.scan-stats-footer {
  display: flex;
  justify-content: space-between;
  align-items: center;
  font-size: 0.72rem;
  color: #94a3b8;
  margin-top: 2px;
}

.scan-footer-left {
  color: #64748b;
}

.scan-footer-right {
  color: #38bdf8;
  font-weight: 600;
}

/* Metric Cards Clickable */
.metric-card.clickable {
  cursor: pointer;
  user-select: none;
}

.metric-card.clickable:hover {
  background: #1e2433;
  border-color: #3b82f6;
  transform: translateY(-1px);
}

.metric-card.clickable.active {
  border-color: #38bdf8;
  box-shadow: 0 0 10px rgba(56, 189, 248, 0.2);
}

/* Table Row Error & Context Menu Active */
.row-error {
  background: rgba(239, 68, 68, 0.06);
}

.row-context-active {
  background: #232a3d !important;
  outline: 1px solid #3b82f6;
}

.status-badge.error {
  background: rgba(239, 68, 68, 0.15);
  color: #f87171;
  border: 1px solid rgba(239, 68, 68, 0.3);
}

.file-error-sub {
  font-size: 0.65rem;
  color: #f87171;
  margin-top: 2px;
  max-width: 140px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* Custom Context Menu */
.custom-context-menu {
  position: fixed;
  z-index: 9999;
  min-width: 180px;
  background: #191c26;
  border: 1px solid #333a4f;
  border-radius: 8px;
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.5), 0 2px 6px rgba(0, 0, 0, 0.3);
  padding: 6px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  user-select: none;
  animation: context-menu-fade 0.1s ease-out;
}

@keyframes context-menu-fade {
  from {
    opacity: 0;
    transform: scale(0.96);
  }
  to {
    opacity: 1;
    transform: scale(1);
  }
}

.context-menu-header {
  padding: 4px 8px 6px 8px;
}

.context-menu-filename {
  font-size: 0.72rem;
  font-weight: 600;
  color: #94a3b8;
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 170px;
}

.context-menu-divider {
  height: 1px;
  background: #272d3e;
  margin: 2px 0 4px 0;
}

.context-menu-item {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 10px;
  font-size: 0.8rem;
  font-weight: 500;
  color: #f1f5f9;
  background: transparent;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  text-align: left;
  transition: all 0.12s ease;
}

.context-menu-item:hover:not(:disabled) {
  background: #2563eb;
  color: white;
}

.context-menu-item:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.context-menu-item svg {
  flex-shrink: 0;
}
</style>
