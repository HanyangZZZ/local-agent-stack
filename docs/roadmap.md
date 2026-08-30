# Roadmap

## 0.1 — Windows/NVIDIA foundation

- [x] Independent Tauri desktop shell.
- [x] Typed configuration and service snapshots.
- [x] Ollama version, model and running-model discovery.
- [x] Ollama pull, delete and immediate unload operations.
- [x] Managed Ollama and Harness child-process start/stop/restart.
- [x] Embedded Harness workspace.
- [x] Streaming model-pull progress.
- [x] NVIDIA driver and GPU diagnostics.
- [ ] Redacted diagnostic bundle export.
- [ ] Signed Windows installer.

## 0.2 — Safe Harness configuration

- [x] Dedicated managed Harness profile.
- [x] Transactional profile bootstrap with Harness-native validation and cleanup.
- Prebuilt Harness companion bundle.
- Version compatibility manifest and upgrade warnings.
- Guided first-run setup.

## 0.3 — Managed installations and more platforms

- App-owned, pinned Ollama and Harness installations.
- Verified downloads, atomic upgrades and rollback.
- macOS and Linux builds.
- Background tray supervisor and verified PID reattachment.

## Later

- llama.cpp, LM Studio and other runtime adapters.
- Community adapter SDK with signed metadata.
- Import/exportable stack presets.
- Headless CLI and remote management over an explicitly enabled secure channel.
