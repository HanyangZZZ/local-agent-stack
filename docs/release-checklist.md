# Release checklist

## Prepare

- Confirm the changelog and compatibility manifest describe the release.
- Keep root, desktop, Tauri, Cargo workspace and updater versions synchronized.
- Review dependencies and the diff for credentials, local paths and generated files.
- Run `pnpm install --frozen-lockfile` and the complete contribution test gate.
- Exercise stack start, authenticated Harness launch, stop, restart, GPU release,
  managed-runtime rollback and one Ultra Trace replay on Windows.

## Publish

- Create the exact `v<version>` tag expected by the release workflow.
- Confirm GitHub Actions produces the NSIS installer, updater signature and release.
- Install the release artifact on a clean Windows user profile.
- Verify the published SHA/signature metadata and in-app updater discovery.

## Afterward

- Confirm the release page contains known limitations and rollback instructions.
- Move completed changelog entries out of Unreleased.
- Open follow-up issues for failures; never silently replace a published artifact.
