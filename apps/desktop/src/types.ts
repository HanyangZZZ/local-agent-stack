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
  configPath: string;
}

export interface ServiceConfig {
  url: string;
  command?: string;
  args: string[];
}

export interface StackConfig {
  ollama: ServiceConfig;
  harness: ServiceConfig;
  harnessProfile: string;
}

export interface ActionResult {
  ok: boolean;
  message: string;
}

