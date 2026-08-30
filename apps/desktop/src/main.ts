import { invoke } from "@tauri-apps/api/core";
import "./styles.css";
import type { ActionResult, ServiceKind, ServiceSnapshot, StackConfig, StackSnapshot } from "./types";

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("Application root is missing");

let snapshot: StackSnapshot | undefined;
let config: StackConfig | undefined;
let busy = false;
let workspaceVisible = false;

app.innerHTML = `
  <header class="topbar">
    <div class="brand">
      <div class="mark">LS</div>
      <div><strong>Local Agent Stack</strong><span>Local AI control plane</span></div>
    </div>
    <nav>
      <button class="nav active" data-view="dashboard">Control center</button>
      <button class="nav" data-view="workspace">Harness workspace</button>
    </nav>
    <button class="icon-button" id="settings-button" title="Settings">⚙</button>
  </header>
  <main>
    <section id="dashboard-view">
      <div class="hero">
        <div><p class="eyebrow">SYSTEM OVERVIEW</p><h1>Your local agent stack,<br><em>under control.</em></h1></div>
        <div class="hero-actions"><button class="secondary" id="refresh-button">Refresh</button><button class="primary" id="launch-button">Open Harness</button></div>
      </div>
      <div id="notice" class="notice hidden"></div>
      <div class="service-grid" id="service-grid"><div class="skeleton"></div><div class="skeleton"></div></div>
      <section class="panel">
        <div class="panel-heading"><div><p class="eyebrow">MODEL LIBRARY</p><h2>Ollama models</h2></div><form id="pull-form"><input id="model-name" placeholder="e.g. qwen3:8b" autocomplete="off"><button class="primary" type="submit">Pull model</button></form></div>
        <div id="models" class="models"><p class="muted">Checking Ollama…</p></div>
      </section>
      <footer><span id="config-path"></span><span>Apache-2.0 · v0.1.0</span></footer>
    </section>
    <section id="workspace-view" class="workspace hidden"><div class="workspace-empty"><h2>Harness is not available</h2><p>Start Harness from the control center, then refresh this workspace.</p></div></section>
  </main>
  <dialog id="settings-dialog">
    <form method="dialog" id="settings-form">
      <div class="dialog-heading"><div><p class="eyebrow">LOCAL CONFIGURATION</p><h2>Runtime settings</h2></div><button class="icon-button" value="cancel">×</button></div>
      <label>Ollama URL<input id="ollama-url" required></label>
      <label>Ollama executable<input id="ollama-command" placeholder="Auto-detect"></label>
      <label>Harness URL<input id="harness-url" required></label>
      <label>Harness executable<input id="harness-command" placeholder="Auto-detect dsh"></label>
      <label>Harness arguments<input id="harness-args" placeholder="web --port 3000"></label>
      <label>Managed profile<input id="harness-profile" required></label>
      <p class="hint">Version 0.1 accepts loopback URLs only. Arguments are separated by spaces; quoted argument editing is planned.</p>
      <div class="dialog-actions"><button value="cancel" class="secondary">Cancel</button><button value="default" class="primary" id="save-settings">Save settings</button></div>
    </form>
  </dialog>
`;

const $ = <T extends Element>(selector: string): T => {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Missing element: ${selector}`);
  return element;
};

function bytes(value: number): string {
  if (!value) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)), units.length - 1);
  return `${(value / 1024 ** index).toFixed(index > 2 ? 1 : 0)} ${units[index]}`;
}

function serviceCard(service: ServiceSnapshot): string {
  const canStop = service.managed;
  const online = service.state === "online";
  return `<article class="service-card">
    <div class="service-title"><div class="service-icon ${service.kind}">${service.kind === "ollama" ? "O" : "H"}</div><div><h2>${service.kind === "ollama" ? "Ollama" : "DeepSeek Harness"}</h2><p>${service.url}</p></div><span class="status ${service.state}"><i></i>${service.state}</span></div>
    <div class="facts"><div><span>VERSION</span><strong>${service.version ?? "—"}</strong></div><div><span>OWNERSHIP</span><strong>${service.managed ? `Managed · PID ${service.pid}` : online ? "External" : "Not running"}</strong></div></div>
    <div class="card-actions">
      <button class="primary service-action" data-action="start" data-service="${service.kind}" ${online ? "disabled" : ""}>Start</button>
      <button class="secondary service-action" data-action="restart" data-service="${service.kind}" ${!canStop ? "disabled" : ""}>Restart</button>
      <button class="danger service-action" data-action="stop" data-service="${service.kind}" ${!canStop ? "disabled" : ""}>Stop</button>
    </div>
  </article>`;
}

function render(): void {
  if (!snapshot) return;
  $("#service-grid").innerHTML = serviceCard(snapshot.ollama) + serviceCard(snapshot.harness);
  $("#config-path").textContent = `Config: ${snapshot.configPath}`;

  const running = new Map(snapshot.runningModels.map((model) => [model.name, model]));
  $("#models").innerHTML = snapshot.installedModels.length
    ? snapshot.installedModels.map((model) => {
        const active = running.get(model.name);
        return `<div class="model-row"><div class="model-main"><span class="model-dot ${active ? "active" : ""}"></span><div><strong>${model.name}</strong><span>${[model.parameterSize, model.quantizationLevel, bytes(model.size)].filter(Boolean).join(" · ")}</span></div></div><div class="model-runtime">${active ? `<strong>${bytes(active.sizeVram)} VRAM</strong><span>${active.contextLength ? `${active.contextLength.toLocaleString()} context` : "Loaded"}</span>` : "<span>Not loaded</span>"}</div><div class="row-actions">${active ? `<button class="secondary model-action" data-action="unload" data-model="${escapeAttribute(model.name)}">Release VRAM</button>` : ""}<button class="ghost danger-text model-action" data-action="delete" data-model="${escapeAttribute(model.name)}">Delete</button></div></div>`;
      }).join("")
    : `<div class="empty"><strong>${snapshot.ollama.state === "online" ? "No models installed" : "Ollama is offline"}</strong><span>${snapshot.ollama.state === "online" ? "Pull a model above to get started." : "Start Ollama to view and manage models."}</span></div>`;

  document.querySelectorAll<HTMLButtonElement>(".service-action").forEach((button) => button.addEventListener("click", () => serviceAction(button)));
  document.querySelectorAll<HTMLButtonElement>(".model-action").forEach((button) => button.addEventListener("click", () => modelAction(button)));
  updateWorkspace();
}

function escapeAttribute(value: string): string {
  return value.replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;");
}

async function refresh(): Promise<void> {
  try {
    snapshot = await invoke<StackSnapshot>("get_snapshot");
    config ??= await invoke<StackConfig>("get_config");
    render();
  } catch (error) {
    notify(String(error), true);
  }
}

async function runAction(action: () => Promise<ActionResult>): Promise<void> {
  if (busy) return;
  busy = true;
  document.body.classList.add("busy");
  try {
    const result = await action();
    notify(result.message, !result.ok);
    await refresh();
  } catch (error) {
    notify(String(error), true);
  } finally {
    busy = false;
    document.body.classList.remove("busy");
  }
}

async function serviceAction(button: HTMLButtonElement): Promise<void> {
  const service = button.dataset.service as ServiceKind;
  const action = button.dataset.action;
  await runAction(() => invoke<ActionResult>(`${action}_service`, { service }));
}

async function modelAction(button: HTMLButtonElement): Promise<void> {
  const model = button.dataset.model ?? "";
  if (button.dataset.action === "delete" && !window.confirm(`Delete ${model} from Ollama?`)) return;
  const command = button.dataset.action === "unload" ? "unload_model" : "delete_model";
  await runAction(() => invoke<ActionResult>(command, { model }));
}

function notify(message: string, error = false): void {
  const notice = $("#notice");
  notice.textContent = message;
  notice.className = `notice ${error ? "error" : "success"}`;
  window.setTimeout(() => notice.classList.add("hidden"), 6000);
}

function updateWorkspace(): void {
  if (!snapshot || !workspaceVisible) return;
  const view = $("#workspace-view");
  if (snapshot.harness.state === "online") {
    const current = view.querySelector<HTMLIFrameElement>("iframe");
    if (!current || current.src !== snapshot.harness.url + "/") {
      view.innerHTML = `<iframe src="${escapeAttribute(snapshot.harness.url)}" title="DeepSeek Harness"></iframe>`;
    }
  } else {
    view.innerHTML = `<div class="workspace-empty"><h2>Harness is not available</h2><p>Start Harness from the control center, then refresh this workspace.</p></div>`;
  }
}

function selectView(name: string): void {
  workspaceVisible = name === "workspace";
  $("#dashboard-view").classList.toggle("hidden", workspaceVisible);
  $("#workspace-view").classList.toggle("hidden", !workspaceVisible);
  document.querySelectorAll(".nav").forEach((item) => item.classList.toggle("active", (item as HTMLElement).dataset.view === name));
  updateWorkspace();
}

$("#refresh-button").addEventListener("click", refresh);
$("#launch-button").addEventListener("click", () => selectView("workspace"));
document.querySelectorAll<HTMLButtonElement>(".nav").forEach((button) => button.addEventListener("click", () => selectView(button.dataset.view ?? "dashboard")));

$("#pull-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const input = $("#model-name") as HTMLInputElement;
  const model = input.value.trim();
  if (!model) return;
  await runAction(() => invoke<ActionResult>("pull_model", { model }));
  input.value = "";
});

const dialog = $("#settings-dialog") as HTMLDialogElement;
$("#settings-button").addEventListener("click", async () => {
  config = await invoke<StackConfig>("get_config");
  ($("#ollama-url") as HTMLInputElement).value = config.ollama.url;
  ($("#ollama-command") as HTMLInputElement).value = config.ollama.command ?? "";
  ($("#harness-url") as HTMLInputElement).value = config.harness.url;
  ($("#harness-command") as HTMLInputElement).value = config.harness.command ?? "";
  ($("#harness-args") as HTMLInputElement).value = config.harness.args.join(" ");
  ($("#harness-profile") as HTMLInputElement).value = config.harnessProfile;
  dialog.showModal();
});

$("#settings-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!config) return;
  const updated: StackConfig = {
    ollama: { ...config.ollama, url: ($("#ollama-url") as HTMLInputElement).value.trim(), command: ($("#ollama-command") as HTMLInputElement).value.trim() || undefined },
    harness: { ...config.harness, url: ($("#harness-url") as HTMLInputElement).value.trim(), command: ($("#harness-command") as HTMLInputElement).value.trim() || undefined, args: ($("#harness-args") as HTMLInputElement).value.trim().split(/\s+/).filter(Boolean) },
    harnessProfile: ($("#harness-profile") as HTMLInputElement).value.trim(),
  };
  await runAction(() => invoke<ActionResult>("save_config", { config: updated }));
  config = updated;
  dialog.close();
});

void refresh();
window.setInterval(() => { if (!busy) void refresh(); }, 10000);

