# Contributing

Thank you for helping make local AI deployment easier.

## Development workflow

1. Open an issue before undertaking a large behavioral or architectural change.
2. Keep runtime-specific behavior behind an adapter boundary.
3. Add tests for configuration, process identity and API-response handling.
4. Run `pnpm check`, `cargo fmt --all -- --check`,
   `cargo clippy --workspace --all-targets -- -D warnings` and
   `cargo test --workspace` before opening a pull request.

Never add an operation that accepts arbitrary shell text from the UI. New
privileged operations must have typed inputs, validation and a documented threat
model.

By submitting a contribution, you agree that it is licensed under Apache-2.0.

