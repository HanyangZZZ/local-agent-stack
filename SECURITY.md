# Security Policy

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting feature rather than a public
issue. Include the affected version, operating system, reproduction steps and
the security impact.

## Current security boundary

Version 0.1 is intended for a single user on one machine. Management targets
must use loopback addresses. The application does not authenticate or secure
remote Ollama or Harness instances and must not be exposed to an untrusted
network.

The application stops only child processes it launched and still owns. It does
not terminate a process by port or name alone.

