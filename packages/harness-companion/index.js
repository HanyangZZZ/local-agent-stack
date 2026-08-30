const name = "local-agent-stack-companion";
const inject = ["commands"];

const DEFAULTS = Object.freeze({
  controlUrl: "http://127.0.0.1:32145",
  ollamaUrl: "http://127.0.0.1:11434",
});

function loopbackUrl(value, label) {
  const url = new URL(value);
  const loopback = ["127.0.0.1", "localhost", "[::1]", "::1"].includes(url.hostname);
  if (!loopback || !["http:", "https:"].includes(url.protocol)) {
    throw new Error(`${label} must use an HTTP loopback URL`);
  }
  return url;
}

async function readJson(fetchImpl, url, timeoutMs = 1200) {
  const signal = AbortSignal.timeout(timeoutMs);
  const response = await fetchImpl(url, { signal });
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  return response.json();
}

async function queryStatus(config = {}, fetchImpl = globalThis.fetch) {
  const values = { ...DEFAULTS, ...config };
  const controlUrl = loopbackUrl(values.controlUrl, "controlUrl");
  const ollamaUrl = loopbackUrl(values.ollamaUrl, "ollamaUrl");

  try {
    const status = await readJson(fetchImpl, new URL("/v1/status", controlUrl));
    return renderControlStatus(status);
  } catch {
    // The desktop supervisor may not expose its authenticated bridge yet. A
    // read-only Ollama fallback keeps this command useful and never mutates a
    // process or model.
  }

  try {
    const running = await readJson(fetchImpl, new URL("/api/ps", ollamaUrl));
    const models = Array.isArray(running.models) ? running.models : [];
    const totalVram = models.reduce((sum, model) => sum + Number(model.size_vram || 0), 0);
    const lines = [
      "Local Agent Stack desktop: not connected",
      `Ollama: online · ${models.length} model${models.length === 1 ? "" : "s"} loaded · ${formatBytes(totalVram)} VRAM`,
    ];
    for (const model of models) {
      lines.push(`- ${model.name}: ${formatBytes(Number(model.size_vram || 0))} VRAM`);
    }
    return lines.join("\n");
  } catch {
    return "Local Agent Stack desktop: not connected\nOllama: offline\nOpen the Local Agent Stack desktop app to start or configure services.";
  }
}

function renderControlStatus(status) {
  const ollama = status?.ollama?.state ?? "unknown";
  const harness = status?.harness?.state ?? "unknown";
  const models = Array.isArray(status?.runningModels) ? status.runningModels : [];
  const totalVram = models.reduce((sum, model) => sum + Number(model.sizeVram || 0), 0);
  return [
    "Local Agent Stack desktop: connected",
    `Ollama: ${ollama}`,
    `Harness: ${harness}`,
    `Loaded models: ${models.length} · ${formatBytes(totalVram)} VRAM`,
  ].join("\n");
}

function formatBytes(value) {
  if (!Number.isFinite(value) || value <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  return `${(value / 1024 ** index).toFixed(index >= 3 ? 1 : 0)} ${units[index]}`;
}

function apply(ctx, config = {}) {
  ctx.effect(
    () =>
      ctx.commands.register({
        name: "local-stack",
        description: "show local model, runtime, and GPU-memory status",
        input: { hint: "[status]" },
        recordInput: false,
        handler: async (invocation) => {
          const input = invocation.rawInput.trim().toLowerCase();
          if (input && input !== "status") {
            return { kind: "error", text: "Usage: /local-stack [status]" };
          }
          try {
            return { kind: "success", text: await queryStatus(config) };
          } catch (error) {
            return {
              kind: "error",
              text: error instanceof Error ? error.message : "Local stack status failed",
            };
          }
        },
      }),
    "local-agent-stack: status command",
  );
}

export { apply, inject, name, queryStatus };

