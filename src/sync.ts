import { readFile, writeFile, mkdir, readdir } from "node:fs/promises";
import { join } from "node:path";
import { existsSync } from "node:fs";
import { execFile as execFileCb } from "node:child_process";
import { promisify } from "node:util";
import { ContextStore } from "./store.js";

const execFile = promisify(execFileCb);

export interface SyncResult {
  peerName: string;
  pulled: string[];
  pushed: string[];
  errors: string[];
}

interface Transport {
  type: "http" | "ssh";
  baseUrl?: string;
  host?: string;
  path?: string;
}

function parseUrl(url: string): Transport {
  if (url.startsWith("http://") || url.startsWith("https://")) {
    return { type: "http", baseUrl: url.replace(/\/$/, "") };
  } else if (url.startsWith("ssh://")) {
    const rest = url.slice(6);
    const colonIdx = rest.indexOf(":");
    if (colonIdx === -1) throw new Error("SSH URL must be ssh://host:/path");
    return { type: "ssh", host: rest.slice(0, colonIdx), path: rest.slice(colonIdx + 1) };
  }
  throw new Error(`Unknown URL scheme: ${url}. Use http:// or ssh://`);
}

export async function syncAll(store: ContextStore): Promise<SyncResult[]> {
  const config = await store.readConfig();
  const results: SyncResult[] = [];
  for (const peer of config.peers) {
    try {
      results.push(await syncPeer(store, peer));
    } catch (e: any) {
      results.push({ peerName: peer.name, pulled: [], pushed: [], errors: [e.message] });
    }
  }
  return results;
}

export async function syncOne(store: ContextStore, peerName: string): Promise<SyncResult> {
  const config = await store.readConfig();
  const peer = config.peers.find((p) => p.name === peerName || p.id === peerName);
  if (!peer) throw new Error(`Peer not found: ${peerName}`);
  return syncPeer(store, peer);
}

async function syncPeer(
  store: ContextStore,
  peer: { name: string; id: string; url: string },
): Promise<SyncResult> {
  const transport = parseUrl(peer.url);
  const peerDir = join(store.root, ".peers", peer.name);
  await mkdir(peerDir, { recursive: true });

  if (transport.type === "http") {
    return syncHttp(store, peer, transport.baseUrl!, peerDir);
  } else {
    return syncSsh(store, peer, transport.host!, transport.path!, peerDir);
  }
}

// --- HTTP sync ---

async function syncHttp(
  store: ContextStore,
  peer: { name: string; id: string },
  baseUrl: string,
  peerDir: string,
): Promise<SyncResult> {
  const pulled: string[] = [];
  const pushed: string[] = [];
  const errors: string[] = [];

  for (const file of ["CONTEXT.md"]) {
    try {
      const resp = await fetch(`${baseUrl}/read/${file}`);
      if (resp.ok) {
        await writeFile(join(peerDir, file), await resp.text());
        pulled.push(file);
      }
    } catch (e: any) {
      errors.push(`${file}: ${e.message}`);
    }
  }

  for (const dir of ["shared", "knowledge"]) {
    try {
      const resp = await fetch(`${baseUrl}/ls/${dir}`);
      if (!resp.ok) continue;
      const files = (await resp.json()) as { name: string; is_dir: boolean }[];
      const localDir = join(peerDir, dir);
      await mkdir(localDir, { recursive: true });
      for (const f of files) {
        if (f.is_dir) continue;
        const r = await fetch(`${baseUrl}/read/${dir}/${f.name}`);
        if (r.ok) {
          await writeFile(join(localDir, f.name), Buffer.from(await r.arrayBuffer()));
          pulled.push(`${dir}/${f.name}`);
        }
      }
    } catch (e: any) {
      errors.push(`${dir}/: ${e.message}`);
    }
  }

  // Push outbox → peer inbox
  const outboxDir = join(store.root, "outbox");
  if (existsSync(outboxDir)) {
    for (const fname of await readdir(outboxDir)) {
      if (!fname.endsWith(".json")) continue;
      if (!fname.includes(peer.name) && !fname.includes(peer.id)) continue;
      try {
        const body = await readFile(join(outboxDir, fname), "utf-8");
        const r = await fetch(`${baseUrl}/inbox`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body,
        });
        if (r.ok) pushed.push(fname);
        else errors.push(`push ${fname}: HTTP ${r.status}`);
      } catch (e: any) {
        errors.push(`push ${fname}: ${e.message}`);
      }
    }
  }

  await writeFile(join(peerDir, ".last-sync"), new Date().toISOString());
  return { peerName: peer.name, pulled, pushed, errors };
}

// --- SSH sync (rsync) ---

async function syncSsh(
  store: ContextStore,
  peer: { name: string; id: string },
  host: string,
  remotePath: string,
  peerDir: string,
): Promise<SyncResult> {
  const pulled: string[] = [];
  const pushed: string[] = [];
  const errors: string[] = [];

  for (const file of ["CONTEXT.md"]) {
    try {
      await execFile("rsync", ["-az", `${host}:${remotePath}/${file}`, join(peerDir, file)]);
      pulled.push(file);
    } catch (e: any) {
      errors.push(`${file}: ${e.stderr || e.message}`);
    }
  }

  for (const dir of ["shared", "knowledge"]) {
    const localDir = join(peerDir, dir);
    await mkdir(localDir, { recursive: true });
    try {
      await execFile("rsync", ["-az", "--delete", `${host}:${remotePath}/${dir}/`, `${localDir}/`]);
      pulled.push(`${dir}/`);
    } catch (e: any) {
      errors.push(`${dir}/: ${e.stderr || e.message}`);
    }
  }

  // Push outbox
  const outboxDir = join(store.root, "outbox");
  if (existsSync(outboxDir)) {
    for (const fname of await readdir(outboxDir)) {
      if (!fname.endsWith(".json")) continue;
      if (!fname.includes(peer.name) && !fname.includes(peer.id)) continue;
      try {
        await execFile("rsync", ["-az", join(outboxDir, fname), `${host}:${remotePath}/inbox/${fname}`]);
        pushed.push(fname);
      } catch (e: any) {
        errors.push(`push ${fname}: ${e.stderr || e.message}`);
      }
    }
  }

  await writeFile(join(peerDir, ".last-sync"), new Date().toISOString());
  return { peerName: peer.name, pulled, pushed, errors };
}
