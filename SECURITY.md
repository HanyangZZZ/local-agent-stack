# Security Policy

## Supported versions

Only the latest published alpha receives security fixes. Older builds and
source snapshots are unsupported; reproduce reports against the latest release
when possible.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting feature rather than a public
issue. Include the affected version, operating system, reproduction steps and
the security impact. Do not attach credentials, private prompts, model files or
unredacted session archives. We will acknowledge the report in GitHub and
coordinate disclosure after a fix is available.

## Current security boundary

Version 0.1 is intended for a single user on one machine. Management targets
must use loopback addresses. The application does not authenticate or secure
remote Ollama or Harness instances and must not be exposed to an untrusted
network.

The application stops only child processes it launched and still owns. It does
not terminate a process by port or name alone.

The updater verifies project signatures, but alpha Windows installers do not
yet carry an Authenticode publisher signature. Verify that downloads come from
this repository's GitHub Releases page.
