import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import "./styles.css";
import type { ActionResult, AppUpdateMetadata, AppUpdateProgress, PullProgress, RuntimeInstallProgress, ServiceKind, ServiceSnapshot, StackConfig, StackSnapshot } from "./types";

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) throw new Error("Application root is missing");

let snapshot: StackSnapshot | undefined;
let config: StackConfig | undefined;
let busy = false;
let workspaceVisible = false;
let onboardingShown = false;
let setupVerified = false;
let noticeTimer: number | undefined;

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
        <div class="hero-actions"><button class="secondary" id="update-button">Check for updates</button><button class="secondary" id="diagnostics-button">Export diagnostics</button><button class="secondary" id="refresh-button">Refresh</button><button class="primary" id="launch-button">Open Harness</button></div>
      </div>
      <div id="notice" class="notice hidden"></div>
      <div class="environment-strip" id="environment-strip"><span>Inspecting local environment…</span></div>
      <div class="compatibility-strip" id="compatibility-strip"></div>
      <div class="runtime-management" id="runtime-management"></div>
      <div class="service-grid" id="service-grid"><div class="skeleton"></div><div class="skeleton"></div></div>
      <section class="panel">
        <div class="panel-heading"><div><p class="eyebrow">MODEL LIBRARY</p><h2>Ollama models</h2></div><form id="pull-form"><input id="model-name" placeholder="e.g. qwen3:8b" autocomplete="off"><button class="primary" type="submit">Pull model</button></form></div>
        <div id="models" class="models"><p class="muted">Checking Ollama…</p></div>
      </section>
      <footer><span id="config-path"></span><span>Apache-2.0 · <span id="app-version">version…</span></span></footer>
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
      <label>Harness home<input id="harness-home" placeholder="Auto-detect ~/.dsh"></label>
      <label>Harness arguments<input id="harness-args" placeholder="web --port 3000"></label>
      <label>Managed profile<input id="harness-profile" required></label>
      <p class="hint">Version 0.1 accepts loopback URLs only. Arguments are separated by spaces; quoted argument editing is planned.</p>
      <div class="dialog-actions"><button value="cancel" class="secondary">Cancel</button><button value="default" class="primary" id="save-settings">Save settings</button></div>
    </form>
  </dialog>
  <dialog id="setup-dialog" class="setup-dialog">
    <div class="setup-content">
      <div class="setup-heading"><div class="mark">LS</div><div><p class="eyebrow">FIRST-RUN SETUP</p><h2>Connect your local agent stack</h2></div></div>
      <p class="setup-intro">Local Agent Stack can adopt independently installed runtimes or keep verified app-owned releases. This checklist prepares both services and verifies the local endpoints.</p>
      <div class="setup-summary" id="setup-summary">Inspecting this computer…</div>
      <div class="setup-steps">
        <article><span>1</span><div><strong>Review runtime settings</strong><p>Confirm loopback URLs, executable paths, and Harness home.</p><button class="secondary" id="setup-settings" type="button">Open settings</button></div></article>
        <article><span>2</span><div><strong>Install verified Ollama</strong><p>Download the official 1.36 GB Windows archive, verify SHA-256, and activate it in app-owned storage.</p><button class="secondary" id="setup-ollama" type="button">Install managed Ollama</button><small id="setup-ollama-result"></small></div></article>
        <article><span>3</span><div><strong>Install managed Harness</strong><p>Import the tested release into app-owned storage. Your external installation remains untouched.</p><button class="secondary" id="setup-managed" type="button">Import managed Harness</button><small id="setup-managed-result"></small></div></article>
        <article><span>4</span><div><strong>Prepare Harness profile</strong><p>Clone and validate an isolated profile without modifying the stock web profile.</p><button class="secondary" id="setup-profile" type="button">Prepare profile</button><small id="setup-profile-result"></small></div></article>
        <article><span>5</span><div><strong>Install companion</strong><p>Add the versioned read-only <code>/local-stack</code> command bundle.</p><button class="secondary" id="setup-companion" type="button">Install companion</button><small id="setup-companion-result"></small></div></article>
        <article><span>6</span><div><strong>Verify health</strong><p>Refresh service, GPU, model, and local toolchain state.</p><button class="primary" id="setup-verify" type="button">Run health check</button><small id="setup-verify-result"></small></div></article>
      </div>
      <div class="dialog-actions"><button class="secondary" id="setup-later" type="button">Set up later</button><button class="primary" id="setup-finish" type="button" disabled>Finish setup</button></div>
    </div>
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
      ${service.kind === "harness" ? '<button class="secondary" id="prepare-profile">Prepare profile</button>' : ""}
      ${service.kind === "harness" ? '<button class="secondary" id="install-companion">Install companion</button>' : ""}
    </div>
  </article>`;
}

function render(): void {
  if (!snapshot) return;
  $("#service-grid").innerHTML = serviceCard(snapshot.ollama) + serviceCard(snapshot.harness);
  $("#config-path").textContent = `Config: ${snapshot.configPath}`;
  const gpu = snapshot.environment.gpus[0];
  $("#environment-strip").innerHTML = `
    <div><span class="env-label">SYSTEM</span><strong>${snapshot.environment.operatingSystem} · ${snapshot.environment.architecture}</strong></div>
    <div><span class="env-label">GPU</span><strong>${gpu ? gpu.name : "No NVIDIA GPU detected"}</strong></div>
    <div><span class="env-label">VRAM</span><strong>${gpu ? `${(gpu.memoryUsedMib / 1024).toFixed(1)} / ${(gpu.memoryTotalMib / 1024).toFixed(1)} GB` : "—"}</strong></div>
    <div><span class="env-label">DRIVER</span><strong>${gpu?.driverVersion ?? "—"}</strong></div>
    <div><span class="env-label">TOOLS</span><strong>${[snapshot.environment.ollamaPath && "Ollama", snapshot.environment.harnessPath && "Harness", snapshot.environment.nodePath && "Node", snapshot.environment.gitPath && "Git"].filter(Boolean).join(" · ") || "None detected"}</strong></div>`;
  $("#compatibility-strip").innerHTML = snapshot.compatibility.components.map((component) => `
    <div class="compatibility-item ${component.state}">
      <span class="compatibility-state">${component.state}</span>
      <div><strong>${component.displayName} ${component.detectedVersion ?? "version unknown"}</strong><small>${component.message} · Recommended ${component.recommendedVersion}</small></div>
    </div>`).join("");
  const managedOllama = snapshot.managedOllama;
  const managedHarness = snapshot.managedHarness;
  $("#runtime-management").innerHTML = `
    <div class="runtime-item"><div><p class="eyebrow">APP-OWNED OLLAMA</p><strong>${managedOllama.installed ? `Ollama ${managedOllama.currentVersion}` : "No managed Ollama release"}</strong><span>${managedOllama.installed ? `Verified release active${managedOllama.previousVersion ? ` · Previous ${managedOllama.previousVersion}` : ""}` : "Official archive · 1.36 GB download · SHA-256 verified"}</span></div><div class="runtime-actions"><button class="secondary" id="install-managed-ollama">${managedOllama.installed ? "Reinstall verified release" : "Install verified Ollama"}</button><button class="secondary" id="rollback-managed-ollama" ${managedOllama.canRollback ? "" : "disabled"}>Rollback</button></div></div>
    <div class="runtime-item"><div><p class="eyebrow">APP-OWNED HARNESS</p><strong>${managedHarness.installed ? `Harness ${managedHarness.currentVersion}` : "Harness is externally installed"}</strong><span>${managedHarness.installed ? `Versioned release active${managedHarness.previousVersion ? ` · Previous ${managedHarness.previousVersion}` : ""}` : "Import a tested copy without modifying the existing Harness installation."}</span></div><div class="runtime-actions"><button class="secondary" id="install-managed-harness">${managedHarness.installed ? "Re-import tested release" : "Import managed Harness"}</button><button class="secondary" id="rollback-managed-harness" ${managedHarness.canRollback ? "" : "disabled"}>Rollback</button></div></div>`;

  const running = new Map(snapshot.runningModels.map((model) => [model.name, model]));
  $("#models").innerHTML = snapshot.installedModels.length
    ? snapshot.installedModels.map((model) => {
        const active = running.get(model.name);
        return `<div class="model-row"><div class="model-main"><span class="model-dot ${active ? "active" : ""}"></span><div><strong>${model.name}</strong><span>${[model.parameterSize, model.quantizationLevel, bytes(model.size)].filter(Boolean).join(" · ")}</span></div></div><div class="model-runtime">${active ? `<strong>${bytes(active.sizeVram)} VRAM</strong><span>${active.contextLength ? `${active.contextLength.toLocaleString()} context` : "Loaded"}</span>` : "<span>Not loaded</span>"}</div><div class="row-actions">${active ? `<button class="secondary model-action" data-action="unload" data-model="${escapeAttribute(model.name)}">Release VRAM</button>` : ""}<button class="ghost danger-text model-action" data-action="delete" data-model="${escapeAttribute(model.name)}">Delete</button></div></div>`;
      }).join("")
    : `<div class="empty"><strong>${snapshot.ollama.state === "online" ? "No models installed" : "Ollama is offline"}</strong><span>${snapshot.ollama.state === "online" ? "Pull a model above to get started." : "Start Ollama to view and manage models."}</span></div>`;

  document.querySelectorAll<HTMLButtonElement>(".service-action").forEach((button) => button.addEventListener("click", () => serviceAction(button)));
  document.querySelectorAll<HTMLButtonElement>(".model-action").forEach((button) => button.addEventListener("click", () => modelAction(button)));
  document.querySelector<HTMLButtonElement>("#prepare-profile")?.addEventListener("click", () => {
    void runAction(() => invoke<ActionResult>("prepare_harness_profile"));
  });
  document.querySelector<HTMLButtonElement>("#install-companion")?.addEventListener("click", () => {
    void runAction(() => invoke<ActionResult>("install_harness_companion"));
  });
  $("#install-managed-harness").addEventListener("click", () => {
    void runAction(() => invoke<ActionResult>("install_managed_harness"));
  });
  $("#install-managed-ollama").addEventListener("click", () => {
    if (!window.confirm("Download and install the verified 1.36 GB Ollama archive? The safety preflight requires about 9.5 GB of free disk space.")) return;
    void runAction(() => invoke<ActionResult>("install_managed_ollama"));
  });
  $("#rollback-managed-ollama").addEventListener("click", () => {
    if (!window.confirm("Switch Ollama back to the previous app-owned release?")) return;
    void runAction(() => invoke<ActionResult>("rollback_managed_ollama"));
  });
  $("#rollback-managed-harness").addEventListener("click", () => {
    if (!window.confirm("Switch Harness back to the previous app-owned release?")) return;
    void runAction(() => invoke<ActionResult>("rollback_managed_harness"));
  });
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
    updateSetupSummary();
    if (!config.setupCompleted && !onboardingShown && !dialog.open && !setupDialog.open) {
      onboardingShown = true;
      setupDialog.showModal();
    }
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
  if (noticeTimer !== undefined) window.clearTimeout(noticeTimer);
  const notice = $("#notice");
  notice.textContent = message;
  notice.className = `notice ${error ? "error" : "success"}`;
  noticeTimer = window.setTimeout(() => notice.classList.add("hidden"), 6000);
}

void listen<PullProgress>("ollama-pull-progress", ({ payload }) => {
  if (noticeTimer !== undefined) window.clearTimeout(noticeTimer);
  const notice = $("#notice");
  const percent = payload.total && payload.completed != null
    ? ` · ${Math.min(100, Math.round(payload.completed / payload.total * 100))}%`
    : "";
  notice.textContent = `${payload.status}${percent}`;
  notice.className = "notice progress";
});

void listen<RuntimeInstallProgress>("runtime-install-progress", ({ payload }) => {
  if (noticeTimer !== undefined) window.clearTimeout(noticeTimer);
  const notice = $("#notice");
  const percent = payload.total
    ? ` · ${Math.min(100, Math.round(payload.completed / payload.total * 100))}%`
    : "";
  notice.textContent = `${payload.message}${percent}`;
  notice.className = "notice progress";
});

void listen<AppUpdateProgress>("app-update-progress", ({ payload }) => {
  if (noticeTimer !== undefined) window.clearTimeout(noticeTimer);
  const notice = $("#notice");
  const percent = payload.total
    ? ` · ${Math.min(100, Math.round(payload.downloaded / payload.total * 100))}%`
    : "";
  notice.textContent = `${payload.message}${percent}`;
  notice.className = "notice progress";
});

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
$("#update-button").addEventListener("click", async () => {
  if (busy) return;
  busy = true;
  document.body.classList.add("busy");
  try {
    notify("Checking the signed release channel…");
    const update = await invoke<AppUpdateMetadata | null>("check_for_app_update");
    if (!update) {
      notify("Local Agent Stack is up to date.");
      return;
    }
    if (!window.confirm(`Install signed update ${update.version}? The app will close during installation.`)) return;
    await invoke<void>("install_app_update");
  } catch (error) {
    notify(String(error), true);
  } finally {
    busy = false;
    document.body.classList.remove("busy");
  }
});
$("#diagnostics-button").addEventListener("click", () => {
  void runAction(() => invoke<ActionResult>("export_diagnostics"));
});
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
const setupDialog = $("#setup-dialog") as HTMLDialogElement;

async function openSettings(): Promise<void> {
  config = await invoke<StackConfig>("get_config");
  ($("#ollama-url") as HTMLInputElement).value = config.ollama.url;
  ($("#ollama-command") as HTMLInputElement).value = config.ollama.command ?? "";
  ($("#harness-url") as HTMLInputElement).value = config.harness.url;
  ($("#harness-command") as HTMLInputElement).value = config.harness.command ?? "";
  ($("#harness-home") as HTMLInputElement).value = config.harnessHome ?? "";
  ($("#harness-args") as HTMLInputElement).value = config.harness.args.join(" ");
  ($("#harness-profile") as HTMLInputElement).value = config.harnessProfile;
  dialog.showModal();
}

$("#settings-button").addEventListener("click", openSettings);

$("#settings-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (!config) return;
  const updated: StackConfig = {
    ollama: { ...config.ollama, url: ($("#ollama-url") as HTMLInputElement).value.trim(), command: ($("#ollama-command") as HTMLInputElement).value.trim() || undefined },
    harness: { ...config.harness, url: ($("#harness-url") as HTMLInputElement).value.trim(), command: ($("#harness-command") as HTMLInputElement).value.trim() || undefined, args: ($("#harness-args") as HTMLInputElement).value.trim().split(/\s+/).filter(Boolean) },
    harnessHome: ($("#harness-home") as HTMLInputElement).value.trim() || undefined,
    harnessProfile: ($("#harness-profile") as HTMLInputElement).value.trim(),
    managedHarnessNode: config.managedHarnessNode,
    managedHarnessSource: config.managedHarnessSource,
    managedHarnessEntrypoint: config.managedHarnessEntrypoint,
    setupCompleted: config.setupCompleted,
  };
  await runAction(() => invoke<ActionResult>("save_config", { config: updated }));
  config = updated;
  dialog.close();
  if (!updated.setupCompleted) setupDialog.showModal();
});

function updateSetupSummary(): void {
  if (!snapshot) return;
  const gpu = snapshot.environment.gpus[0];
  const detected = [
    snapshot.environment.ollamaPath ? "Ollama detected" : "Ollama not detected",
    snapshot.managedOllama.installed ? `Managed Ollama ${snapshot.managedOllama.currentVersion}` : "External Ollama mode",
    snapshot.environment.harnessPath ? "Harness detected" : "Harness not detected",
    snapshot.managedHarness.installed ? `Managed Harness ${snapshot.managedHarness.currentVersion}` : "External Harness mode",
    gpu ? `${gpu.name} · ${(gpu.memoryTotalMib / 1024).toFixed(1)} GB VRAM` : "No NVIDIA GPU detected",
    snapshot.compatibility.components.some((component) => component.state === "outdated") ? "Upgrade warning" : "Compatibility checked",
  ];
  $("#setup-summary").textContent = detected.join("  •  ");
}

async function runSetupCommand(
  command: string,
  buttonSelector: string,
  resultSelector: string,
): Promise<void> {
  if (busy) return;
  const button = $(buttonSelector) as HTMLButtonElement;
  const result = $(resultSelector);
  busy = true;
  button.disabled = true;
  result.textContent = "Working…";
  result.className = "working";
  try {
    const response = await invoke<ActionResult>(command);
    result.textContent = response.message;
    result.className = response.ok ? "complete" : "failed";
    await refresh();
  } catch (error) {
    result.textContent = String(error);
    result.className = "failed";
  } finally {
    busy = false;
    button.disabled = false;
  }
}

$("#setup-settings").addEventListener("click", () => {
  setupDialog.close();
  void openSettings();
});
$("#setup-profile").addEventListener("click", () => {
  void runSetupCommand("prepare_harness_profile", "#setup-profile", "#setup-profile-result");
});
$("#setup-managed").addEventListener("click", () => {
  void runSetupCommand("install_managed_harness", "#setup-managed", "#setup-managed-result");
});
$("#setup-ollama").addEventListener("click", () => {
  if (!window.confirm("Download and install the verified 1.36 GB Ollama archive? The safety preflight requires about 9.5 GB of free disk space.")) return;
  void runSetupCommand("install_managed_ollama", "#setup-ollama", "#setup-ollama-result");
});
$("#setup-companion").addEventListener("click", () => {
  void runSetupCommand("install_harness_companion", "#setup-companion", "#setup-companion-result");
});
$("#setup-verify").addEventListener("click", async () => {
  if (busy) return;
  const result = $("#setup-verify-result");
  result.textContent = "Checking services and hardware…";
  result.className = "working";
  await refresh();
  setupVerified = true;
  result.textContent = `Health check complete · Ollama ${snapshot?.ollama.state ?? "unknown"} · Harness ${snapshot?.harness.state ?? "unknown"}`;
  result.className = "complete";
  ($("#setup-finish") as HTMLButtonElement).disabled = false;
});
$("#setup-later").addEventListener("click", () => setupDialog.close());
$("#setup-finish").addEventListener("click", async () => {
  if (!setupVerified || busy) return;
  busy = true;
  try {
    const result = await invoke<ActionResult>("complete_setup");
    if (config) config.setupCompleted = true;
    setupDialog.close();
    notify(result.message, !result.ok);
  } catch (error) {
    $("#setup-verify-result").textContent = String(error);
    $("#setup-verify-result").className = "failed";
  } finally {
    busy = false;
  }
});

void getVersion().then((version) => { $("#app-version").textContent = `v${version}`; });
void refresh();
window.setInterval(() => { if (!busy) void refresh(); }, 10000);
