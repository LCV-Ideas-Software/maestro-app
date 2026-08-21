import { invoke } from "@tauri-apps/api/core";
import type {
  BootstrapConfig,
  CloudflareEnvSnapshot,
  DependencyPreflight,
  RuntimeBootstrapActionResult,
  RuntimeBootstrapControlResult,
  RuntimeBootstrapDisposition,
  RuntimeBootstrapPlan,
} from "../types";

export const dependencyPreflight = () => invoke<DependencyPreflight>("dependency_preflight");

export const readBootstrapConfig = () => invoke<BootstrapConfig>("read_bootstrap_config");

export const readCloudflareEnvSnapshot = () =>
  invoke<CloudflareEnvSnapshot>("cloudflare_env_snapshot");

export const writeBootstrapConfig = (config: BootstrapConfig) =>
  invoke<BootstrapConfig>("write_bootstrap_config", { config });

export const openDataFile = (path: string) => invoke<string>("open_data_file", { path });

export const createRuntimeBootstrapPlan = () =>
  invoke<RuntimeBootstrapPlan>("runtime_bootstrap_plan");

export const executeRuntimeBootstrapAction = (
  actionId: string,
  planHash: string,
  approved: boolean,
) =>
  invoke<RuntimeBootstrapActionResult>("execute_runtime_bootstrap_action", {
    actionId,
    planHash,
    approved,
  });

export const controlRuntimeBootstrapAction = (
  actionId: string,
  planHash: string,
  disposition: RuntimeBootstrapDisposition,
) =>
  invoke<RuntimeBootstrapControlResult>("runtime_bootstrap_action_control", {
    actionId,
    planHash,
    disposition,
  });
