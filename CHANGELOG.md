# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-11-15

### Added
- **Multi-Stage Real-Time Scan Progress**: Emits granular progress during device connection (5%), media enumeration (10% to 35%), local backup indexing (35% to 63%), and file classification (65% to 100%).
- **Transparent Scan Progress Card**: Replaced the generic indeterminate loading bar with real-time percentage indicators, phase descriptions, and live item counters (e.g. files discovered, files indexed, items diffed).
- **Scan Cancellation Support**: Added `cancel_scan` command and UI button to safely interrupt device scans at any stage via thread-safe atomic cancellation tokens.
- **Custom Context Menu**: Right-click context menu on file table rows featuring "Sync this file" for single-asset transfer and "Skip file" to toggle persistent skip list status.
- **Global Context Menu Suppression**: Disabled default browser context menu globally across the app window for a native desktop application experience.
- **Dedicated "Stop Sync" Button**: Allows immediate cancellation of an active batch transfer with safe deletion of temporary `.part` files without blacklisting cancelled items.
- **Interactive Metric Card Filters**: All dashboard metric cards (Total on iPhone, Total in Backup, NEW Files, Size Mismatch, Verified Ignored, Blacklisted) now act as clickable table filters.
- **Resilient Sync Error Recovery**: Added `ERROR` status for files encountering device read faults (such as `0x8007001E`), recording specific error causes and providing a dedicated "Failed" filter tab.

### Changed
- **WPD Session Lifecycle Management**: Implemented `WpdDeviceGuard` implementing `Drop` with `device.Close()` to ensure MTP sessions are cleanly closed, preventing locked device sessions.
- **Terminal Batch Completion**: Sync engine reliably emits terminal completion events (`finished` or `aborted`) even when errors occur, ensuring transfer state and buttons always reset properly.

## [0.1.0] - 2026-11-13

### Added
- **Windows Portable Devices (WPD) Driver**: Direct COM communication via `IPortableDeviceManager` and `IPortableDeviceContent` for fast, resilient MTP traversal.
- **Unicode Direction Marker Sanitizer**: Automatic stripping of invisible directional Unicode characters (`\u{200E}`, `\u{200F}`, `\u{202A}`-`\u{202E}`) from file paths.
- **Byte-Exact Local Diffing**: High-speed recursive indexing using `walkdir`, classifying camera roll assets into `NEW`, `MISMATCH`, and `VERIFIED_IGNORED`.
- **Non-Destructive Size Mismatch Handling**: Safe writing to `<filename>_new.<ext>` on size mismatch to prevent data loss.
- **`_new` Variant Matching Rule**: Skips redownloading files if an existing `<stem>_new.<ext>` variant matches the iPhone asset size.
- **Streamed Chunk Copying**: Reads streams in 64KB-256KB chunks into `.part` files with atomic rename on completion.
- **Automatic Crash Recovery**: Uses `current_copying.tmp` lockfile to detect interrupted transfers and automatically blacklist stalled assets on next launch.
- **Instant Skip Mechanism**: Single-key (<kbd>S</kbd>) and UI button skip functionality to abort hanging assets, delete partial artifacts, and advance immediately.
- **Dark Mode Dashboard (Vue 3 + TypeScript)**:
  - Real-time progress bars with dual file and batch progress percentages and transfer speed (MB/s).
  - Metrics cards displaying total, new, mismatch, verified ignored, and blacklisted counts.
  - Interactive file tables with tabs (`All Pending`, `New Files`, `Size Mismatch (_new)`, `Skip List`, `All Files`).
  - Search filter by filename and directory path.
  - Sleek 6px custom scrollbar styling with zero window-level horizontal overflow.
