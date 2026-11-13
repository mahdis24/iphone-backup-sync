<div align="center">

<img src="src-tauri/icons/128x128.png" width="120" height="120" alt="iPhone Backup Sync Logo" />

# iPhone Backup Sync

<p>
  <a href="https://github.com/mahdis24/iphone-backup-sync/releases/latest"><img src="https://img.shields.io/github/v/release/mahdis24/iphone-backup-sync?color=24C8D5&label=Release&logo=github" alt="GitHub Release" /></a>
  <a href="https://github.com/mahdis24/iphone-backup-sync/actions/workflows/ci.yml"><img src="https://github.com/mahdis24/iphone-backup-sync/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-blue.svg" alt="License: MIT" /></a>
  <a href="https://microsoft.com/windows"><img src="https://img.shields.io/badge/Platform-Windows%2010%20%7C%2011-0078D4?logo=windows&logoColor=white" alt="Platform" /></a>
  <a href="https://tauri.app/"><img src="https://img.shields.io/badge/Tauri-v2-24C8D5?logo=tauri&logoColor=white" alt="Tauri v2" /></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-2024%20Edition-dea584?logo=rust&logoColor=white" alt="Rust" /></a>
  <a href="https://vuejs.org/"><img src="https://img.shields.io/badge/Vue-3.5-4FC08D?logo=vue.js&logoColor=white" alt="Vue 3" /></a>
  <a href="https://www.typescriptlang.org/"><img src="https://img.shields.io/badge/TypeScript-6.x-3178C6?logo=typescript&logoColor=white" alt="TypeScript" /></a>
</p>

### [⬇️ Download Latest Release (.exe)](https://github.com/mahdis24/iphone-backup-sync/releases/latest)

</div>

---

**iPhone Backup Sync** is a high-performance desktop application designed for fast, incremental local backup of iPhone DCIM photos and videos to Windows drives using Rust, Tauri, and `libimobiledevice` / Windows Portable Devices (WPD) protocols. Communicating directly with low-level device streaming interfaces, it prevents MTP transfer freezes, handles size conflicts without overwriting, and operates completely offline with zero cloud subscriptions.

---

## Comparison

| Feature / Capability                      | `iphone-backup-sync`                                                                                              | Windows File Explorer                                                                                     | iTunes / iCloud for Windows                                                                         |
| :---------------------------------------- | :---------------------------------------------------------------------------------------------------------------- | :-------------------------------------------------------------------------------------------------------- | :-------------------------------------------------------------------------------------------------- |
| **Incremental Sync**                      | **Yes** — indexes destination and copies only new or modified assets                                              | **No** — manual drag-and-drop; re-evaluates all files on every copy                                       | **Partial** — iTunes creates opaque device image; iCloud syncs through cloud                        |
| **Conflict & Duplicate Handling**         | **Non-destructive** — preserves originals, saves size mismatches as `<stem>_new.<ext>`, skips verified duplicates | **Destructive / Guesswork** — prompts to overwrite or skip without byte verification                      | **Opaque** — overwrites or replaces local files based on cloud cache state                          |
| **Connection Stability & Crash Recovery** | **High** — atomic `.part` chunked streaming, lockfile crash recovery, and auto-quarantine                         | **Poor** — freezes with _"Catastrophic failure"_ or _"Device unreachable"_, leaving corrupt partial files | **Moderate / Rigid** — fails entire backup batch if a single connection timeout occurs              |
| **Cloud & Subscription Requirements**     | **Zero** — 100% offline, local direct USB transfer                                                                | **Zero** — offline USB, but hampered by MTP driver stalls                                                 | **Required for Full Library** — requires Apple ID and paid iCloud storage plans for large libraries |
| **Stuck File Handling**                   | **Instant skip** with <kbd>S</kbd> key; blacklists failing asset without halting batch                            | **Unresponsive** — hangs entire Explorer window; requires killing task or unplugging device               | **Fails entire process** — aborts backup on unreadable files                                        |
| **Path & Unicode Integrity**              | **Sanitized** — strips invisible iOS Unicode direction markers (`\u{200E}`, `\u{200F}`, etc.)                     | **Unsanitized** — fails path checks or creates phantom folders on NTFS drives                             | **Abstracted** — stored inside proprietary iOS backup database structure                            |

---

## Preview

![iPhone Backup Sync Dashboard](docs/screenshots/preview.png)

---

## Installation & Windows SmartScreen

Download the latest `.exe` installer or standalone executable from the [GitHub Releases](https://github.com/mahdis24/iphone-backup-sync/releases/latest) page.

### Handling the "Unknown Publisher" Notice

When running the installer for the first time, Windows Defender SmartScreen may display a warning dialog stating _"Windows protected your PC"_ or _"Unknown Publisher"_.

> **Why does this happen?**  
> `iphone-backup-sync` is a free, community-driven open-source project. Code-signing certificates that suppress this prompt (Extended Validation / EV certificates) require costly annual subscriptions from commercial certificate authorities.

To run the application:

1. Click **More info** on the SmartScreen dialog.
2. Click **Run anyway**.

**Security & Open-Source Assurance:**

- The complete source code is publicly accessible and auditable in this repository.
- Binaries are built transparently in isolated virtual machines using GitHub Actions (`.github/workflows/release.yml`).
- No telemetry, analytics, or external network requests are executed at any time.
- You can inspect the automated build logs or build the binary locally from source.

---

## Recommended iPhone Setting

To avoid mid-transfer iOS transcoding crashes over USB, configure your device:

1. Open **Settings** on your iPhone.
2. Navigate to **Photos**.
3. Scroll to the bottom under **Transfer to Mac or PC** and select **Keep Originals**.

Selecting _Keep Originals_ prevents iOS from attempting on-the-fly HEIC/JPEG or ProRes/H.264 transcoding, which is the primary cause of MTP stream timeouts and device resets.

---

## Core Features

- **Direct WPD & MTP Streaming:** Directly interfaces with the Windows Portable Devices COM subsystem (`IPortableDeviceManager`, `IPortableDevice`, `IPortableDeviceContent`, `IStream`) and compatible with `ifuse` / `libimobiledevice` architectures, eliminating Explorer shell crashes.
- **Recursive DCIM Scanning:** Recursively traverses the iPhone storage root, automatically discovering all media subdirectories (`DCIM/*APPLE`).
- **Byte-Exact Diffing & Classification:** Scans destination folders via indexed hash maps, classifying assets into:
  - `NEW`: Asset does not exist in target backup directory.
  - `MISMATCH`: Asset exists with a different file size.
  - `VERIFIED_IGNORED`: Asset exists with exact byte match, or an exact-sized `_new` variant is already present.
  - `SKIPPED`: Asset is listed in the persistent skip list.
- **Non-Destructive Conflict Resolution:** Size-mismatched files are saved with a `_new` suffix (e.g., `IMG_1234_new.HEIC`). If an identical-sized `_new` file already exists, it is marked as backed up and skipped automatically.
- **Resilient Chunked Streaming:** Media streams in 64 KB – 256 KB buffers directly into temporary `.part` files, flushing and atomically renaming to final destination only after complete transfer.
- **Crash Recovery & Lockfile Tracking:** Employs a `current_copying.tmp` lockfile. If an MTP disconnection or system shutdown occurs mid-copy, the interrupted file is automatically quarantined to `skip_list.json` on next startup, and partial files are removed.
- **Instant Skip Shortcut (<kbd>S</kbd>):** Press <kbd>S</kbd> or click **Skip Current** to immediately drop an in-flight file, delete its partial fragment, add it to `skip_list.json`, and proceed with remaining queue.
- **Skip List Management:** Full UI interface to inspect blacklisted files, remove specific entries, or clear the entire skip list.
- **Unicode Path Sanitization:** Strips invisible Unicode direction control markers (`\u{200E}`, `\u{200F}`, `\u{202A}`..=`\u{202E}`) from iOS metadata to ensure valid NTFS paths.
- **Real-Time Transfer Metrics:** Displays current file progress, overall batch percentage, streaming throughput (MB/s), and real-time status banners.
- **Offline & Private:** Operates strictly over local USB with zero telemetry or network calls.

---

## Architecture

```
iphone-backup-sync/
├── src/                          # Vue 3 + TypeScript Frontend
│   ├── App.vue                   # Dashboard, real-time monitors, and file tables
│   ├── types.ts                  # Serde-synchronized TypeScript data contracts
│   ├── style.css                 # Dark-mode styling, responsive metrics, custom scrollbars
│   └── main.ts                   # Application bootstrap
├── src-tauri/                    # Rust Backend (Tauri v2)
│   ├── src/
│   │   ├── wpd.rs                # Windows Portable Devices COM wrappers & chunked streaming
│   │   ├── sync.rs               # Diffing engine, crash recovery, and skip-list manager
│   │   ├── lib.rs                # Tauri command dispatch and IPC event emitters
│   │   └── main.rs               # Executable entrypoint
│   ├── Cargo.toml                # Rust dependencies (windows, tokio, walkdir, serde)
│   └── tauri.conf.json           # Tauri v2 application configuration & bundle settings
└── .github/workflows/            # GitHub Actions CI/CD pipelines
    ├── ci.yml                    # Automated formatting, linting, and build verification
    └── release.yml               # Automated release packaging via tauri-action
```

---

## Building from Source

### Prerequisites

- **Operating System:** Windows 10 or 11 (64-bit).
- **Rust Toolchain:** Stable `x86_64-pc-windows-msvc` toolchain ([rustup.rs](https://rustup.rs/)).
- **Node.js runtime or Bun:** [Bun](https://bun.sh/) (v1.0+) or [Node.js](https://nodejs.org/) (v18+).
- **C++ Build Tools:** [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with Windows SDK.

### 1. Clone the Repository

```bash
git clone https://github.com/mahdis24/iphone-backup-sync.git
cd iphone-backup-sync
```

### 2. Install Frontend Dependencies

Using Bun (recommended):

```bash
bun install
```

Using npm:

```bash
npm install
```

### 3. Run in Development Mode

```bash
bun run tauri dev
# or: npm run tauri dev
```

### 4. Run Quality Checks & Linters

```bash
# Frontend type check & build
bun run build
# or: npm run build

# Rust formatting check
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

# Rust Clippy linter
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

### 5. Build Standalone Installer & Binary

```bash
bun run tauri build
# or: npm run tauri build
```

Compiled binaries and NSIS installer (`.exe`) are output to:
`src-tauri/target/release/bundle/nsis/`

---

## Usage Workflow

1. Connect your iPhone to your Windows PC with a high-quality USB cable.
2. Unlock your iPhone and tap **Trust This Computer** if prompted.
3. Launch **iPhone Backup Sync**. The detected device will appear in the top-right status badge.
4. Click **Browse...** to select your target backup directory on any internal, external, or networked drive.
5. Click **Scan iPhone**. The application will recursively index your DCIM directory and compare it against the backup folder.
6. Click **Start Sync** to initiate incremental chunked copying.
7. Use <kbd>S</kbd> at any point during active transfer to skip an unresponsive or corrupt asset.

---

## Contributing

Contributions and issue reports are welcome. Please consult [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before opening a pull request.

---

## License & Security

- **License:** Distributed under the [MIT License](LICENSE).
- **Security:** Review [SECURITY.md](SECURITY.md) for vulnerability disclosure procedures.
- **Author:** Created by [Mahdi Salek](https://mahdis24.tech) ([@mahdis24](https://github.com/mahdis24)).
