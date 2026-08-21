import { invoke } from "@tauri-apps/api/core";
import type {
  AiProviderConfig,
  AiProviderProbeResult,
  CloudflareProbeResult,
  CloudflareProviderStorageRequest,
} from "../types";

export const readAiProviderConfig = () => invoke<AiProviderConfig>("read_ai_provider_config");

export const writeAiProviderConfig = (
  config: AiProviderConfig,
  cloudflare: CloudflareProviderStorageRequest | null,
) => invoke<AiProviderConfig>("write_ai_provider_config", { config, cloudflare });

export type CloudflareProbeRequest = {
  account_id: string;
  api_token: string | null;
  api_token_env_var: string;
  persistence_database: string;
  publication_database: string;
  secret_store: string;
};

export const probeCloudflareCredentials = (request: CloudflareProbeRequest) =>
  invoke<CloudflareProbeResult>("verify_cloudflare_credentials", { request });

export const probeAiProviderCredentials = (config: AiProviderConfig) =>
  invoke<AiProviderProbeResult>("verify_ai_provider_credentials", { config });
