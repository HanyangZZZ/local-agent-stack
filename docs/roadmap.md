# Roadmap

## 0.1 — Windows/NVIDIA foundation

- [x] Independent Tauri desktop shell.
- [x] Typed configuration and service snapshots.
- [x] Ollama version, model and running-model discovery.
- [x] Ollama pull, delete and immediate unload operations.
- [x] One-click release of GPU memory used by every loaded Ollama model.
- [x] Transactional stack start and ownership-safe managed stack shutdown.
- [x] Bounded, local-only service log viewer with missing-log and truncation handling.
- [x] Managed Ollama and Harness child-process start/stop/restart.
- [x] Authenticated first-party Harness application window.
- [x] Streaming model-pull progress.
- [x] NVIDIA driver and GPU diagnostics.
- [x] Redacted diagnostic bundle export.
- [x] Playable Ultra Trace reconstruction for model I/O, contexts, agents, workflows, queues, slots, and GPU telemetry.
- [ ] Signed Windows installer.

## 0.2 — Safe Harness configuration

- [x] Dedicated managed Harness profile.
- [x] Transactional profile bootstrap with Harness-native validation and cleanup.
- [x] Prebuilt, versioned Harness companion bundle with one-click installation.
- [x] Version compatibility manifest and upgrade warnings.
- [x] Guided first-run setup.

## 0.3 — Managed installations and more platforms

- [x] App-owned, versioned Harness import with validation and rollback.
- [x] App-owned, pinned Ollama installation.
- [x] Verified runtime downloads, secure extraction and atomic activation.
- [x] Managed-runtime rollback and stale staging cleanup.
- [x] Background system-tray supervisor with single-instance relaunch.
- macOS and Linux builds.
- [x] Verified PID reattachment after unexpected supervisor termination.

## 0.4 — Trusted distribution

- [x] Signature-enforced desktop updater with an isolated release key.
- [x] Signed updater artifacts and automated release manifest publication.
- Signed compatibility/runtime-artifact manifests with key rotation.
- Signed Windows installers and binaries.
- In-app update channels and richer release notes.
- Download cancellation and resumable large runtime payloads.

## Later

- llama.cpp, LM Studio and other runtime adapters.
- Community adapter SDK with signed metadata.
- Import/exportable stack presets.
- Headless CLI and remote management over an explicitly enabled secure channel.
