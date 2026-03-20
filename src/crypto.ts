import { generateKeyPairSync, sign, verify, createPrivateKey, createPublicKey } from "node:crypto";
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { join } from "node:path";
import { existsSync } from "node:fs";

const KEY_DIR = ".keys";

export interface SignedMessage {
  from: string;
  timestamp: string;
  message: string;
  signature: string;
  publicKey: string;
}

export async function generateKeys(storeRoot: string): Promise<{ publicKey: string; privateKey: string }> {
  const keyDir = join(storeRoot, KEY_DIR);
  await mkdir(keyDir, { recursive: true });

  const { publicKey: pubObj, privateKey: privObj } = generateKeyPairSync("ed25519");

  const pubJwk = pubObj.export({ format: "jwk" }) as { x: string };
  const privJwk = privObj.export({ format: "jwk" }) as { d: string; x: string };

  // Store raw key bytes as hex strings (same format as Rust implementation)
  const publicHex = Buffer.from(pubJwk.x, "base64url").toString("hex");
  const privateHex = Buffer.from(privJwk.d, "base64url").toString("hex");

  await writeFile(join(keyDir, "public.key"), publicHex, { mode: 0o644 });
  await writeFile(join(keyDir, "private.key"), privateHex, { mode: 0o600 });

  return { publicKey: publicHex, privateKey: privateHex };
}

export async function hasKeys(storeRoot: string): Promise<boolean> {
  return existsSync(join(storeRoot, KEY_DIR, "private.key"));
}

async function loadPrivateKey(storeRoot: string) {
  const privHex = (await readFile(join(storeRoot, KEY_DIR, "private.key"), "utf-8")).trim();
  const pubHex = (await readFile(join(storeRoot, KEY_DIR, "public.key"), "utf-8")).trim();
  const d = Buffer.from(privHex, "hex").toString("base64url");
  const x = Buffer.from(pubHex, "hex").toString("base64url");
  return createPrivateKey({ key: { kty: "OKP", crv: "Ed25519", d, x }, format: "jwk" });
}

async function loadPublicKey(storeRoot: string): Promise<string> {
  return (await readFile(join(storeRoot, KEY_DIR, "public.key"), "utf-8")).trim();
}

export async function signMessage(storeRoot: string, from: string, message: string): Promise<SignedMessage> {
  const privateKey = await loadPrivateKey(storeRoot);
  const publicKey = await loadPublicKey(storeRoot);
  const timestamp = new Date().toISOString();

  const payload = Buffer.from(`${from}\n${timestamp}\n${message}`);
  const signature = sign(null, payload, privateKey).toString("base64");

  return { from, timestamp, message, signature, publicKey };
}

export function verifyMessage(signed: SignedMessage): boolean {
  try {
    const payload = Buffer.from(`${signed.from}\n${signed.timestamp}\n${signed.message}`);
    const x = Buffer.from(signed.publicKey.trim(), "hex").toString("base64url");
    const pubKey = createPublicKey({ key: { kty: "OKP", crv: "Ed25519", x }, format: "jwk" });
    return verify(null, payload, pubKey, Buffer.from(signed.signature, "base64"));
  } catch {
    return false;
  }
}

// Wrap a message in security tags for the LLM
export function wrapExternalMessage(signed: SignedMessage, verified: boolean): string {
  const status = verified ? "verified" : "UNVERIFIED";
  return `<external_message from="${signed.from}" verified="${verified}" time="${signed.timestamp}" status="${status}">
${signed.message}
</external_message>`;
}

// Format for writing to inbox files
export function serializeSignedMessage(signed: SignedMessage): string {
  return JSON.stringify(signed, null, 2);
}

export function deserializeSignedMessage(raw: string): SignedMessage | null {
  try {
    const parsed = JSON.parse(raw);
    if (parsed.from && parsed.message && parsed.signature && parsed.publicKey) {
      return parsed as SignedMessage;
    }
  } catch {}
  return null;
}
