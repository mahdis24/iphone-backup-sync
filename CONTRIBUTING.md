# Contributing to iPhone Backup Sync

Thank you for your interest in contributing to **iPhone Backup Sync**! We appreciate bug reports, feature suggestions, and code contributions that help make Windows-iOS media synchronization fast, transparent, and resilient.

---

## Code of Conduct

All contributors and participants are expected to adhere to our [Code of Conduct](CODE_OF_CONDUCT.md).

---

## How Can I Contribute?

### Reporting Bugs
Before filing an issue, check existing GitHub Issues to see if the problem has already been reported. When reporting a bug, please include:
- Windows version and build (e.g. Windows 11 23H2).
- iOS device model and iOS version (e.g. iPhone 15 Pro, iOS 18.1).
- Transfer settings (e.g. "Keep Originals" status in Settings > Photos, and whether iCloud Photos is set to "Download and Keep Originals" or "Optimize iPhone Storage").
- Steps to reproduce the issue.
- Relevant error logs or UI messages.

### Requesting Features
Open an issue using the Feature Request template detailing:
- The problem you are trying to solve.
- Your proposed solution or enhancement.
- Any alternative approaches considered.

### Submitting Pull Requests
1. **Fork the Repository**: Clone your fork locally.
2. **Create a Feature Branch**:
   ```bash
   git checkout -b feature/your-feature-name
   # or
   git checkout -b fix/issue-description
   ```
3. **Commit Cleanly**: Write descriptive, concise commit messages.
4. **Run Local Verification**:
   Ensure all checks pass before pushing:
   ```bash
   # Frontend type check & bundle
   bun run build    # or: npm run build

   # Rust formatting check
   cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

   # Rust strict clippy checks
   cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

   # Rust unit tests
   cargo test --manifest-path src-tauri/Cargo.toml
   ```
5. **Open a PR**: Submit a pull request targeting the `main` branch and fill out the pull request template.

---

## Development Guidelines

- **Zero Telemetry**: Never introduce network analytics, trackers, or external phone-home requests. This project is 100% offline.
- **Safety First**: Never overwrite user backup files on disk when byte sizes differ. Always utilize non-destructive renaming (e.g., `<stem>_new.<ext>`).
- **COM & Windows API**: Keep COM initialization and uninitialization enclosed within `wpd::ComGuard` or safe RAII abstractions, and wrap active MTP device sessions in `wpd::WpdDeviceGuard` to guarantee scoped cleanup with `device.Close()` so devices are never locked.
- **TypeScript & Serde Synchronization**: Ensure all structs serialized across the Tauri IPC boundary in `src-tauri/src/` have matching interface definitions in `src/types.ts`.
