import { mockIPC } from "@tauri-apps/api/mocks";

import type { StackConfig, StackSnapshot, TraceEvent, TraceReplay, TraceSessionSummary } from "./types";

const start = new Date("2026-09-01T19:41:54-04:00").getTime();
const branch = {
  supervisor: "session-supervisor",
  workflowA: "workflow:session-supervisor:wf-8c2",
  research: "session-research",
  code: "session-code",
  audit: "session-audit",
  workflowB: "workflow:session-supervisor:wf-9a1",
  planner: "session-planner",
  fixer: "session-fixer",
  tester: "session-tester",
};

const definitions: Array<[string, number, string, string, number, number, unknown]> = [
  [branch.supervisor, 0, "turn/input", "Release audit requested", 18_400, 1, { type: "user/message", seq: 0, time: start, data: { content: [{ type: "text", text: "Run a complete release-readiness audit." }], source: { kind: "user" } } }],
  [branch.supervisor, 1, "request/header", "Supervisor model request", 25_900, 1, { type: "request/header", seq: 1, time: start + 2_000, data: { header: { config: { provider: "local-qwen", model: "Qwen3.8-27B-Q6_K" }, system: "Ultra Workflow supervisor system prompt…", tools: [{ name: "workflow" }, { name: "subagent" }, { name: "report" }], messages: [{ role: "user", content: "Run a complete release-readiness audit." }] } } }],
  [branch.supervisor, 2, "assistant/message", "Delegation plan", 25_900, 1, { type: "assistant/message", seq: 2, time: start + 4_000, data: { turn: 1, step: 1, usage: { inputTokens: 25_900, outputTokens: 1_240 }, message: { content: [{ type: "reasoning", text: "This is large enough for a dynamic workflow. I will reserve one supervisor slot and fan out three independent missions." }, { type: "text", text: "Starting a bounded release audit workflow." }] } } }],
  [branch.workflowA, 3, "tool-workflow/run-start", "Workflow wf-8c2 starts", 25_900, 1, { type: "tool-workflow/run-start", seq: 3, time: start + 5_000, data: { runId: "wf-8c2", name: "Release audit" } }],
  [branch.workflowA, 4, "tool-workflow/agent-start", "Research, code, and audit fan out", 25_900, 1, { type: "tool-workflow/agent-start", seq: 4, time: start + 6_000, data: { runId: "wf-8c2", members: [branch.research, branch.code, branch.audit] } }],
  [branch.research, 5, "request/header", "Research request", 11_100, 1, { type: "request/header", seq: 0, time: start + 7_000, data: { header: { config: { model: "Qwen3.8-27B-Q6_K" }, system: "Research worker", tools: [{ name: "mcp__browser" }], messages: [] } } }],
  [branch.code, 6, "request/header", "Code request", 12_900, 1, { type: "request/header", seq: 0, time: start + 7_100, data: { header: { config: { model: "Qwen3.8-27B-Q6_K" }, system: "Code worker", tools: [{ name: "read" }, { name: "edit" }], messages: [] } } }],
  [branch.audit, 7, "request/header", "Forked audit request", 20_200, 1, { type: "request/header", seq: 0, time: start + 7_200, data: { header: { config: { model: "Qwen3.8-27B-Q6_K" }, system: "Independent audit fork", tools: [{ name: "test" }], messages: [] } } }],
  [branch.research, 8, "assistant/message", "Research report", 62_700, 1, { type: "assistant/message", seq: 1, time: start + 15_000, data: { turn: 1, step: 1, usage: { inputTokens: 62_700, outputTokens: 2_400 }, message: { content: [{ type: "reasoning", text: "The primary source and implementation agree." }, { type: "text", text: "Research complete; evidence attached." }] } } }],
  [branch.supervisor, 9, "request/header", "Supervisor reviews report", 36_200, 2, { type: "request/header", seq: 4, time: start + 16_000, data: { header: { config: { model: "Qwen3.8-27B-Q6_K" }, system: "Supervisor resumes from durable state", tools: [], messages: [] } } }],
  [branch.workflowB, 10, "tool-workflow/run-start", "Replacement workflow wf-9a1", 40_500, 2, { type: "tool-workflow/run-start", seq: 5, time: start + 18_000, data: { runId: "wf-9a1", name: "Fix and verify" } }],
  [branch.planner, 11, "request/header", "Planner starts", 22_600, 1, { type: "request/header", seq: 0, time: start + 19_000, data: { header: { config: { model: "Qwen3.8-27B-Q6_K" } } } }],
  [branch.fixer, 12, "request/header", "Fixer starts", 44_800, 1, { type: "request/header", seq: 0, time: start + 19_100, data: { header: { config: { model: "Qwen3.8-27B-Q6_K" } } } }],
  [branch.code, 13, "assistant/message", "Code worker releases slot", 68_200, 1, { type: "assistant/message", seq: 1, time: start + 20_000, data: { turn: 1, step: 1, usage: { inputTokens: 68_200, outputTokens: 3_100 }, message: { content: [{ type: "text", text: "Implementation completed." }] } } }],
  [branch.tester, 14, "request/header", "Tester fork expands workflow", 31_800, 1, { type: "request/header", seq: 0, time: start + 20_100, data: { header: { config: { model: "Qwen3.8-27B-Q6_K" } } } }],
  [branch.audit, 15, "assistant/message", "Audit releases slot", 35_300, 1, { type: "assistant/message", seq: 1, time: start + 24_000, data: { turn: 1, step: 1, usage: { inputTokens: 35_300, outputTokens: 1_900 }, message: { content: [{ type: "text", text: "Independent audit passed." }] } } }],
  [branch.workflowB, 16, "tool-workflow/run-end", "Workflow wf-9a1 settles", 48_600, 2, { type: "tool-workflow/run-end", seq: 8, time: start + 29_000, data: { runId: "wf-9a1", stopReason: "completed" } }],
];

const events: TraceEvent[] = definitions.map(([branchId, index, eventType, label, contextTokens, step, raw]) => ({
  id: `${branchId}:${index}`,
  index,
  time: start + index * 1_800,
  sequence: index,
  branchId,
  eventType,
  label,
  message: label,
  turn: 1,
  step,
  contextTokens,
  contextWindow: 98_304,
  contextPercent: contextTokens / 98_304 * 100,
  outputTokens: eventType === "assistant/message" ? 2_000 : undefined,
  streamChunks: eventType === "assistant/message" ? 840 : 0,
  rawEventCount: 1,
  foldedEventTypes: [eventType],
  rawRecords: [raw],
  raw,
}));

events[0]!.rawEventCount = 3;
events[0]!.foldedEventTypes = ["user/message", "permission/preset", "user/message"];
events[0]!.rawRecords = [
  events[0]!.raw,
  { type: "user/message", seq: 1, time: start, data: { content: [{ type: "text", text: "Current runtime policy and workspace permissions." }], source: { kind: "plugin", plugin: "@deepseek-ai/dsh-system-prompt" } } },
  { type: "user/message", seq: 2, time: start, data: { content: [{ type: "text", text: "Available skill catalog." }], source: { kind: "skill-catalog" } } },
];

const session: TraceSessionSummary = {
  id: branch.supervisor,
  title: "Release audit with dynamic slot reuse",
  cwd: "C:\\workspace\\project",
  preset: "ultra-workflow",
  model: "Qwen3.8-27B-Q6_K",
  createdAt: start,
  updatedAt: start + 29_000,
  eventCount: 12_940,
  branchCount: 7,
  status: "recorded",
};

const replay: TraceReplay = {
  session,
  source: "development fixture",
  maxSlots: 4,
  startTime: start,
  endTime: start + 30_000,
  branches: [
    { id: branch.supervisor, label: "Supervisor", role: "supervisor", contextId: "ctx-a", forked: false, createdAt: start, status: "recorded" },
    { id: branch.workflowA, parentId: branch.supervisor, label: "Release audit · wf-8c2", role: "workflow", contextId: "ctx-a", forked: false, createdAt: start + 5_000, removedAt: start + 24_000, status: "removed" },
    { id: branch.research, parentId: branch.workflowA, label: "Research worker", role: "subagent", contextId: "ctx-b", forked: false, createdAt: start + 6_000, removedAt: start + 15_000, status: "removed" },
    { id: branch.code, parentId: branch.workflowA, label: "Code worker", role: "subagent", contextId: "ctx-c", forked: false, createdAt: start + 6_000, removedAt: start + 20_000, status: "removed" },
    { id: branch.audit, parentId: branch.workflowA, label: "Audit fork", role: "subagent", contextId: "ctx-a", forked: true, createdAt: start + 6_000, removedAt: start + 24_000, status: "removed" },
    { id: branch.workflowB, parentId: branch.supervisor, label: "Fix and verify · wf-9a1", role: "workflow", contextId: "ctx-a", forked: false, createdAt: start + 18_000, removedAt: start + 29_000, status: "removed" },
    { id: branch.planner, parentId: branch.workflowB, label: "Planner", role: "subagent", contextId: "ctx-e", forked: false, createdAt: start + 18_500, removedAt: start + 29_000, status: "removed" },
    { id: branch.fixer, parentId: branch.workflowB, label: "Fixer", role: "subagent", contextId: "ctx-d", forked: false, createdAt: start + 18_500, removedAt: start + 29_000, status: "removed" },
    { id: branch.tester, parentId: branch.workflowB, label: "Tester fork", role: "subagent", contextId: "ctx-d", forked: true, createdAt: start + 20_000, removedAt: start + 29_000, status: "removed" },
  ],
  events,
  edges: [
    { from: events[0]!.id, to: events[1]!.id, kind: "same", label: "" },
    { from: events[1]!.id, to: events[2]!.id, kind: "same", label: "" },
    { from: events[2]!.id, to: events[3]!.id, kind: "workflow", label: "workflow" },
    { from: events[3]!.id, to: events[4]!.id, kind: "workflow", label: "fan-out" },
    { from: events[4]!.id, to: events[5]!.id, kind: "spawn", label: "spawn" },
    { from: events[4]!.id, to: events[6]!.id, kind: "spawn", label: "spawn" },
    { from: events[4]!.id, to: events[7]!.id, kind: "fork", label: "fork" },
    { from: events[5]!.id, to: events[8]!.id, kind: "same", label: "" },
    { from: events[8]!.id, to: events[9]!.id, kind: "report", label: "report" },
    { from: events[9]!.id, to: events[10]!.id, kind: "workflow", label: "workflow" },
    { from: events[10]!.id, to: events[11]!.id, kind: "spawn", label: "spawn" },
    { from: events[10]!.id, to: events[12]!.id, kind: "spawn", label: "spawn" },
    { from: events[6]!.id, to: events[13]!.id, kind: "same", label: "" },
    { from: events[13]!.id, to: events[14]!.id, kind: "fork", label: "slot released → fork" },
    { from: events[12]!.id, to: events[14]!.id, kind: "fork", label: "fork" },
    { from: events[7]!.id, to: events[15]!.id, kind: "same", label: "" },
    { from: events[14]!.id, to: events[16]!.id, kind: "report", label: "evidence" },
  ],
  leases: [
    { id: "L-100", slot: 1, branchId: branch.supervisor, requestEventId: events[1]!.id, responseEventId: events[2]!.id, requestedAt: start + 2_000, startedAt: start + 2_000, endedAt: start + 6_500, contextTokens: 25_900, contextWindow: 98_304, contextPercent: 26.3, model: "Qwen3.8-27B-Q6_K", source: "derived" },
    { id: "L-101", slot: 2, branchId: branch.research, requestEventId: events[5]!.id, responseEventId: events[8]!.id, requestedAt: start + 7_000, startedAt: start + 7_000, endedAt: start + 15_000, contextTokens: 62_700, contextWindow: 98_304, contextPercent: 63.8, model: "Qwen3.8-27B-Q6_K", source: "derived" },
    { id: "L-102", slot: 3, branchId: branch.code, requestEventId: events[6]!.id, responseEventId: events[13]!.id, requestedAt: start + 7_100, startedAt: start + 7_100, endedAt: start + 20_000, contextTokens: 68_200, contextWindow: 98_304, contextPercent: 69.4, model: "Qwen3.8-27B-Q6_K", source: "derived" },
    { id: "L-103", slot: 4, branchId: branch.audit, requestEventId: events[7]!.id, responseEventId: events[15]!.id, requestedAt: start + 7_200, startedAt: start + 7_200, endedAt: start + 24_000, contextTokens: 35_300, contextWindow: 98_304, contextPercent: 35.9, model: "Qwen3.8-27B-Q6_K", source: "derived" },
    { id: "L-104", slot: 2, branchId: branch.supervisor, requestEventId: events[9]!.id, requestedAt: start + 16_000, startedAt: start + 16_000, endedAt: start + 18_000, contextTokens: 36_200, contextWindow: 98_304, contextPercent: 36.8, model: "Qwen3.8-27B-Q6_K", source: "derived" },
    { id: "L-107", slot: 1, branchId: branch.planner, requestEventId: events[11]!.id, requestedAt: start + 19_000, startedAt: start + 19_000, endedAt: start + 29_000, contextTokens: 22_600, contextWindow: 98_304, contextPercent: 23, model: "Qwen3.8-27B-Q6_K", source: "derived" },
    { id: "L-108", slot: 2, branchId: branch.fixer, requestEventId: events[12]!.id, requestedAt: start + 19_100, startedAt: start + 19_100, endedAt: start + 29_000, contextTokens: 44_800, contextWindow: 98_304, contextPercent: 45.6, model: "Qwen3.8-27B-Q6_K", source: "derived" },
    { id: "L-109", slot: 3, branchId: branch.tester, requestEventId: events[14]!.id, requestedAt: start + 20_100, startedAt: start + 20_100, endedAt: start + 29_000, contextTokens: 31_800, contextWindow: 98_304, contextPercent: 32.3, model: "Qwen3.8-27B-Q6_K", source: "derived" },
  ],
  telemetry: Array.from({ length: 31 }, (_, offset) => ({ time: start + offset * 1_000, gpu: { name: "NVIDIA GeForce RTX 5090", driverVersion: "581.80", memoryTotalMib: 32_607, memoryUsedMib: 28_500 + offset * 38, memoryFreeMib: 4_107 - offset * 38, utilizationPercent: Math.min(98, 18 + offset * 3) }, runningModels: [] })),
};

const config: StackConfig = {
  ollama: { url: "http://127.0.0.1:11434", args: ["serve"] },
  harness: { url: "http://127.0.0.1:3000", args: ["web", "--port", "3000"] },
  harnessProfile: "local-agent-stack",
  setupCompleted: true,
  trace: { enabled: true, gpuSampleIntervalMs: 1_000, inferenceSlots: 4 },
};

const snapshot: StackSnapshot = {
  ollama: { kind: "ollama", state: "online", url: config.ollama.url, managed: true, pid: 101 },
  harness: { kind: "harness", state: "online", url: config.harness.url, managed: true, pid: 102, launchUrl: config.harness.url },
  installedModels: [], runningModels: [],
  environment: { operatingSystem: "windows", architecture: "x86_64", gpus: [replay.telemetry[0]!.gpu!] },
  compatibility: { manifestUpdatedAt: "2026-09-01", components: [] },
  managedOllama: { installed: true, currentVersion: "0.11", canRollback: false },
  managedHarness: { installed: true, currentVersion: "0.1", canRollback: false },
  configPath: "development fixture",
};

export function installTraceDevMock(): void {
  const simulateLive = new URLSearchParams(window.location.search).get("live") === "1";
  let replayLoads = 0;
  mockIPC((command) => {
    if (command === "list_trace_sessions") return [{ ...session, status: simulateLive ? "live" : session.status }];
    if (command === "load_trace_session") {
      replayLoads += 1;
      if (!simulateLive) return replay;
      const additions = Math.min(4, Math.max(0, replayLoads - 1));
      const liveEvents: TraceEvent[] = Array.from({ length: additions }, (_, offset) => {
        const index = events.length + offset;
        const time = replay.endTime + (offset + 1) * 2_000;
        return {
          id: `${branch.supervisor}:live-${offset + 1}`,
          index,
          time,
          sequence: index,
          branchId: branch.supervisor,
          eventType: "model/execution",
          label: `Quiet live update ${offset + 1}`,
          message: `Background trace event ${offset + 1}`,
          turn: 2 + offset,
          step: 1,
          contextTokens: 38_000 + offset * 1_000,
          contextWindow: 98_304,
          contextPercent: (38_000 + offset * 1_000) / 98_304 * 100,
          streamChunks: 0,
          rawEventCount: 1,
          foldedEventTypes: ["model/execution"],
          rawRecords: [{ type: "assistant/message", time, data: { message: { content: [{ type: "text", text: `Live result ${offset + 1}` }] } } }],
          raw: { type: "assistant/message", time, data: { message: { content: [{ type: "text", text: `Live result ${offset + 1}` }] } } },
        };
      });
      const liveEdges = liveEvents.map((event, offset) => ({
        from: offset === 0 ? events.at(-1)!.id : liveEvents[offset - 1]!.id,
        to: event.id,
        kind: "same" as const,
        label: "live",
      }));
      return {
        ...replay,
        session: { ...session, status: "live", updatedAt: liveEvents.at(-1)?.time ?? session.updatedAt },
        endTime: liveEvents.at(-1)?.time ?? replay.endTime,
        events: [...events, ...liveEvents],
        edges: [...replay.edges, ...liveEdges],
      } satisfies TraceReplay;
    }
    if (command === "get_snapshot") return snapshot;
    if (command === "get_config") return config;
    if (command === "plugin:app|version") return "0.1.1-dev";
    return { ok: true, message: `${command} completed in development fixture` };
  }, { shouldMockEvents: true });
}
