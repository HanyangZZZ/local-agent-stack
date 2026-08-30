export type ServiceKind = "ollama" | "harness";
export type ServiceState = "online" | "offline" | "starting" | "error";

export interface ServiceSnapshot {
  kind: ServiceKind;
  state: ServiceState;
  url: string;
  version?: string;
  managed: boolean;
  pid?: number;
  message?: string;
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
}

export interface ActionResult {
  ok: boolean;
  message: string;
}
