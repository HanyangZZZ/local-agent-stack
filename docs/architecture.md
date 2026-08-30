# Architecture

## Product boundary

Local Agent Stack is a control plane. It does not implement inference, replace
the Harness agent loop, or silently take ownership of installations created by
other tools.

The desktop application and optional CLI share `local-stack-core`. Runtime
integrations are adapters behind typed Rust interfaces. The future Harness
bundle is a thin user-interface bridge to the independent local supervisor, so
the supervisor remains usable when Harness is stopped or broken.

## Bring-your-own and managed installations

Version 0.1 implements bring-your-own installation discovery, an app-owned
Harness import, and a verified app-owned Ollama installer. The Harness import
copies an already installed, manifest-tested package and Node executable into a
unique release directory, validates the copied CLI, atomically activates a state
pointer, and keeps the previous release for rollback. The external installation
remains untouched.

The Ollama installer selects an OS/architecture-specific artifact from the
embedded release manifest, requires an allowlisted official HTTPS origin,
preflights disk space, streams the archive while hashing it, and rejects size or
SHA-256 mismatches. Extraction rejects path traversal and symbolic links,
enforces an extracted-size ceiling, and writes into a unique staging directory.
The staged executable must report the expected version before the same atomic
release-pointer mechanism can activate it. Download, validation, or extraction
failure cannot replace the current runtime.

Managed runtime staging accepts only direct children of the app-owned staging
root. Executable paths and release identifiers are validated as relative path
components before use. Unsupported links or special files abort an import, and
failed validation removes only the transaction's staging directory. Stale
app-owned staging transactions are cleaned up on a later launch without touching
active or previous releases.

## Service lifecycle

The supervisor records child handles for processes it launches. Stop and
restart operations are accepted only for those children. A service discovered
through an HTTP health check is shown as external and is never terminated using
port-based or process-name-based guesses.

Long-term reattachment will use a persisted process record containing the PID,
executable identity, creation timestamp and per-launch nonce. All fields must be
verified before termination.

## Configuration transactions

Harness configuration work will use a dedicated profile. A configuration
transaction follows this sequence:

1. Read and validate the desired manifest.
2. Snapshot every file that may change.
3. Write changes to temporary files in the same filesystem.
4. Atomically replace the target files.
5. run `dsh --profile <profile> --dump-config`.
6. perform a health and inference smoke test.
7. restore the snapshot if validation fails.

The stock Harness profiles are never modified by default.

The initial profile bootstrap clones the selected `web` composition into a
uniquely named temporary sibling. Harness must successfully run
`--dump-config` against that temporary profile before it is renamed into place.
If validation fails, only the newly created temporary directory is removed; an
existing target profile is never overwritten.

## Local security

- Bind management endpoints to loopback or use OS-local IPC.
- Use a random per-install secret for any HTTP bridge.
- Expose typed, allowlisted operations; never expose an arbitrary shell RPC.
- Redact secrets and user prompts from diagnostic bundles.
- Require an explicit user action before downloads, upgrades or process stops.

## Compatibility

Desktop, runtime adapter and Harness companion versions are independent. A
signed compatibility manifest will state tested Harness and Ollama ranges. CI
will test the current release, previous release and upstream development build
where practical.

The first embedded manifest is checked into `manifests/compatibility.json` and
ships with the desktop binary. Runtime versions are compared locally using
semantic-version ranges; an unknown or newer version produces a warning rather
than an automatic mutation. A future remotely fetched manifest must be signed
and verified before it can influence installation or upgrade actions.
