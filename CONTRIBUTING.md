# Contributing

Thank you for helping make local AI deployment easier.

## Before you start

- Search existing issues and pull requests before opening a duplicate.
- Use an issue to discuss large behavioral, security or architectural changes.
- Keep each pull request focused and explain user-visible behavior and risk.
- Do not commit models, runtime downloads, build output, local configuration,
  session traces, credentials or diagnostic archives.

## Development workflow

1. Install Node.js 22+, pnpm 11, Rust stable with MSVC on Windows, and WebView2.
2. Run `pnpm install --frozen-lockfile`.
3. Keep runtime-specific behavior behind an adapter boundary.
4. Add tests for configuration, process identity, API responses and failure
   rollback when those areas change.
5. Run the complete local gate:

   ```powershell
   pnpm check
   pnpm format:check
   pnpm lint
   pnpm test
   pnpm --filter @local-agent-stack/desktop build
   pnpm audit --audit-level high
   ```

6. Update the changelog, schemas and architecture documentation when the public
   behavior or data model changes.

The main boundaries are `apps/desktop`, `crates/local-stack-core`, and
`packages/harness-companion`. Development-only UI fixtures belong under
`apps/desktop/src/dev`; production code must not import them.

## Pull requests

Describe the motivation, implementation, verification and any security or
rollback implications. Include screenshots for material UI changes. Generated
installer artifacts belong in GitHub Releases, not in Git.

Never add an operation that accepts arbitrary shell text from the UI. New
privileged operations must have typed inputs, validation and a documented threat
model.

By submitting a contribution, you agree that it is licensed under Apache-2.0.
