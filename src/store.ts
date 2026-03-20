import { readFile, writeFile, mkdir, readdir, stat } from "node:fs/promises";
import { join, resolve } from "node:path";
import { existsSync } from "node:fs";

export interface MeshConfig {
  id: string;
  name: string;
  created: string;
  peers: PeerConfig[];
}

export interface PeerConfig {
  id: string;
  name: string;
  url: string;
  access: "read" | "readwrite";
  mountPath?: string;
}

const STORE_DIRS = ["history", "knowledge", "inbox", "shared"];

export class ContextStore {
  readonly root: string;

  constructor(root: string) {
    this.root = resolve(root);
  }

  get configPath() {
    return join(this.root, ".mesh.json");
  }

  async exists(): Promise<boolean> {
    return existsSync(this.configPath);
  }

  async init(name: string, id: string): Promise<void> {
    // Create directory structure
    await mkdir(this.root, { recursive: true });
    for (const dir of STORE_DIRS) {
      await mkdir(join(this.root, dir), { recursive: true });
    }

    // Copy templates
    const templatesDir = new URL("../templates/", import.meta.url).pathname;
    for (const file of ["CONTEXT.md", "SOUL.md"]) {
      const templatePath = join(templatesDir, file);
      const destPath = join(this.root, file);
      if (!existsSync(destPath)) {
        const content = await readFile(templatePath, "utf-8");
        await writeFile(destPath, content);
      }
    }

    // Write mesh config
    const config: MeshConfig = {
      id,
      name,
      created: new Date().toISOString(),
      peers: [],
    };
    await this.writeConfig(config);
  }

  async readConfig(): Promise<MeshConfig> {
    const raw = await readFile(this.configPath, "utf-8");
    return JSON.parse(raw) as MeshConfig;
  }

  async writeConfig(config: MeshConfig): Promise<void> {
    await writeFile(this.configPath, JSON.stringify(config, null, 2) + "\n");
  }

  async readContext(): Promise<string> {
    return readFile(join(this.root, "CONTEXT.md"), "utf-8");
  }

  async writeContext(content: string): Promise<void> {
    await writeFile(join(this.root, "CONTEXT.md"), content);
  }

  async readSoul(): Promise<string> {
    return readFile(join(this.root, "SOUL.md"), "utf-8");
  }

  async writeSoul(content: string): Promise<void> {
    await writeFile(join(this.root, "SOUL.md"), content);
  }

  // --- Inbox ---

  async sendInbox(peerId: string, message: string): Promise<void> {
    const inboxDir = join(this.root, "inbox");
    const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
    const filename = `${timestamp}_${peerId}.md`;
    await writeFile(join(inboxDir, filename), message);
  }

  async readInbox(): Promise<Array<{ file: string; content: string; from: string; time: string }>> {
    const inboxDir = join(this.root, "inbox");
    if (!existsSync(inboxDir)) return [];

    const files = await readdir(inboxDir);
    const messages = [];

    for (const file of files.filter((f) => f.endsWith(".md"))) {
      const content = await readFile(join(inboxDir, file), "utf-8");
      // Parse filename: 2026-03-20T01-30-00-000Z_peer-id.md
      const parts = file.replace(".md", "").split("_");
      const from = parts.slice(1).join("_");
      const time = parts[0].replace(/-/g, (m, i) => (i < 10 ? "-" : i < 13 ? "T" : i < 19 ? ":" : ".")).replace("Z", "");
      messages.push({ file, content, from, time });
    }

    return messages.sort((a, b) => a.time.localeCompare(b.time));
  }

  // --- Shared files ---

  async listShared(): Promise<string[]> {
    const sharedDir = join(this.root, "shared");
    if (!existsSync(sharedDir)) return [];
    return readdir(sharedDir);
  }

  async share(filename: string, content: string): Promise<void> {
    const sharedDir = join(this.root, "shared");
    await mkdir(sharedDir, { recursive: true });
    await writeFile(join(sharedDir, filename), content);
  }

  // --- Status ---

  async status(): Promise<{
    id: string;
    name: string;
    peers: number;
    inboxCount: number;
    sharedCount: number;
  }> {
    const config = await this.readConfig();
    const inbox = await this.readInbox();
    const shared = await this.listShared();
    return {
      id: config.id,
      name: config.name,
      peers: config.peers.length,
      inboxCount: inbox.length,
      sharedCount: shared.length,
    };
  }
}
