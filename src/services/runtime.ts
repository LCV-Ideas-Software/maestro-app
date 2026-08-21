import { invoke } from "@tauri-apps/api/core";
import type { BootstrapConfig, CloudflareEnvSnapshot, DependencyPreflight } from "../types";

export const dependencyPreflight = () => invoke<DependencyPreflight>("dependency_preflight");

export const readBootstrapConfig = () => invoke<BootstrapConfig>("read_bootstrap_config");

export const readCloudflareEnvSnapshot = () =>
  invoke<CloudflareEnvSnapshot>("cloudflare_env_snapshot");

export const writeBootstrapConfig = (config: BootstrapConfig) =>
  invoke<BootstrapConfig>("write_bootstrap_config", { config });

export const openDataFile = (path: string) => invoke<string>("open_data_file", { path });
