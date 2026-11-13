# iPhone Backup Sync

[![CI](https://github.com/mahdis24/iphone-backup-sync/actions/workflows/ci.yml/badge.svg)](https://github.com/mahdis24/iphone-backup-sync/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Tauri v2](https://img.shields.io/badge/Tauri-v2-24C8D5?logo=tauri&logoColor=white)](https://tauri.app/)
[![Rust](https://img.shields.io/badge/Rust-2024%20Edition-dea584?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Vue 3](https://img.shields.io/badge/Vue-3.5-4FC08D?logo=vue.js&logoColor=white)](https://vuejs.org/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.x-3178C6?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
[![Platform: Windows](https://img.shields.io/badge/Platform-Windows%2010%2F11-0078D4?logo=windows&logoColor=white)](https://microsoft.com/windows)

A high-reliability, zero-telemetry desktop application built with **Tauri v2**, **Rust**, and **Vue 3** that solves the notorious Windows MTP transfer crashes (*"A device attached to the system is not functioning"*, *"Catastrophic failure"*, silent freezes, and corrupt partial files) when backing up photos and videos from iPhones to Windows.

---

## The Problem with Windows MTP & iOS

Anyone who has tried copying large camera rolls (100+ GB of HEIC, ProRAW, and 4K ProRes videos) from an iPhone to Windows using Windows File Explorer has run into:
1. **MTP Driver Timeouts & Hangs**: Explorer locks up completely when an asset cannot be transcoded or read in time, forcing you to unplug the device and guess where the transfer stopped.
2. **Accidental Overwrites or Duplicates**: Explorer blindly asks to overwrite or skip, without checking whether existing files are valid or identical in byte size.
3. **Hidden Unicode Markers**: iOS directory and file names often include invisible Unicode direction markers (`\u{200E}`, `\u{200F}`, `\u{202A}`–`\u{202E}`) that cause path comparisons to silently fail or create phantom folders on NTFS drives.
4. **Stuck Assets Halting the Entire Batch**: A single corrupted live photo or iCloud-offloaded stub stalls the whole transfer.

**iPhone Backup Sync** communicates directly with the low-level **Windows Portable Devices (WPD)** COM subsystem in Rust, streaming media in chunks, diffing files with byte-exact precision, recovering from crashes automatically, and letting you skip problematic assets with a single keystroke.

---

## 💡 Why I Built This

This project started out of a personal frustration: trying to reliably back up thousands of photos and large 4K videos from an iPhone to a Windows PC over USB/MTP. Between Windows Explorer's infamous *"Catastrophic failure"* errors, silent overwrites, and transfers freezing mid-way without telling you which files actually made it, backing up media became a chore.

After building a local script to solve this for my own workflow, I decided to turn it into a proper desktop utility and open-source it—hoping it saves time and headaches for anyone else dealing with the same Windows + iPhone MTP bottlenecks.

---

## Preview

![iPhone Backup Sync Screenshot](docs/screenshots/preview.png)
*(Drop an interface preview screenshot at `docs/screenshots/preview.png`)*

---

## Key Features

- **Direct WPD COM Integration**: Native access to `IPortableDeviceManager` and `IPortableDeviceContent` without relying on flaky Windows Explorer shell extensions.
- **Unicode Path Sanitization**: Strips invisible directional markers (`\u{200E}`, `\u{200F}`, `\u{202A}`..=`\u{202E}`) to ensure reliable NTFS and path matching.
- **Byte-Exact Diffing**:
  - Classifies assets into `NEW`, `MISMATCH`, and `VERIFIED_IGNORED`.
  - **Non-Destructive `_new` Variant Rule**: Size-mismatched files are never overwritten; they are saved alongside originals as `<stem>_new.<ext>`. If a `_new` variant of exact size already exists in the backup, the file is automatically recognized as backed up and skipped.
- **Resilient Chunked Streaming**: Media is read in 64KB–256KB streams directly to `.part` files, verifying integrity before renaming to final filenames.
- **Crash Recovery & Lockfile**: Maintains a `current_copying.tmp` lockfile. If an MTP disconnection or system shutdown occurs, the stalled asset is automatically moved to `skip_list.json` on the next launch.
- **Instant Skip (<kbd>S</kbd> Shortcut)**: Press <kbd>S</kbd> or click **Skip Current** to instantly abort a hanging file, purge its partial file on disk, blacklist it in `skip_list.json`, and proceed with the remaining queue.
- **100% Offline & Private**: Zero cloud dependencies, zero telemetry, zero analytics. All operations occur strictly locally over USB.

---

## Architecture

```
iphone-backup-sync/
├── src/                          # Vue 3 + TypeScript Frontend
│   ├── App.vue                   # Dark-mode dashboard, metrics & file tables
│   ├── types.ts                  # Serde-synchronized TypeScript interfaces
│   ├── style.css                 # Sleek 6px custom scrollbars & layout rules
│   └── main.ts                   # Vue app initialization
└── src-tauri/                    # Rust Backend (Tauri v2)
    ├── src/
    │   ├── wpd.rs                # Windows Portable Devices COM wrappers & chunk streaming
    │   ├── sync.rs               # Diffing engine, crash recovery, and skip-list manager
    │   └── lib.rs                # Tauri command handlers & IPC event emitter
    ├── Cargo.toml                # Native windows crate features (WPD, COM, Shell)
    └── tauri.conf.json           # Tauri v2 application configuration
```

---

## Prerequisites

1. **Windows 10 / 11** (64-bit).
2. **iPhone Configuration**:
   - On your iPhone, navigate to **Settings** > **Photos**.
   - Scroll to the bottom under **Transfer to Mac or PC** and select **"Keep Originals"**.
   *(This prevents iOS from attempting on-the-fly JPEG/H.264 transcoding over USB, which is the #1 cause of MTP disconnects).*
3. **Development Environment**:
   - [Rust](https://rustup.rs/) (stable `x86_64-pc-windows-msvc` toolchain recommended).
   - [Node.js](https://nodejs.org/) (v18+) or [Bun](https://bun.sh/) (v1.0+).
   - [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) (included with standard Windows Rust setups).

---

## Getting Started

### 1. Clone the Repository
```bash
git clone https://github.com/mahdis24/iphone-backup-sync.git
cd iphone-backup-sync
```

### 2. Install Dependencies
Using **Bun**:
```bash
bun install
```
*(or using npm)*:
```bash
npm install
```

### 3. Run in Development Mode
```bash
bun run tauri dev
# or
npm run tauri dev
```

### 4. Build Standalone Installer
```bash
bun run tauri build
# or
npm run tauri build
```
The compiled executable and `.msi` / `.exe` installer will be generated in `src-tauri/target/release/bundle/`.

---

## How to Use

1. Connect your iPhone to your PC via a reliable USB-C or Lightning cable.
2. Unlock your iPhone and tap **"Trust This Computer"** if prompted.
3. Launch **iPhone Backup Sync**. Your device will appear in the top status bar (click **Refresh** if connected after launch).
4. Select your backup destination folder (click **Browse...** to pick any internal, external, or network drive).
5. Click **Scan iPhone**. The app will recursively traverse your DCIM library and classify all media against your destination folder.
6. Click **Start Sync** to begin streamed transfer with live progress and transfer rate (MB/s).
7. If any file hangs or you wish to bypass it, press <kbd>S</kbd> to skip it immediately.

---

## Contributing

Contributions, bug reports, and suggestions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before submitting a pull request.

To run code-quality checks locally:
```bash
# Frontend type check & build
bun run build    # or: npm run build

# Rust formatting & linter
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

---

## 💬 Feedback & Early Stage Notice

This is an **early-stage release (`v0.1.0`)** built around real-world personal needs. There is plenty of room to grow—such as side-by-side media previews, multi-device profiles, and broader Android/camera MTP optimizations.

If you try it out and have suggestions, run into edge cases, or want to contribute improvements, please **open an Issue or submit a Pull Request**. Constructive feedback is genuinely welcome!

---

## Security & Privacy

This application is strictly local and communicates solely with your attached device over USB and your local filesystem. Read [SECURITY.md](SECURITY.md) for details on our vulnerability disclosure policy and local data handling.

---

## 🙏 Acknowledgments

This application was architected and developed with AI-assisted engineering workflows powered by **Gemini** and **Antigravity**.

---

## Author & License

Created by **Mahdi Salek**:
- **GitHub**: [@mahdis24](https://github.com/mahdis24)
- **LinkedIn**: [mahdi-s24](https://www.linkedin.com/in/mahdi-s24/)
- **Website**: [mahdis24.tech](https://mahdis24.tech)

Released under the [MIT License](LICENSE).
