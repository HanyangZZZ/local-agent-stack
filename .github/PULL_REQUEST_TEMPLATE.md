## Why

Describe the user problem and why this change belongs in Local Agent Stack.

## What changed

Summarize the implementation and its architecture boundaries.

## Verification

- [ ] `pnpm check`
- [ ] `pnpm format:check`
- [ ] `pnpm lint`
- [ ] `pnpm test`
- [ ] `pnpm --filter @local-agent-stack/desktop build`
- [ ] UI changes include screenshots or a short recording

## Safety and compatibility

- [ ] No credentials, model files, traces, local configuration or build output are committed
- [ ] Process-control and download changes use typed validation and safe rollback
- [ ] Public behavior, schemas and changelog are updated where necessary
