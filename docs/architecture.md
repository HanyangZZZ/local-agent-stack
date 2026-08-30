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

Version 0.1 implements bring-your-own installation discovery. A later managed
mode may install pinned versions into an app-owned data directory. Both modes
must use the same adapter interface and declarative `StackConfig` schema.

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
