# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0] - 2026-11-13

### Added
- **Windows Portable Devices (WPD) Driver**: Direct COM communication via `IPortableDeviceManager` and `IPortableDeviceContent` for fast, resilient MTP traversal.
- **Unicode Direction Marker Sanitizer**: Automatic stripping of invisible directional Unicode characters (`\u{200E}`, `\u{200F}`, `\u{202A}`–`\u{202E}`) from file paths.
- **Byte-Exact Local Diffing**: High-speed recursive indexing using `walkdir`, classifying camera roll assets into `NEW`, `MISMATCH`, and `VERIFIED_IGNORED`.
- **Non-Destructive Size Mismatch Handling**: Safe writing to `<filename>_new.<ext>` on size mismatch to prevent data loss.
- **`_new` Variant Matching Rule**: Skips redownloading files if an existing `<stem>_new.<ext>` variant matches the iPhone asset size.
- **Streamed Chunk Copying**: Reads streams in 64KB–256KB chunks into `.part` files with atomic rename on completion.
- **Automatic Crash Recovery**: Uses `current_copying.tmp` lockfile to detect interrupted transfers and automatically blacklist stalled assets on next launch.
- **Instant Skip Mechanism**: Single-key (<kbd>S</kbd>) and UI button skip functionality to abort hanging assets, delete partial artifacts, and advance immediately.
- **Dark Mode Dashboard (Vue 3 + TypeScript)**:
  - Real-time progress bars with dual file and batch progress percentages and transfer speed (MB/s).
  - Metrics cards displaying total, new, mismatch, verified ignored, and blacklisted counts.
  - Interactive file tables with tabs (`All Pending`, `New Files`, `Size Mismatch (_new)`, `Skip List`, `All Files`).
  - Search filter by filename and directory path.
  - Sleek 6px custom scrollbar styling with zero window-level horizontal overflow.
