import { invoke } from "@tauri-apps/api/core";

import type {
  TraceBranch,
  TraceEdge,
  TraceEvent,
  TraceReplay,
  TraceRole,
  TraceSessionSummary,
  TraceTelemetrySample,
} from "./types";

const CONTEXT_COLORS = ["#2563eb", "#16815c", "#c25310", "#7a3ed2", "#0b7c9e", "#c52f3d", "#8a6a13", "#556170"];
const CONTEXT_COLOR_INDEX = new Map<string, number>();

interface TraceScene {
  index: number;
  time: number;
  events: TraceEvent[];
  title: string;
}

interface TraceRenderOptions {
  revealCurrentScene: boolean;
  animateCurrentScene: boolean;
}

interface TraceFoldState {
  key: string;
  open: boolean;
  scrollLeft: number;
  scrollTop: number;
}

interface TraceInteractionState {
  documentLeft: number;
  documentTop: number;
  graphLeft: number;
  graphTop: number;
  historyLeft: number;
  historyTop: number;
  detailOpen: boolean;
  folds: TraceFoldState[];
  focusedEventId?: string;
}

const INTERACTIVE_RENDER: TraceRenderOptions = { revealCurrentScene: true, animateCurrentScene: true };
const QUIET_RENDER: TraceRenderOptions = { revealCurrentScene: false, animateCurrentScene: false };

export class TraceViewer {
  private readonly root: HTMLElement;
  private sessions: TraceSessionSummary[] = [];
  private replay: TraceReplay | undefined;
  private scenes: TraceScene[] = [];
  private sceneByEvent = new Map<string, number>();
  private sceneIndex = 0;
  private selectedEventId: string | undefined;
  private playing = false;
  private playTimer: number | undefined;
  private liveTimer: number | undefined;
  private liveReloadInFlight = false;
  private active = false;
  private speed = 1;

  constructor(root: HTMLElement) {
    this.root = root;
    this.root.innerHTML = this.shell();
    this.bindControls();
  }

  async activate(): Promise<void> {
    this.active = true;
    await this.refreshSessions();
    if (this.liveTimer === undefined) {
      this.liveTimer = window.setInterval(() => {
        if (this.active && this.replay?.session.status === "live" && !this.playing) void this.reloadSelected();
      }, 4_000);
    }
  }

  deactivate(): void {
    this.active = false;
    this.pause();
  }

  async refresh(): Promise<void> {
    await this.refreshSessions();
  }

  private shell(): string {
    return `<div id="ultra-timeline-live" class="ut-window">
      <header class="ut-header">
        <div><h1 class="ut-title">Ultra Workflow Timeline</h1><div class="ut-subtitle" id="ut-session-meta">Live Harness trace · reusable np4 slots · context-lineage colors</div></div>
        <div class="ut-header-right">
          <select id="ut-session-select" aria-label="Recorded Harness session"><option>Loading sessions…</option></select>
          <button class="ut-control" id="ut-reload" type="button">Reload</button>
          <div class="ut-now"><strong id="ut-step-title">Reading session</strong><span id="ut-step-time">—</span></div>
        </div>
      </header>
      <div class="ut-empty" id="ut-empty"><strong>Reading the Harness session store…</strong><span>The recorder leaves Harness files unchanged.</span></div>
      <div class="ut-content hidden" id="ut-content">
        <section class="ut-playbar" aria-label="Timeline controls">
          <div class="ut-controls">
            <div class="ut-control-group"><button class="ut-control" id="ut-prev" aria-label="Previous step">←</button><button class="ut-control play" id="ut-play" aria-label="Play timeline">▶ Play</button><button class="ut-control" id="ut-next" aria-label="Next step">→</button></div>
            <input class="ut-range" id="ut-range" type="range" min="0" max="0" value="0" aria-label="Timeline step">
            <div class="ut-control-group"><select class="ut-control" id="ut-speed" aria-label="Playback speed"><option value="0.5">0.5×</option><option value="1" selected>1×</option><option value="2">2×</option></select><span class="ut-step-label" id="ut-step-count">Step 0 / 0</span></div>
          </div>
          <div class="ut-ticks-scroll"><div class="ut-ticks" id="ut-ticks"></div></div>
          <div class="ut-context-key" id="ut-context-key" aria-label="Context color legend"></div>
        </section>
        <main class="ut-main">
          <section class="ut-state-grid">
            <article class="ut-panel">
              <div class="ut-panel-title"><span>GPU now</span><span id="ut-gpu-time">telemetry</span></div>
              <div class="ut-gpu-name" id="ut-gpu-name">Telemetry unavailable</div>
              <div class="ut-gpu-stat"><div class="ut-gpu-line"><span>VRAM</span><strong id="ut-vram">—</strong></div><div class="ut-meter"><span id="ut-vram-bar"></span></div></div>
              <div class="ut-gpu-stat"><div class="ut-gpu-line"><span>GPU utilization</span><strong id="ut-util">—</strong></div><div class="ut-meter util"><span id="ut-util-bar"></span></div></div>
            </article>
            <article class="ut-panel">
              <div class="ut-panel-title"><span>Reusable inference slots</span><span id="ut-busy-count">0 busy</span></div>
              <div class="ut-workflow-bands" id="ut-workflow-bands"></div>
              <div class="ut-slots" id="ut-slots"></div>
            </article>
            <article class="ut-panel">
              <div class="ut-panel-title"><span>Queued requests</span><span id="ut-queue-count">0 waiting · derived</span></div>
              <div class="ut-queue" id="ut-queue"></div>
            </article>
          </section>
          <section class="ut-panel ut-history-panel">
            <div class="ut-panel-title"><span>Slot lease timeline</span><span>completed owners stay visible; slots are reused</span></div>
            <div class="ut-history-axis" id="ut-history-axis"></div><div id="ut-history"></div>
          </section>
          <section class="ut-panel ut-graph-panel">
            <div class="ut-graph-head"><strong>Agent, workflow, and communication graph</strong><span>new events appear at the playhead</span></div>
            <div class="ut-graph-scroll" id="ut-graph-scroll"><div class="ut-graph" id="ut-graph" aria-label="Animated agent workflow graph"></div></div>
            <div class="ut-edge-key"><span class="same">same branch</span><span class="spawn">spawn/new context</span><span class="fork">fork/shared context</span><span class="report">report</span><span class="consultation">consultation</span><span class="workflow">workflow membership</span></div>
          </section>
        </main>
        <details class="ut-detail" open>
          <summary>Selected event · click any graph node</summary>
          <div class="ut-detail-grid"><div><span>Branch</span><span id="ut-detail-branch">—</span></div><div><span>Event</span><span id="ut-detail-event">—</span></div><div><span>Message</span><span id="ut-detail-message">—</span></div><div><span>Context</span><span id="ut-detail-context">—</span></div></div>
          <div class="ut-detail-folds" id="ut-detail-folds"></div>
        </details>
      </div>
    </div>`;
  }

  private bindControls(): void {
    this.element<HTMLButtonElement>("#ut-reload").addEventListener("click", () => void this.refreshSessions());
    this.element<HTMLSelectElement>("#ut-session-select").addEventListener("change", (event) => void this.loadSession((event.currentTarget as HTMLSelectElement).value));
    this.element<HTMLButtonElement>("#ut-prev").addEventListener("click", () => this.setScene(this.sceneIndex - 1));
    this.element<HTMLButtonElement>("#ut-next").addEventListener("click", () => this.setScene(this.sceneIndex + 1));
    this.element<HTMLButtonElement>("#ut-play").addEventListener("click", () => this.playing ? this.pause() : this.play());
    this.element<HTMLInputElement>("#ut-range").addEventListener("input", (event) => this.setScene(Number((event.currentTarget as HTMLInputElement).value)));
    this.element<HTMLSelectElement>("#ut-speed").addEventListener("change", (event) => {
      this.speed = Number((event.currentTarget as HTMLSelectElement).value);
      if (this.playing) { this.pause(); this.play(); }
    });
  }

  private async refreshSessions(): Promise<void> {
    const select = this.element<HTMLSelectElement>("#ut-session-select");
    const selected = select.value || this.replay?.session.id;
    select.disabled = true;
    try {
      this.sessions = await invoke<TraceSessionSummary[]>("list_trace_sessions");
      select.innerHTML = this.sessions.length
        ? this.sessions.map((session) => `<option value="${escapeAttribute(session.id)}">${escapeHtml(session.title)} · ${formatClock(session.updatedAt)}</option>`).join("")
        : "<option>No Harness sessions found</option>";
      if (!this.sessions.length) { this.showEmpty("No Harness sessions found", "Start a Harness conversation or set the session-store path in Settings."); return; }
      const target = this.sessions.some((session) => session.id === selected) ? selected! : this.sessions[0]!.id;
      select.value = target;
      await this.loadSession(target);
    } catch (error) {
      this.showEmpty("Trace recorder could not read Harness", String(error));
    } finally {
      select.disabled = false;
    }
  }

  private async loadSession(sessionId: string): Promise<void> {
    this.pause();
    this.showEmpty("Reconstructing semantic timeline…", "Joining model activity, agents, workflows, slots, and communication edges.");
    try {
      this.replay = await invoke<TraceReplay>("load_trace_session", { sessionId });
      this.rebuildScenes();
      this.sceneIndex = 0;
      this.selectedEventId = this.scenes[0]?.events[0]?.id;
      this.element("#ut-empty").classList.add("hidden");
      this.element("#ut-content").classList.remove("hidden");
      this.render();
    } catch (error) {
      this.showEmpty("Session reconstruction failed", String(error));
    }
  }

  private async reloadSelected(): Promise<void> {
    if (!this.replay || this.liveReloadInFlight) return;
    const sessionId = this.replay.session.id;
    const fallbackSceneIndex = this.sceneIndex;
    const anchorEventId = this.selectedEventId ?? this.scenes[this.sceneIndex]?.events.at(-1)?.id;
    this.liveReloadInFlight = true;
    try {
      const nextReplay = await invoke<TraceReplay>("load_trace_session", { sessionId });
      if (!this.active || this.replay?.session.id !== sessionId) return;

      // Capture this after the async read so a drag or fold toggle made while it
      // was in flight is the state that wins.
      const interaction = this.captureInteractionState();
      this.replay = nextReplay;
      this.rebuildScenes();
      const anchoredScene = anchorEventId ? this.sceneByEvent.get(anchorEventId) : undefined;
      this.sceneIndex = anchoredScene ?? clamp(fallbackSceneIndex, 0, Math.max(0, this.scenes.length - 1));
      this.render(QUIET_RENDER);
      this.restoreInteractionState(interaction);
    } catch (error) {
      // A transient recorder read must not replace or move the trace currently
      // being inspected. The next live tick will retry.
      console.warn("Ultra Trace live update failed", error);
    } finally {
      this.liveReloadInFlight = false;
    }
  }

  private rebuildScenes(): void {
    if (!this.replay) { this.scenes = []; this.sceneByEvent.clear(); return; }
    CONTEXT_COLOR_INDEX.clear();
    for (const branch of this.replay.branches) {
      if (!CONTEXT_COLOR_INDEX.has(branch.contextId)) CONTEXT_COLOR_INDEX.set(branch.contextId, CONTEXT_COLOR_INDEX.size);
    }
    const built = buildScenes(this.replay);
    this.scenes = built.scenes;
    this.sceneByEvent = built.sceneByEvent;
  }

  private render(options: TraceRenderOptions = INTERACTIVE_RENDER): void {
    const replay = this.replay;
    if (!replay || !this.scenes.length) { this.showEmpty("This session has no semantic operations", "Harness created the session header, but no execution activity was found."); return; }
    this.sceneIndex = clamp(this.sceneIndex, 0, this.scenes.length - 1);
    const scene = this.scenes[this.sceneIndex]!;
    this.element<HTMLInputElement>("#ut-range").max = String(this.scenes.length - 1);
    this.element<HTMLInputElement>("#ut-range").value = String(this.sceneIndex);
    this.element("#ut-step-title").textContent = scene.title;
    this.element("#ut-step-time").textContent = `${formatClock(scene.time)} · Step ${this.sceneIndex + 1} / ${this.scenes.length}`;
    this.element("#ut-step-count").textContent = `Step ${this.sceneIndex + 1} / ${this.scenes.length}`;
    const rawRecords = replay.events.reduce((total, event) => total + (event.rawEventCount || 1), 0);
    this.element("#ut-session-meta").textContent = `${replay.events.length} semantic nodes · ${rawRecords.toLocaleString()} folded raw records · ${replay.maxSlots} reusable slots`;
    this.renderTicks();
    this.renderContextKey();
    this.renderGpu(scene.time);
    this.renderSlots(scene.time, options.animateCurrentScene);
    this.renderQueue(scene.time);
    this.renderHistory();
    this.renderGraph(scene.time, options.revealCurrentScene, options.animateCurrentScene);
    const selected = replay.events.find((event) => event.id === this.selectedEventId && (this.sceneByEvent.get(event.id) ?? Infinity) <= this.sceneIndex);
    const newest = scene.events.at(-1) ?? replay.events.filter((event) => (this.sceneByEvent.get(event.id) ?? Infinity) <= this.sceneIndex).at(-1);
    this.selectEvent((selected ?? newest)?.id);
  }

  private renderTicks(): void {
    const ticks = this.element("#ut-ticks");
    const minimum = Math.max(0, this.scenes.length * 28);
    ticks.setAttribute("style", `grid-template-columns:repeat(${this.scenes.length},minmax(28px,1fr));min-width:${minimum}px`);
    ticks.innerHTML = this.scenes.map((_, index) => `<span class="ut-tick ${index === this.sceneIndex ? "active" : ""}">${index + 1}</span>`).join("");
  }

  private renderContextKey(): void {
    const replay = this.replay!;
    const seen = new Set<string>();
    const entries: string[] = [];
    for (const branch of replay.branches) {
      const identity = `${branch.contextId}:${branch.forked}`;
      if (seen.has(identity)) continue;
      seen.add(identity);
      const label = branch.forked ? `Fork of ${parentLabel(branch, replay.branches)}` : branch.role === "supervisor" ? "Supervisor context" : `Spawned ${branch.label}`;
      entries.push(`<span class="ut-key-item"><i class="ut-swatch ${branch.forked ? "fork" : ""}" style="--swatch:${contextColor(branch.contextId)};--swatch-soft:${contextSoft(branch.contextId)}"></i>${escapeHtml(label)}</span>`);
    }
    this.element("#ut-context-key").innerHTML = entries.join("");
  }

  private renderGpu(time: number): void {
    const sample = nearestTelemetry(this.replay!.telemetry, time);
    const gpu = sample?.gpu;
    this.element("#ut-gpu-name").textContent = gpu?.name ?? "Telemetry unavailable for this time";
    this.element("#ut-gpu-time").textContent = sample ? formatClock(sample.time) : "not captured";
    this.element("#ut-vram").textContent = gpu ? `${(gpu.memoryUsedMib / 1024).toFixed(1)} / ${(gpu.memoryTotalMib / 1024).toFixed(1)} GiB` : "—";
    this.element("#ut-util").textContent = gpu?.utilizationPercent == null ? "—" : `${gpu.utilizationPercent}%`;
    this.element<HTMLElement>("#ut-vram-bar").style.width = `${gpu ? gpu.memoryUsedMib / gpu.memoryTotalMib * 100 : 0}%`;
    this.element<HTMLElement>("#ut-util-bar").style.width = `${gpu?.utilizationPercent ?? 0}%`;
  }

  private renderSlots(time: number, animateCurrentScene: boolean): void {
    const replay = this.replay!;
    const branches = new Map(replay.branches.map((branch) => [branch.id, branch]));
    const active = replay.leases.filter((lease) => lease.startedAt <= time && lease.endedAt > time);
    this.element("#ut-busy-count").textContent = `${active.length}/${replay.maxSlots} busy · derived`;
    const workflows = replay.branches.filter((branch) => branch.role === "workflow" && branch.createdAt <= time && (!branch.removedAt || branch.removedAt > time));
    this.element("#ut-workflow-bands").innerHTML = workflows.length ? workflows.map((workflow) => {
      const members = replay.branches.filter((branch) => branch.parentId === workflow.id && branch.createdAt <= time && (!branch.removedAt || branch.removedAt > time));
      const slots = active.filter((lease) => members.some((member) => member.id === lease.branchId)).map((lease) => `S${lease.slot}`);
      return `<span class="ut-workflow-band" style="--band-color:${contextColor(workflow.contextId)}">${roleIcon("workflow")} ${escapeHtml(workflow.label)} · ${slots.length ? slots.join(", ") : "waiting"}<span class="ut-band-members">${members.map((member) => `<i class="ut-band-dot ${member.forked ? "forked" : ""}" style="--member-color:${contextColor(member.contextId)};--member-soft:${contextSoft(member.contextId)}"></i>`).join("")}</span></span>`;
    }).join("") : "<span class=\"ut-key-item\">No active dynamic workflow</span>";
    this.element("#ut-slots").innerHTML = Array.from({ length: replay.maxSlots }, (_, offset) => {
      const slot = offset + 1;
      const lease = active.find((candidate) => candidate.slot === slot);
      if (!lease) return `<div class="ut-slot empty"><div class="ut-slot-top"><span class="ut-slot-number">Slot ${slot}</span><span class="ut-slot-status">FREE</span></div><div class="ut-slot-owner"><span>Available for queue</span></div><div class="ut-slot-meta"><span>previous lease retained below</span></div></div>`;
      const branch = branches.get(lease.branchId)!;
      const context = lease.contextTokens == null || lease.contextPercent == null ? "context not emitted" : `${formatTokens(lease.contextTokens)} · ${lease.contextPercent.toFixed(1)}%`;
      const scene = this.sceneByEvent.get(lease.requestEventId);
      return `<div class="ut-slot ${branch.forked ? "forked" : ""} ${animateCurrentScene && scene === this.sceneIndex ? "pop" : ""}" style="--slot-color:${contextColor(branch.contextId)};--slot-soft:${contextSoft(branch.contextId)}"><div class="ut-slot-top"><span class="ut-slot-number">Slot ${slot}</span><span class="ut-slot-status">DECODING</span></div><div class="ut-slot-owner">${roleIcon(branch.role)}<span>${escapeHtml(branch.label)}</span></div><div class="ut-slot-meta"><span>${escapeHtml(shortId(lease.id))}</span><span>${context}</span></div></div>`;
    }).join("");
  }

  private renderQueue(time: number): void {
    const replay = this.replay!;
    const branches = new Map(replay.branches.map((branch) => [branch.id, branch]));
    const queued = replay.leases.filter((lease) => lease.requestedAt <= time && lease.startedAt > time);
    this.element("#ut-queue-count").textContent = `${queued.length} waiting · derived`;
    this.element("#ut-queue").innerHTML = queued.length ? queued.map((lease, index) => `<div class="ut-queue-row"><span class="ut-queue-pos">${index + 1}</span><span class="ut-queue-name">${roleIcon(branches.get(lease.branchId)?.role ?? "subagent")} ${escapeHtml(branches.get(lease.branchId)?.label ?? lease.branchId)}</span><span class="ut-queue-age">${formatDuration(lease.startedAt - lease.requestedAt)}</span><span class="ut-queue-need">needs S${lease.slot}</span></div>`).join("") : "<div class=\"ut-queue-empty\">Queue empty</div>";
  }

  private renderHistory(): void {
    const replay = this.replay!;
    const branches = new Map(replay.branches.map((branch) => [branch.id, branch]));
    this.element("#ut-history-axis").setAttribute("style", `grid-template-columns:42px repeat(${this.scenes.length},minmax(18px,1fr));min-width:${Math.max(0, this.scenes.length * 24)}px`);
    this.element("#ut-history-axis").innerHTML = "<span></span>" + this.scenes.map((_, index) => `<span>${index + 1}</span>`).join("");
    this.element("#ut-history").innerHTML = Array.from({ length: replay.maxSlots }, (_, offset) => {
      const slot = offset + 1;
      const segments = replay.leases.filter((lease) => lease.slot === slot && sceneForTime(this.scenes, lease.startedAt) <= this.sceneIndex).map((lease) => {
        const branch = branches.get(lease.branchId)!;
        const start = sceneForTime(this.scenes, lease.startedAt);
        const end = Math.max(start + 1, sceneForTime(this.scenes, lease.endedAt) + 1);
        const visibleEnd = Math.min(end, this.sceneIndex + 1);
        return `<span class="ut-history-segment ${branch.forked ? "forked" : ""}" style="--seg-start:${start + 1};--seg-end:${visibleEnd + 1};--seg-color:${contextColor(branch.contextId)};--seg-soft:${contextSoft(branch.contextId)}" title="${escapeAttribute(`${lease.id} · ${branch.label}`)}">${escapeHtml(branch.label)}</span>`;
      }).join("");
      const playhead = (this.sceneIndex + .5) / this.scenes.length * 100;
      return `<div class="ut-history-row"><span class="ut-history-label">Slot ${slot}</span><div class="ut-history-track" style="grid-template-columns:repeat(${this.scenes.length},minmax(18px,1fr));min-width:${Math.max(0, this.scenes.length * 24)}px">${segments}<i class="ut-history-playhead" style="--playhead:${playhead}%"></i></div></div>`;
    }).join("");
  }

  private renderGraph(time: number, revealCurrentScene: boolean, animateCurrentScene: boolean): void {
    const replay = this.replay!;
    const visible = replay.events.filter((event) => (this.sceneByEvent.get(event.id) ?? Infinity) <= this.sceneIndex);
    const visibleIds = new Set(visible.map((event) => event.id));
    const branches = replay.branches;
    const laneIndex = new Map(branches.map((branch, index) => [branch.id, index]));
    const xSpacing = 150;
    const width = Math.max(1140, 300 + this.scenes.length * xSpacing);
    const height = Math.max(220, branches.length * 86);
    const positions = new Map<string, { x: number; y: number }>();
    const sameCell = new Map<string, number>();
    for (const event of visible) {
      const scene = this.sceneByEvent.get(event.id) ?? 0;
      const key = `${event.branchId}:${scene}`;
      const offset = sameCell.get(key) ?? 0;
      sameCell.set(key, offset + 1);
      positions.set(event.id, { x: 122 + scene * xSpacing + offset * 24, y: (laneIndex.get(event.branchId) ?? 0) * 86 + 21 });
    }
    const lanes = branches.map((branch, index) => {
      const present = branch.createdAt <= time;
      const retired = branch.removedAt != null && branch.removedAt <= time;
      const activeLease = replay.leases.some((lease) => lease.branchId === branch.id && lease.startedAt <= time && lease.endedAt > time);
      const current = visible.some((event) => event.branchId === branch.id && this.sceneByEvent.get(event.id) === this.sceneIndex);
      const status = !present ? "not created" : retired ? "removed · history only" : current ? "active at playhead" : activeLease ? "active lease · evaluating" : "waiting / earlier activity";
      return `<div class="ut-lane" style="--lane-y:${index * 86}px;--lane-color:${contextColor(branch.contextId)};opacity:${present ? 1 : .28}"><span class="ut-lane-label">${roleIcon(branch.role)} ${escapeHtml(branch.label)}${branch.forked ? " · striped/shared" : ""}</span><span class="ut-lane-status">${status}</span></div>`;
    }).join("");
    const markerIds = new Map<string, string>();
    for (const branch of branches) markerIds.set(branch.contextId, `ut-arrow-${hash(branch.contextId)}`);
    const defs = [...markerIds.entries()].map(([context, id]) => `<marker id="${id}" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0,0 L7,3.5 L0,7 Z" fill="${contextColor(context)}"/></marker>`).join("");
    const edges = replay.edges.filter((edge) => visibleIds.has(edge.from) && visibleIds.has(edge.to)).map((edge) => edgeSvg(edge, positions, replay, markerIds)).join("");
    const nodes = visible.map((event) => {
      const branch = branches.find((candidate) => candidate.id === event.branchId)!;
      const position = positions.get(event.id)!;
      const scene = this.sceneByEvent.get(event.id) ?? 0;
      const semantic = semanticEvent(event, branch, replay);
      const context = event.contextTokens == null || event.contextPercent == null ? "context not emitted" : `${formatTokens(event.contextTokens)} · ${event.contextPercent.toFixed(1)}%`;
      return `<button class="ut-node ${branch.forked ? "forked" : ""} ${animateCurrentScene && scene === this.sceneIndex ? "new" : ""}" style="--node-x:${position.x}px;--node-y:${position.y}px;--node-color:${contextColor(branch.contextId)};--node-soft:${contextSoft(branch.contextId)}" type="button" data-ut-event="${escapeAttribute(event.id)}" aria-pressed="${event.id === this.selectedEventId}" title="${escapeAttribute(event.message)}"><span class="ut-node-head">${roleIcon(semantic.role)} ${escapeHtml(semantic.type)}</span><span class="ut-node-title">${escapeHtml(semantic.title)}</span><span class="ut-node-context">${context}</span></button>`;
    }).join("");
    const graph = this.element<HTMLElement>("#ut-graph");
    graph.style.width = `${width}px`; graph.style.height = `${height}px`;
    graph.innerHTML = lanes + `<svg class="ut-edge-layer" viewBox="0 0 ${width} ${height}" width="${width}" height="${height}" aria-hidden="true"><defs>${defs}</defs>${edges}</svg>` + nodes;
    graph.querySelectorAll<HTMLButtonElement>("[data-ut-event]").forEach((button) => button.addEventListener("click", () => this.selectEvent(button.dataset.utEvent)));
    const newest = this.scenes[this.sceneIndex]?.events.at(-1);
    const newestPosition = newest ? positions.get(newest.id) : undefined;
    if (revealCurrentScene && newestPosition) this.element<HTMLElement>("#ut-graph-scroll").scrollLeft = Math.max(0, newestPosition.x - 560);
  }

  private selectEvent(id: string | undefined): void {
    const replay = this.replay;
    const event = replay?.events.find((candidate) => candidate.id === id);
    if (!replay || !event) return;
    this.selectedEventId = event.id;
    this.root.querySelectorAll("[data-ut-event]").forEach((node) => node.setAttribute("aria-pressed", String((node as HTMLElement).dataset.utEvent === id)));
    const branch = replay.branches.find((candidate) => candidate.id === event.branchId)!;
    const semantic = semanticEvent(event, branch, replay);
    this.element("#ut-detail-branch").textContent = `${branch.label} · context ${shortId(branch.contextId)}${branch.forked ? " fork/shared" : ""}`;
    this.element("#ut-detail-event").textContent = `${formatClock(event.time)} · ${semantic.type} · ${event.rawEventCount || 1} raw records folded`;
    this.element("#ut-detail-message").textContent = event.message;
    this.element("#ut-detail-context").textContent = event.contextTokens == null || event.contextPercent == null ? "Not emitted" : `${formatTokens(event.contextTokens)} (${event.contextPercent.toFixed(1)}%)`;
    this.element("#ut-detail-folds").innerHTML = payloadFolds(event);
  }

  private setScene(next: number): void {
    if (!this.scenes.length) return;
    this.sceneIndex = clamp(next, 0, this.scenes.length - 1);
    this.selectedEventId = this.scenes[this.sceneIndex]?.events.at(-1)?.id;
    this.render();
    if (this.sceneIndex === this.scenes.length - 1 && this.playing) this.pause();
  }

  private play(): void {
    if (!this.scenes.length) return;
    if (this.sceneIndex >= this.scenes.length - 1) this.sceneIndex = 0;
    this.playing = true;
    this.element<HTMLButtonElement>("#ut-play").textContent = "Ⅱ Pause";
    this.playTimer = window.setInterval(() => this.setScene(this.sceneIndex + 1), Math.max(350, 1_450 / this.speed));
    this.render();
  }

  private pause(): void {
    this.playing = false;
    if (this.playTimer !== undefined) window.clearInterval(this.playTimer);
    this.playTimer = undefined;
    const play = this.root.querySelector<HTMLButtonElement>("#ut-play");
    if (play) play.textContent = "▶ Play";
  }

  private captureInteractionState(): TraceInteractionState {
    const scrolling = document.scrollingElement;
    const graph = this.element<HTMLElement>("#ut-graph-scroll");
    const history = this.element<HTMLElement>(".ut-history-panel");
    const detail = this.element<HTMLDetailsElement>(".ut-detail");
    const focused = document.activeElement instanceof HTMLElement ? document.activeElement : undefined;
    const folds = [...this.root.querySelectorAll<HTMLDetailsElement>("#ut-detail-folds details")].map((fold, index) => {
      const key = fold.querySelector("summary")?.textContent?.trim() || String(index);
      const content = fold.querySelector<HTMLElement>("pre");
      return { key, open: fold.open, scrollLeft: content?.scrollLeft ?? 0, scrollTop: content?.scrollTop ?? 0 };
    });
    return {
      documentLeft: scrolling?.scrollLeft ?? 0,
      documentTop: scrolling?.scrollTop ?? 0,
      graphLeft: graph.scrollLeft,
      graphTop: graph.scrollTop,
      historyLeft: history.scrollLeft,
      historyTop: history.scrollTop,
      detailOpen: detail.open,
      folds,
      focusedEventId: focused?.dataset.utEvent,
    };
  }

  private restoreInteractionState(state: TraceInteractionState): void {
    const detail = this.element<HTMLDetailsElement>(".ut-detail");
    detail.open = state.detailOpen;
    const folds = new Map(state.folds.map((fold) => [fold.key, fold]));
    this.root.querySelectorAll<HTMLDetailsElement>("#ut-detail-folds details").forEach((fold, index) => {
      const key = fold.querySelector("summary")?.textContent?.trim() || String(index);
      const saved = folds.get(key);
      if (!saved) return;
      fold.open = saved.open;
      const content = fold.querySelector<HTMLElement>("pre");
      if (content) { content.scrollLeft = saved.scrollLeft; content.scrollTop = saved.scrollTop; }
    });
    const graph = this.element<HTMLElement>("#ut-graph-scroll");
    graph.scrollLeft = state.graphLeft;
    graph.scrollTop = state.graphTop;
    const history = this.element<HTMLElement>(".ut-history-panel");
    history.scrollLeft = state.historyLeft;
    history.scrollTop = state.historyTop;
    if (state.focusedEventId) {
      this.root.querySelector<HTMLElement>(`[data-ut-event="${CSS.escape(state.focusedEventId)}"]`)?.focus({ preventScroll: true });
    }
    document.scrollingElement?.scrollTo(state.documentLeft, state.documentTop);
  }

  private showEmpty(title: string, message: string): void {
    this.root.querySelector("#ut-content")?.classList.add("hidden");
    const empty = this.element("#ut-empty");
    empty.classList.remove("hidden");
    empty.innerHTML = `<strong>${escapeHtml(title)}</strong><span>${escapeHtml(message)}</span>`;
  }

  private element<T extends Element = HTMLElement>(selector: string): T {
    const element = this.root.querySelector<T>(selector);
    if (!element) throw new Error(`Timeline element is missing: ${selector}`);
    return element;
  }
}

function buildScenes(replay: TraceReplay): { scenes: TraceScene[]; sceneByEvent: Map<string, number> } {
  const index = new Map(replay.events.map((event, position) => [event.id, position]));
  const rank = new Map<string, number>();
  for (const event of replay.events) {
    let value = 0;
    for (const edge of replay.edges.filter((candidate) => candidate.to === event.id)) {
      const fromIndex = index.get(edge.from);
      if (fromIndex == null || fromIndex >= (index.get(event.id) ?? 0)) continue;
      value = Math.max(value, (rank.get(edge.from) ?? 0) + 1);
    }
    if (value === 0) {
      const branch = replay.branches.find((candidate) => candidate.id === event.branchId);
      if (branch?.parentId) {
        const parent = replay.events.filter((candidate) => candidate.branchId === branch.parentId && candidate.time <= event.time).at(-1);
        if (parent) value = (rank.get(parent.id) ?? 0) + 1;
      }
    }
    rank.set(event.id, value);
  }
  const usedRanks = [...new Set(rank.values())].sort((a, b) => a - b);
  const compressed = new Map(usedRanks.map((value, position) => [value, position]));
  const sceneByEvent = new Map<string, number>();
  for (const event of replay.events) sceneByEvent.set(event.id, compressed.get(rank.get(event.id) ?? 0) ?? 0);
  const scenes = usedRanks.map((_, position) => {
    const events = replay.events.filter((event) => sceneByEvent.get(event.id) === position);
    const time = Math.max(...events.map((event) => event.time));
    return { index: position, time, events, title: sceneTitle(events, replay) };
  });
  return { scenes, sceneByEvent };
}

function sceneTitle(events: TraceEvent[], replay: TraceReplay): string {
  const semantics = events.map((event) => {
    const branch = replay.branches.find((candidate) => candidate.id === event.branchId)!;
    return semanticEvent(event, branch, replay);
  });
  if (semantics.some((event) => event.type === "user/message")) return "User request enters supervisor";
  if (semantics.filter((event) => event.type === "subagent/spawn" || event.type === "subagent/fork").length > 1) return "Workflow fans out across parallel agents";
  if (semantics.filter((event) => event.type.includes("tool-call") || event.type === "tool/call").length > 1) return "Workers call tools in parallel";
  if (semantics.some((event) => event.type === "workflow/start")) return "Dynamic workflow starts";
  if (semantics.some((event) => event.type === "workflow/end")) return "Dynamic workflow settles";
  if (semantics.some((event) => event.type === "supervisor/wake")) return "Supervisor wakes to evaluate new evidence";
  return semantics.length > 1 ? `${semantics[0]?.title ?? "Parallel activity"} · ${semantics.length} parallel events` : semantics[0]?.title ?? "Recorded activity";
}

function semanticEvent(event: TraceEvent, branch: TraceBranch, replay: TraceReplay): { type: string; title: string; role: TraceRole } {
  const first = replay.events.find((candidate) => candidate.branchId === branch.id)?.id === event.id;
  const records = event.rawRecords?.map(asRecord) ?? [asRecord(event.raw)];
  const call = records.find((record) => record.type === "tool/call");
  const callData = asRecord(call?.data);
  const tool = String(callData.name ?? "tool");
  const argumentsText = String(callData.arguments ?? "");
  if (event.eventType === "turn/input") return { type: branch.role === "supervisor" ? "user/message" : "subagent/input", title: humanMessage(event) || event.label, role: branch.role };
  if (event.eventType === "model/execution") {
    if (first && branch.role === "subagent") return { type: branch.forked ? "subagent/fork" : "subagent/spawn", title: `${branch.label} starts`, role: "subagent" };
    if (branch.role === "supervisor") return { type: (event.turn ?? 1) > 1 ? "supervisor/wake" : "model/request", title: (event.turn ?? 1) > 1 ? "Supervisor resumes" : "Plan responsibilities", role: "supervisor" };
    return { type: "subagent/model", title: event.label, role: branch.role };
  }
  if (event.eventType === "tool/execution") {
    if (tool === "report") return { type: argumentsText.includes("consultation") ? "agent/consultation" : "subagent/report", title: argumentsText.includes("consultation") ? `${branch.label} asks supervisor` : `${branch.label} reports`, role: branch.role };
    return { type: tool.toLowerCase().includes("mcp") ? "mcp/tool-call" : "tool/call", title: `Call ${tool}`, role: branch.role };
  }
  if (event.eventType === "tool-workflow/run-start") return { type: "workflow/start", title: event.label, role: "workflow" };
  if (event.eventType === "tool-workflow/agent-start") return { type: "workflow/fan-out", title: event.label, role: "workflow" };
  if (event.eventType === "tool-workflow/agent-end") return { type: "subagent/report", title: event.label, role: "workflow" };
  if (event.eventType === "tool-workflow/run-end") return { type: "workflow/end", title: event.label, role: "workflow" };
  return { type: event.eventType, title: event.label, role: branch.role };
}

function payloadFolds(event: TraceEvent): string {
  const records = event.rawRecords?.length ? event.rawRecords.map(asRecord) : [asRecord(event.raw)];
  const request = records.find((record) => record.type === "request/header");
  const response = records.find((record) => record.type === "assistant/message");
  const header = asRecord(asRecord(request?.data).header);
  const responseData = asRecord(response?.data);
  const blocks = Array.isArray(asRecord(responseData.message).content) ? (asRecord(responseData.message).content as unknown[]).map(asRecord) : [];
  const reasoning = blocks.filter((block) => block.type === "reasoning").map((block) => String(block.text ?? "")).join("\n");
  const output = blocks.filter((block) => block.type === "text").map((block) => String(block.text ?? "")).join("\n");
  const sections: string[] = [];
  if (event.eventType === "turn/input") {
    const messages = records.filter((record) => record.type === "user/message");
    messages.forEach((record, index) => {
      const data = asRecord(record.data); const source = asRecord(data.source); const kind = String(source.kind ?? "injected");
      const title = kind === "user" ? "User message" : kind === "skill-catalog" ? "Injected skill catalog" : source.plugin ? `Injected runtime context · ${String(source.plugin)}` : `Injected context ${index + 1}`;
      sections.push(fold(title, messageContent(data)));
    });
    const lifecycle = records.filter((record) => record.type !== "user/message");
    if (lifecycle.length) sections.push(fold(`Session and turn lifecycle (${lifecycle.length})`, pretty(lifecycle)));
  } else if (event.eventType === "model/execution") {
    sections.push(fold("System prompt", String(header.system ?? "")));
    sections.push(fold("Tool / MCP schemas", pretty(header.tools ?? [])));
    sections.push(fold("Messages sent to model", pretty(header.messages ?? [])));
    sections.push(fold("Provider and model config", pretty({ config: header.config, adapterDefaults: header.adapterDefaults })));
    if (reasoning) sections.push(fold("Model reasoning", reasoning));
    if (output) sections.push(fold("Model output", output));
    sections.push(fold("Usage and replay metadata", pretty({ usage: responseData.usage, source: responseData.source, streamChunksCollapsed: event.streamChunks })));
    const chunks = records.filter((record) => ["assistant/chunk", "reasoning-chunks", "text-chunks", "tool-call-chunks"].includes(String(record.type)));
    if (chunks.length) sections.push(fold(`Streaming payloads (${chunks.length})`, pretty(chunks)));
  } else if (event.eventType === "tool/execution") {
    const call = records.find((record) => record.type === "tool/call"); const result = records.find((record) => record.type === "tool/result");
    if (call) sections.push(fold("Tool call and arguments", pretty(call)));
    if (result) sections.push(fold("Tool result", pretty(result)));
  }
  sections.push(fold(`Complete raw records (${records.length})`, pretty(records)));
  return sections.join("");
}

function edgeSvg(edge: TraceEdge, positions: Map<string, { x: number; y: number }>, replay: TraceReplay, markers: Map<string, string>): string {
  const from = positions.get(edge.from); const to = positions.get(edge.to); if (!from || !to) return "";
  const source = replay.events.find((event) => event.id === edge.from); const branch = replay.branches.find((candidate) => candidate.id === source?.branchId);
  const context = branch?.contextId ?? replay.session.id; const x1 = from.x + 112; const y1 = from.y + 28; const x2 = to.x; const y2 = to.y + 28; const bend = Math.max(30, Math.abs(x2 - x1) * .45);
  const dot = edge.kind === "report" ? `<circle class="ut-edge-dot" style="--edge-color:${contextColor(context)}" cx="${x1 + 2}" cy="${y1}" r="3"/>` : "";
  return `${dot}<path class="ut-edge ${edge.kind}" style="--edge-color:${contextColor(context)}" d="M${x1} ${y1} C${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}" marker-end="url(#${markers.get(context)})"/>${edge.label ? `<text class="ut-edge-label" x="${(x1 + x2) / 2}" y="${(y1 + y2) / 2 - 5}">${escapeHtml(edge.label)}</text>` : ""}`;
}

function roleIcon(role: TraceRole): string {
  if (role === "supervisor") return '<span class="ut-role-icon" aria-label="Supervisor"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><path d="M8 1.8 13 4v3.5c0 3.1-2 5.4-5 6.7-3-1.3-5-3.6-5-6.7V4l5-2.2Z"/><path d="m5.7 7.7 1.5 1.5 3.2-3.3"/></svg></span>';
  if (role === "workflow") return '<span class="ut-role-icon" aria-label="Workflow"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="3" cy="8" r="1.5"/><circle cx="12.5" cy="3.5" r="1.5"/><circle cx="12.5" cy="12.5" r="1.5"/><path d="M4.5 7.4 11 4.2M4.5 8.6l6.5 3.2"/></svg></span>';
  return '<span class="ut-role-icon" aria-label="Subagent"><svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5"><circle cx="8" cy="5" r="2.3"/><path d="M3.5 13c.6-2.5 2.1-3.7 4.5-3.7s3.9 1.2 4.5 3.7"/></svg></span>';
}

function humanMessage(event: TraceEvent): string {
  for (const raw of event.rawRecords ?? [event.raw]) {
    const record = asRecord(raw); const data = asRecord(record.data); const source = asRecord(data.source);
    if (record.type === "user/message" && source.kind === "user") return compactText(messageContent(data), 52);
  }
  return "";
}

function messageContent(data: Record<string, unknown>): string {
  const direct = Array.isArray(data.content) ? data.content.map(asRecord) : [];
  const nested = Array.isArray(asRecord(data.message).content) ? (asRecord(data.message).content as unknown[]).map(asRecord) : [];
  return [...direct, ...nested].map((block) => String(block.text ?? "")).filter(Boolean).join("\n") || pretty(data);
}

function fold(title: string, content: string): string { return `<details><summary>${escapeHtml(title)}</summary><pre>${escapeHtml(content || "(empty)")}</pre></details>`; }
function parentLabel(branch: TraceBranch, branches: TraceBranch[]): string { return branches.find((candidate) => candidate.id === branch.parentId)?.label ?? "parent context"; }
function sceneForTime(scenes: TraceScene[], time: number): number { const index = scenes.findIndex((scene) => scene.time >= time); return index < 0 ? Math.max(0, scenes.length - 1) : index; }
function nearestTelemetry(samples: TraceTelemetrySample[], time: number): TraceTelemetrySample | undefined { let nearest: TraceTelemetrySample | undefined; let distance = Infinity; for (const sample of samples) { const next = Math.abs(sample.time - time); if (next < distance) { nearest = sample; distance = next; } } return distance <= 6_000 ? nearest : undefined; }
function contextColor(id: string): string { const index = CONTEXT_COLOR_INDEX.get(id) ?? hashNumber(id); return CONTEXT_COLORS[index % CONTEXT_COLORS.length]!; }
function contextSoft(id: string): string { return `${contextColor(id)}1f`; }
function hash(value: string): string { return Math.abs(hashNumber(value)).toString(36); }
function hashNumber(value: string): number { let result = 0; for (const character of value) result = (result * 31 + character.charCodeAt(0)) | 0; return Math.abs(result); }
function asRecord(value: unknown): Record<string, unknown> { return value != null && typeof value === "object" && !Array.isArray(value) ? value as Record<string, unknown> : {}; }
function pretty(value: unknown): string { return JSON.stringify(value, null, 2); }
function compactText(value: string, limit: number): string { const clean = value.replace(/\s+/g, " ").trim(); return clean.length > limit ? `${clean.slice(0, limit - 1)}…` : clean; }
function formatClock(value: number): string { return new Date(value).toLocaleString([], { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit", second: "2-digit" }); }
function formatTokens(value: number): string { return value >= 1_000 ? `${(value / 1_000).toFixed(1)}k occupied` : `${value} occupied`; }
function formatDuration(value: number): string { return value < 1_000 ? `${value}ms` : `${(value / 1_000).toFixed(1)}s`; }
function shortId(value: string): string { return value.length > 18 ? `${value.slice(0, 18)}…` : value; }
function clamp(value: number, minimum: number, maximum: number): number { return Math.min(maximum, Math.max(minimum, value)); }
function escapeHtml(value: string): string { return value.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;"); }
function escapeAttribute(value: string): string { return escapeHtml(value).replaceAll("'", "&#39;"); }
