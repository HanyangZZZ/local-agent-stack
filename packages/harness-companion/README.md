# Harness Companion

This is the optional DeepSeek Harness bundle for Local Agent Stack. It adds a
human `/local-stack` command that reports desktop-supervisor status and falls
back to Ollama's read-only running-model endpoint when the desktop bridge is not
available.

The bundle never starts, stops, downloads or deletes anything. Mutating actions
remain in the independently running desktop supervisor.

Development installation:

```powershell
dsh plugin --profile local-agent-stack add ./packages/harness-companion
dsh --profile local-agent-stack --dump-config
```

