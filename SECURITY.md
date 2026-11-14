# Security Policy

## Privacy & Local Operation Guarantee

**iPhone Backup Sync** is designed with privacy and data integrity as foundational principles:
- **Zero Telemetry**: The application contains no analytics, tracking beacons, error reporting servers, or network calls.
- **100% Local & Offline**: All communication occurs exclusively over local USB through the Windows Portable Devices (WPD) COM subsystem and your local storage media.
- **Non-Destructive Operations**: Backups never overwrite size-mismatched files automatically, ensuring previous assets are protected from accidental truncation or corruption.

---

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| 0.3.x   | :white_check_mark: |
| < 0.3   | :x:                |

---

## Reporting a Vulnerability

If you discover a security vulnerability or critical integrity flaw within iPhone Backup Sync, please report it responsibly:

1. **Do not create a public GitHub issue.**
2. Send an email to **mahdi.6n@gmail.com** with the subject `[SECURITY] iPhone Backup Sync Vulnerability`.
3. Provide a detailed summary including:
   - Nature of the vulnerability (e.g., path traversal, privilege escalation, memory safety).
   - Steps to reproduce or proof-of-concept.
   - Potential impact on local files or devices.
4. You will receive an acknowledgment within 48 hours. We will work with you to understand and resolve the issue before publishing a security advisory and patch.
