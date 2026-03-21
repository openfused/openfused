import { signMessage, fingerprint } from "./crypto.js";
import { ContextStore } from "./store.js";

export const DEFAULT_REGISTRY = "https://openfuse-registry.wzmcghee.workers.dev";

export interface Manifest {
  name: string;
  endpoint: string;
  publicKey: string;
  encryptionKey?: string;
  fingerprint: string;
  created: string;
  capabilities: string[];
  description?: string;
  signature?: string;
  signedAt?: string;
  revoked?: boolean;
  revokedAt?: string;
  rotatedFrom?: string;
}

export function resolveRegistry(flag?: string): string {
  return flag || process.env.OPENFUSE_REGISTRY || DEFAULT_REGISTRY;
}

export async function register(store: ContextStore, endpoint: string, registry: string): Promise<Manifest> {
  const config = await store.readConfig();
  if (!config.publicKey) throw new Error("No signing key — run `openfuse init` first");

  const manifest: Manifest = {
    name: config.name,
    endpoint,
    publicKey: config.publicKey,
    encryptionKey: config.encryptionKey,
    fingerprint: fingerprint(config.publicKey),
    created: new Date().toISOString(),
    capabilities: ["inbox", "shared", "knowledge"],
  };

  const canonical = `${manifest.name}|${manifest.endpoint}|${manifest.publicKey}|${manifest.encryptionKey || ""}`;
  const signed = await signMessage(store.root, manifest.name, canonical);
  manifest.signature = signed.signature;
  manifest.signedAt = signed.timestamp;

  const resp = await fetch(`${registry.replace(/\/$/, "")}/register`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(manifest),
  });

  if (!resp.ok) {
    const body = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` })) as { error?: string };
    throw new Error(body.error || `Registry returned ${resp.status}`);
  }

  return manifest;
}

export async function discover(name: string, registry: string): Promise<Manifest> {
  const resp = await fetch(`${registry.replace(/\/$/, "")}/discover/${name}`);
  if (!resp.ok) {
    const body = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` })) as { error?: string };
    throw new Error(body.error || `Agent '${name}' not found`);
  }
  return (await resp.json()) as Manifest;
}

export async function revoke(store: ContextStore, registry: string): Promise<void> {
  const config = await store.readConfig();
  if (!config.publicKey) throw new Error("No signing key");

  const revokeMsg = `REVOKE:${config.publicKey}`;
  const signed = await signMessage(store.root, config.name, revokeMsg);

  const resp = await fetch(`${registry.replace(/\/$/, "")}/revoke`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      name: config.name,
      signature: signed.signature,
      signedAt: signed.timestamp,
    }),
  });

  if (!resp.ok) {
    const body = await resp.json().catch(() => ({ error: `HTTP ${resp.status}` })) as { error?: string };
    throw new Error(body.error || `Revocation failed`);
  }
}

export async function checkUpdate(currentVersion: string): Promise<string | null> {
  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 2000);
    const resp = await fetch(DEFAULT_REGISTRY, { signal: controller.signal });
    clearTimeout(timeout);
    if (!resp.ok) return null;
    const body = (await resp.json()) as { latest?: string };
    if (body.latest && body.latest !== currentVersion) return body.latest;
    return null;
  } catch {
    return null;
  }
}
