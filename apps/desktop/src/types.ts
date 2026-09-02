export type ServiceKind = "ollama" | "harness";
export type ServiceState = "online" | "offline" | "starting" | "error";

export interface ServiceSnapshot {
  kind: ServiceKind;
  state: ServiceState;
  url: string;
  version?: string;
  managed: boolean;
  pid?: number;
  launchUrl?: string;
  message?: string;
}

export interface ServiceLogTail {
  kind: ServiceKind;
  content: string;
  sourceBytes: number;
  lineCount: number;
  truncated: boolean;
  exists: boolean;
}

export interface InstalledModel {
  name: string;
  size: number;
  parameterSize?: string;
  quantizationLevel?: string;
}

export interface RunningModel {
  name: string;
  size: number;
  sizeVram: number;
  contextLength?: number;
  expiresAt?: string;
}

export interface StackSnapshot {
  ollama: ServiceSnapshot;
  harness: ServiceSnapshot;
  installedModels: InstalledModel[];
  runningModels: RunningModel[];
  environment: EnvironmentSnapshot;
  compatibility: CompatibilityReport;
  managedOllama: ManagedRuntimeStatus;
  managedHarness: ManagedRuntimeStatus;
  configPath: string;
}

export interface ManagedRuntimeStatus {
  installed: boolean;
  currentVersion?: string;
  previousVersion?: string;
  canRollback: boolean;
}

export type CompatibilityState = "compatible" | "outdated" | "untested" | "unknown";

export interface ComponentCompatibility {
  kind: ServiceKind;
  displayName: string;
  detectedVersion?: string;
  recommendedVersion: string;
  state: CompatibilityState;
  message: string;
  source: string;
}

export interface CompatibilityReport {
  manifestUpdatedAt: string;
  components: ComponentCompatibility[];
}

export interface EnvironmentSnapshot {
  operatingSystem: string;
  architecture: string;
  nodePath?: string;
  gitPath?: string;
  ollamaPath?: string;
  harnessPath?: string;
  gpus: GpuSnapshot[];
}

export interface GpuSnapshot {
  name: string;
  driverVersion: string;
  memoryTotalMib: number;
  memoryUsedMib: number;
  memoryFreeMib: number;
  utilizationPercent?: number;
}

export interface PullProgress {
  status: string;
  digest?: string;
  total?: number;
  completed?: number;
}

export interface RuntimeInstallProgress {
  kind: ServiceKind;
  stage: string;
  completed: number;
  total: number;
  message: string;
}

export interface AppUpdateMetadata {
  version: string;
  currentVersion: string;
}

export interface AppUpdateProgress {
  downloaded: number;
  total?: number;
  message: string;
}

export interface ServiceConfig {
  url: string;
  command?: string;
  args: string[];
}

export interface StackConfig {
  ollama: ServiceConfig;
  harness: ServiceConfig;
  harnessHome?: string;
  harnessProfile: string;
  managedHarnessNode?: string;
  managedHarnessSource?: string;
  managedHarnessEntrypoint?: string;
  setupCompleted: boolean;
  trace: TraceConfig;
}

export interface TraceConfig {
  enabled: boolean;
  sessionRoot?: string;
  gpuSampleIntervalMs: number;
  inferenceSlots: number;
}

export interface TraceSessionSummary {
  id: string;
  title: string;
  cwd: string;
  preset?: string;
  model?: string;
  createdAt: number;
  updatedAt: number;
  eventCount: number;
  branchCount: number;
  status: string;
}

export type TraceRole = "supervisor" | "subagent" | "workflow";

export interface TraceBranch {
  id: string;
  parentId?: string;
  label: string;
  role: TraceRole;
  contextId: string;
  forked: boolean;
  createdAt: number;
  removedAt?: number;
  status: string;
}

export interface TraceEvent {
  id: string;
  index: number;
  time: number;
  sequence?: number;
  branchId: string;
  eventType: string;
  label: string;
  message: string;
  turn?: number;
  step?: number;
  contextTokens?: number;
  contextWindow?: number;
  contextPercent?: number;
  outputTokens?: number;
  streamChunks: number;
  rawEventCount: number;
  foldedEventTypes: string[];
  rawRecords: unknown[];
  raw: unknown;
}

export interface TraceEdge {
  from: string;
  to: string;
  kind: "same" | "spawn" | "fork" | "report" | "consultation" | "workflow";
  label: string;
}

export interface TraceLease {
  id: string;
  slot: number;
  branchId: string;
  requestEventId: string;
  responseEventId?: string;
  requestedAt: number;
  startedAt: number;
  endedAt: number;
  contextTokens?: number;
  contextWindow?: number;
  contextPercent?: number;
  model?: string;
  source: string;
}

export interface TraceTelemetrySample {
  time: number;
  gpu?: GpuSnapshot;
  runningModels: RunningModel[];
}

export interface TraceReplay {
  session: TraceSessionSummary;
  source: string;
  maxSlots: number;
  startTime: number;
  endTime: number;
  branches: TraceBranch[];
  events: TraceEvent[];
  edges: TraceEdge[];
  leases: TraceLease[];
  telemetry: TraceTelemetrySample[];
}

export interface ActionResult {
  ok: boolean;
  message: string;
}

export type TrayAction = "show" | "startStack" | "stopManagedStack" | "releaseVram" | "quit";

export interface TrayActionFeedback extends ActionResult {
  action: TrayAction;
}
