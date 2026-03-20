import { watch } from "chokidar";
import { readFile } from "node:fs/promises";
import { join, basename } from "node:path";

export type InboxCallback = (from: string, message: string, file: string) => void;

export function watchInbox(storeRoot: string, callback: InboxCallback): () => void {
  const inboxDir = join(storeRoot, "inbox");

  const watcher = watch(inboxDir, {
    ignoreInitial: true,
    awaitWriteFinish: { stabilityThreshold: 500 },
  });

  watcher.on("add", async (filePath) => {
    if (!filePath.endsWith(".md")) return;
    try {
      const content = await readFile(filePath, "utf-8");
      const filename = basename(filePath, ".md");
      const parts = filename.split("_");
      const from = parts.slice(1).join("_");
      callback(from, content, filePath);
    } catch {}
  });

  watcher.on("change", async (filePath) => {
    if (!filePath.endsWith(".md")) return;
    try {
      const content = await readFile(filePath, "utf-8");
      const filename = basename(filePath, ".md");
      const parts = filename.split("_");
      const from = parts.slice(1).join("_");
      callback(from, content, filePath);
    } catch {}
  });

  return () => watcher.close();
}

export function watchContext(storeRoot: string, callback: (content: string) => void): () => void {
  const contextPath = join(storeRoot, "CONTEXT.md");

  const watcher = watch(contextPath, {
    ignoreInitial: true,
    awaitWriteFinish: { stabilityThreshold: 500 },
  });

  watcher.on("change", async () => {
    try {
      const content = await readFile(contextPath, "utf-8");
      callback(content);
    } catch {}
  });

  return () => watcher.close();
}
