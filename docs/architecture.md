# Architecture

## Product boundary

Local Agent Stack is a control plane. It does not implement inference, replace
the Harness agent loop, or silently take ownership of installations created by
other tools.

The desktop application and optional CLI share `local-stack-core`. Runtime
integrations are adapters behind typed Rust interfaces. The future Harness
bundle is a thin user-interface bridge to the independent local supervisor, so
the supervisor remains usable when Harness is stopped or broken.

Ultra Trace belongs to the independent desktop control plane. Harness remains
the authority for model-visible inputs, completed model output, tool calls,
subagent lineage, and workflow lifecycle records. The recorder reads those
append-only JSONL or Zstandard JSONL artifacts without rewriting them, joins
descendant sessions, and derives a replay projection for the desktop. This is
not a Harness UI fork and does not require a permanently loaded plugin.

The optional Harness companion remains a thin status command. A later push
adapter may reduce live-view latency, but it must feed the same recorder schema;
the durable Harness log remains the recovery source when either process was
stopped.

## Ultra Trace recorder

The replay projection is lossless but uses logical-operation granularity. A set
of human and Harness-injected `user/message` records becomes one input node; a
request header, context declaration, streaming fragments, completed response,
and step lifecycle become one model-execution node; a tool call and its matching
result become one tool-execution node. Workflow routing events remain explicit
because they change the agent graph. Every original record—including streaming
reasoning/text/tool-call fragments and routine turn bookkeeping—remains attached
to its logical node and is available in folded sections. The detail panel
separates the human message, injected runtime context, skill catalog, system
prompt, model and adapter configuration, tool/MCP schemas, messages, reasoning,
output, usage, lifecycle metadata, and complete raw records.

The viewer advances through semantic scenes rather than individual nodes. It
topologically layers causal edges so parallel fan-out, tool work, reports, and
slot changes can appear together at one playhead step. All known lanes are laid
out consistently and transition from not-created to active to history-only.
This keeps the graph spatially stable while agents and workflows dynamically
enter, leave, and reuse physical inference slots.

Context lineage owns graph color. A spawned session receives its own context
identity; a fork reuses the parent context identity and adds a striped visual
treatment. Durable `tool-workflow/run-start`, member start/end, and run-end
records create workflow branches and membership/report edges. Removed branches
leave active slots but remain visible as history.

Inference slots and queues are a derived projection. Each `request/header` to
`assistant/message` interval becomes a lease, and overlapping intervals are
assigned to the configured slot count in request order. A request that cannot
start until an earlier interval ends appears in the queue. Ollama does not
publish its physical scheduler queue, so the UI labels this state as derived
instead of claiming provider-internal precision.

While the desktop is open, a background sampler records NVIDIA VRAM and GPU
utilization plus loaded Ollama models to day-partitioned, local JSONL files.
Historical sessions created before the recorder ran correctly show telemetry as
unavailable rather than fabricating values. Trace recording is configurable and
never enters diagnostic exports.

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

The supervisor persists one process record per app-owned service. Each record
contains the PID, executable, complete launch arguments, operating-system
creation timestamp, and the authenticated Harness launch URL when applicable.
Every snapshot reconciles the record against the live operating-system process;
a stale record is removed, and a stop is refused unless identity still matches.

After an application crash or update, an exact process from an app-owned
versioned runtime can be reattached when its executable and command line match
the configured launch. Port ownership and process names are never used as a
termination guess. Independently installed processes remain external and are
never stopped by the application.

Harness browser authentication is a separate lifecycle artifact. The
supervisor captures the per-process URL printed by `dsh web` only from output
written after that process was spawned and persists it alongside the verified
process identity. The desktop opens Harness as a first-party WebView window so
its strict, authority-bound authentication cookie works as designed. The
control panel never weakens Harness authentication or proxies its API.

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
- Never include the Harness launch token or process registry in diagnostics.
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
