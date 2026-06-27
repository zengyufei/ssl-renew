# Changelog

All notable changes to this project are documented here.

## [0.1.0] - 2026-06-27

### Added

- Initial Rust workspace with shared `ssl-core`, CLI, signer agent, and Tauri GUI.
- Multi-domain certificate profiles with DNS-01 renewal flow.
- Manual DNS, Aliyun, Cloudflare, and restricted signer-agent DNS modes.
- Certificate checking, ACME order creation, DNS propagation checks, certificate issuing, Nginx restart, and scheduled monitoring.
- GUI settings for theme, language, toast, logs, notifications, signer setup, and profile import/export.
- UPX-based Windows release packaging script.

### Changed

- Public project brand is unified as `SSL证书自动续期` with the English subtitle `SSL Certificate Auto Renewal`.
- Release artifacts are packaged as a Windows x64 zip instead of loose executables or rar archives.

### Fixed

- Repository hygiene now excludes generated builds, runtime state, logs, local profiles, certificates, private keys, environment files, and signer secret files.

### Known limitations

- The first public release targets Windows x64.
- The exe is unsigned, so Windows or antivirus software may show a warning.
- Manual DNS mode cannot run fully unattended because TXT records must be created by the user.
- `cargo test --workspace` is not used as the first CI gate because the Tauri build script currently fails in workspace test mode on this project; core, CLI, and signer tests are run separately.
