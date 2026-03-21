import { generateKeyPairSync, sign, verify, createPrivateKey, createPublicKey, createHash } from "node:crypto";
import { readFile, writeFile, mkdir, chmod } from "node:fs/promises";
import { join } from "node:path";
import { existsSync } from "node:fs";
import { Encrypter, Decrypter, generateIdentity, identityToRecipient } from "age-encryption";

const KEY_DIR = ".keys";

// --- Types ---

export interface SignedMessage {
  from: string;
  timestamp: string;
  message: string;
  signature: string;
  publicKey: string;
  encrypted?: boolean;
}

export interface KeyringEntry {
  name: string;
  address: string;
  signingKey: string;
  encryptionKey?: string;
  fingerprint: string;
  trusted: boolean;
  added: string;
}

// --- Key generation ---

export async function generateKeys(storeRoot: string): Promise<{ publicKey: string; encryptionKey: string }> {
  const keyDir = join(storeRoot, KEY_DIR);
  await mkdir(keyDir, { recursive: true });

  // Ed25519 signing keypair
  const { publicKey: pubObj, privateKey: privObj } = generateKeyPairSync("ed25519");
  const pubJwk = pubObj.export({ format: "jwk" }) as { x: string };
  const privJwk = privObj.export({ format: "jwk" }) as { d: string; x: string };

  const publicHex = Buffer.from(pubJwk.x, "base64url").toString("hex");
  const privateHex = Buffer.from(privJwk.d, "base64url").toString("hex");

  await writeFile(join(keyDir, "public.key"), publicHex, { mode: 0o644 });
  await writeFile(join(keyDir, "private.key"), privateHex, { mode: 0o600 });

  // age encryption keypair
  const ageIdentity = await generateIdentity();
  const ageRecipient = await identityToRecipient(ageIdentity);

  await writeFile(join(keyDir, "age.key"), ageIdentity, { mode: 0o600 });
  await writeFile(join(keyDir, "age.pub"), ageRecipient, { mode: 0o644 });

  return { publicKey: publicHex, encryptionKey: ageRecipient };
}

export async function hasKeys(storeRoot: string): Promise<boolean> {
  return existsSync(join(storeRoot, KEY_DIR, "private.key"));
}

// --- Fingerprint ---

export function fingerprint(publicKey: string): string {
  const hash = createHash("sha256").update(publicKey).digest();
  const pairs: string[] = [];
  for (let i = 0; i < 16; i++) {
    pairs.push(hash[i].toString(16).toUpperCase().padStart(2, "0"));
  }
  const groups: string[] = [];
  for (let i = 0; i < pairs.length; i += 2) {
    groups.push(pairs[i] + pairs[i + 1]);
  }
  return groups.join(":");
}

// --- Signing ---

async function loadPrivateKey(storeRoot: string) {
  const privHex = (await readFile(join(storeRoot, KEY_DIR, "private.key"), "utf-8")).trim();
  const pubHex = (await readFile(join(storeRoot, KEY_DIR, "public.key"), "utf-8")).trim();
  const d = Buffer.from(privHex, "hex").toString("base64url");
  const x = Buffer.from(pubHex, "hex").toString("base64url");
  return createPrivateKey({ key: { kty: "OKP", crv: "Ed25519", d, x }, format: "jwk" });
}

async function loadPublicKeyHex(storeRoot: string): Promise<string> {
  return (await readFile(join(storeRoot, KEY_DIR, "public.key"), "utf-8")).trim();
}

export async function loadAgeRecipient(storeRoot: string): Promise<string> {
  return (await readFile(join(storeRoot, KEY_DIR, "age.pub"), "utf-8")).trim();
}

async function loadAgeIdentity(storeRoot: string): Promise<string> {
  return (await readFile(join(storeRoot, KEY_DIR, "age.key"), "utf-8")).trim();
}

export async function signMessage(storeRoot: string, from: string, message: string): Promise<SignedMessage> {
  const privateKey = await loadPrivateKey(storeRoot);
  const publicKey = await loadPublicKeyHex(storeRoot);
  const timestamp = new Date().toISOString();

  const payload = Buffer.from(`${from}\n${timestamp}\n${message}`);
  const signature = sign(null, payload, privateKey).toString("base64");

  return { from, timestamp, message, signature, publicKey, encrypted: false };
}

export async function signAndEncrypt(
  storeRoot: string,
  from: string,
  plaintext: string,
  recipientAgeKey: string,
): Promise<SignedMessage> {
  const ciphertext = await ageEncrypt(plaintext, recipientAgeKey);
  const encoded = Buffer.from(ciphertext).toString("base64");

  const privateKey = await loadPrivateKey(storeRoot);
  const publicKey = await loadPublicKeyHex(storeRoot);
  const timestamp = new Date().toISOString();

  const payload = Buffer.from(`${from}\n${timestamp}\n${encoded}`);
  const signature = sign(null, payload, privateKey).toString("base64");

  return { from, timestamp, message: encoded, signature, publicKey, encrypted: true };
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

export async function decryptMessage(storeRoot: string, signed: SignedMessage): Promise<string> {
  if (!signed.encrypted) return signed.message;
  const ciphertext = Buffer.from(signed.message, "base64");
  return await ageDecrypt(ciphertext, storeRoot);
}

// --- age encryption ---

async function ageEncrypt(plaintext: string, recipientKey: string): Promise<Uint8Array> {
  const e = new Encrypter();
  e.addRecipient(recipientKey);
  return await e.encrypt(plaintext);
}

async function ageDecrypt(ciphertext: Uint8Array, storeRoot: string): Promise<string> {
  const identity = await loadAgeIdentity(storeRoot);
  const d = new Decrypter();
  d.addIdentity(identity);
  return await d.decrypt(ciphertext, "text");
}

// --- Helpers ---

export function wrapExternalMessage(signed: SignedMessage, verified: boolean): string {
  const status = verified ? "verified" : "UNVERIFIED";
  return `<external_message from="${signed.from}" verified="${verified}" time="${signed.timestamp}" status="${status}">
${signed.message}
</external_message>`;
}

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
