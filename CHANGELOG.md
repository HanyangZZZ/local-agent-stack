# Changelog

All notable project changes are documented here. The format is based on Keep a
Changelog, and versions follow Semantic Versioning while the project is in alpha.

## [Unreleased]

### Changed

- Reorganized development-only trace fixtures and added repository integrity
  checks for release versions, updater keys, manifests and common secret types.
- Aligned the published JSON schemas with the runtime configuration and
  compatibility manifest.

### Fixed

- Resolved strict Rust lint failures in supervisor cleanup and trace ordering.

## [0.1.1-alpha.5.1] - 2026-08-31

### Added

- Local desktop control of app-managed Ollama and DeepSeek Harness processes.
- Managed runtime install, validation, rollback and compatibility reporting.
- Ultra Trace timeline reconstruction with context, workflow, slot and GPU views.
- Signed desktop updater artifacts and automated release metadata publication.

[Unreleased]: https://github.com/HanyangZZZ/local-agent-stack/compare/v0.1.1-alpha.5.1...HEAD
[0.1.1-alpha.5.1]: https://github.com/HanyangZZZ/local-agent-stack/releases/tag/v0.1.1-alpha.5.1
