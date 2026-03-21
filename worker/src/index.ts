/**
 * OpenFuse Registry — DNS for AI agents.
 * CF Worker + R2. Validates signed manifests, prevents name squatting.
 */

interface Env {
  REGISTRY: R2Bucket;
}

interface Manifest {
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
  rotatedFrom?: string; // previous key that authorized the rotation
}

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    const path = url.pathname;

    // CORS for browser access
    if (request.method === "OPTIONS") {
      return new Response(null, { headers: corsHeaders() });
    }

    try {
      // GET / — registry info + latest version (CLI checks this)
      if (path === "/" && request.method === "GET") {
        return json({
          service: "openfuse-registry",
          version: "0.3.0",
          latest: "0.3.0",
          changelog: "https://github.com/wearethecompute/openfused/releases",
        });
      }

      // GET /list — list all registered agents
      if (path === "/list" && request.method === "GET") {
        return await listAgents(env);
      }

      // GET /discover/:name — look up an agent
      if (path.startsWith("/discover/") && request.method === "GET") {
        const name = path.slice("/discover/".length);
        return await discoverAgent(env, name);
      }

      // POST /register — register or update an agent
      if (path === "/register" && request.method === "POST") {
        // Rate limit: 1 registration per IP per 60 seconds
        const ip = request.headers.get("CF-Connecting-IP") || "unknown";
        const rateLimitKey = `_ratelimit/${ip}`;
        const recent = await env.REGISTRY.get(rateLimitKey);
        if (recent) {
          const ts = await recent.text();
          const elapsed = Date.now() - new Date(ts).getTime();
          if (elapsed < 60_000) {
            return json({ error: "Rate limited — try again in 60 seconds" }, 429);
          }
        }

        const body = await request.text();
        const result = await registerAgent(env, body);

        if (result.status < 400) {
          await env.REGISTRY.put(rateLimitKey, new Date().toISOString());
        }

        return result;
      }

      // POST /revoke — revoke an agent's key (signed by the key being revoked)
      if (path === "/revoke" && request.method === "POST") {
        const body = await request.text();
        return await revokeAgent(env, body);
      }

      // POST /rotate — rotate to a new key (signed by the old key)
      if (path === "/rotate" && request.method === "POST") {
        const body = await request.text();
        return await rotateKey(env, body);
      }

      return json({ error: "Not found" }, 404);
    } catch (e: any) {
      return json({ error: e.message || "Internal error" }, 500);
    }
  },
};

async function listAgents(env: Env): Promise<Response> {
  const listed = await env.REGISTRY.list();
  const agents: { name: string; endpoint: string; fingerprint: string }[] = [];

  for (const obj of listed.objects) {
    if (!obj.key.endsWith("/manifest.json")) continue;
    const data = await env.REGISTRY.get(obj.key);
    if (!data) continue;
    const manifest: Manifest = JSON.parse(await data.text());
    agents.push({
      name: manifest.name,
      endpoint: manifest.endpoint,
      fingerprint: manifest.fingerprint,
    });
  }

  return json({ agents, count: agents.length });
}

async function discoverAgent(env: Env, name: string): Promise<Response> {
  const safeName = name.replace(/[^a-zA-Z0-9_-]/g, "");
  const obj = await env.REGISTRY.get(`${safeName}/manifest.json`);
  if (!obj) {
    return json({ error: `Agent '${safeName}' not found` }, 404);
  }

  const manifest: Manifest = JSON.parse(await obj.text());
  return json(manifest);
}

async function registerAgent(env: Env, body: string): Promise<Response> {
  let manifest: Manifest;
  try {
    manifest = JSON.parse(body);
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  // Validate required fields
  if (!manifest.name || !manifest.endpoint || !manifest.publicKey || !manifest.fingerprint) {
    return json({ error: "Missing required fields: name, endpoint, publicKey, fingerprint" }, 400);
  }

  if (!manifest.signature || !manifest.signedAt) {
    return json({ error: "Manifest must be signed (signature + signedAt required)" }, 400);
  }

  // Sanitize name
  const safeName = manifest.name.replace(/[^a-zA-Z0-9_-]/g, "");
  if (safeName !== manifest.name) {
    return json({ error: "Name contains invalid characters (use a-z, 0-9, -, _)" }, 400);
  }

  // Verify Ed25519 signature
  const canonical = `${manifest.name}|${manifest.endpoint}|${manifest.publicKey}|${manifest.encryptionKey || ""}`;
  const payload = `${manifest.name}\n${manifest.signedAt}\n${canonical}`;
  const valid = await verifyEd25519(payload, manifest.signature, manifest.publicKey);
  if (!valid) {
    return json({ error: "Invalid signature — manifest must be signed by the declared key" }, 403);
  }

  // Check for name squatting — if name exists, key must match
  const existing = await env.REGISTRY.get(`${safeName}/manifest.json`);
  if (existing) {
    const old: Manifest = JSON.parse(await existing.text());
    if (old.revoked) {
      return json({ error: `Name '${safeName}' has been revoked and cannot be re-registered` }, 410);
    }
    if (old.publicKey !== manifest.publicKey) {
      return json(
        { error: `Name '${safeName}' is already registered to a different key (fingerprint: ${old.fingerprint})` },
        409
      );
    }
  }

  // Write to R2
  await env.REGISTRY.put(`${safeName}/manifest.json`, JSON.stringify(manifest, null, 2));

  return json({
    ok: true,
    name: safeName,
    fingerprint: manifest.fingerprint,
    endpoint: manifest.endpoint,
  }, existing ? 200 : 201);
}

// --- Revocation ---

async function revokeAgent(env: Env, body: string): Promise<Response> {
  let req: { name: string; signature: string; signedAt: string };
  try {
    req = JSON.parse(body);
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  if (!req.name || !req.signature || !req.signedAt) {
    return json({ error: "Missing required fields: name, signature, signedAt" }, 400);
  }

  const safeName = req.name.replace(/[^a-zA-Z0-9_-]/g, "");
  const obj = await env.REGISTRY.get(`${safeName}/manifest.json`);
  if (!obj) {
    return json({ error: `Agent '${safeName}' not found` }, 404);
  }

  const manifest: Manifest = JSON.parse(await obj.text());

  if (manifest.revoked) {
    return json({ error: "Already revoked" }, 410);
  }

  // Verify: must be signed by the registered key
  const payload = `${req.name}\n${req.signedAt}\nREVOKE:${manifest.publicKey}`;
  const valid = await verifyEd25519(payload, req.signature, manifest.publicKey);
  if (!valid) {
    return json({ error: "Invalid signature — revocation must be signed by the registered key" }, 403);
  }

  // Mark as revoked
  manifest.revoked = true;
  manifest.revokedAt = new Date().toISOString();
  await env.REGISTRY.put(`${safeName}/manifest.json`, JSON.stringify(manifest, null, 2));

  return json({ ok: true, name: safeName, status: "revoked" });
}

// --- Key Rotation ---

async function rotateKey(env: Env, body: string): Promise<Response> {
  let req: {
    name: string;
    newPublicKey: string;
    newEncryptionKey?: string;
    newFingerprint: string;
    newEndpoint?: string;
    signature: string;   // signed by OLD key
    signedAt: string;
  };
  try {
    req = JSON.parse(body);
  } catch {
    return json({ error: "Invalid JSON" }, 400);
  }

  if (!req.name || !req.newPublicKey || !req.newFingerprint || !req.signature || !req.signedAt) {
    return json({ error: "Missing required fields" }, 400);
  }

  const safeName = req.name.replace(/[^a-zA-Z0-9_-]/g, "");
  const obj = await env.REGISTRY.get(`${safeName}/manifest.json`);
  if (!obj) {
    return json({ error: `Agent '${safeName}' not found` }, 404);
  }

  const manifest: Manifest = JSON.parse(await obj.text());

  if (manifest.revoked) {
    return json({ error: "Cannot rotate a revoked key" }, 410);
  }

  // Verify: rotation must be signed by the CURRENT (old) key
  const payload = `${req.name}\n${req.signedAt}\nROTATE:${manifest.publicKey}:${req.newPublicKey}`;
  const valid = await verifyEd25519(payload, req.signature, manifest.publicKey);
  if (!valid) {
    return json({ error: "Invalid signature — rotation must be signed by the current registered key" }, 403);
  }

  // Update manifest with new key
  const oldKey = manifest.publicKey;
  manifest.publicKey = req.newPublicKey;
  manifest.fingerprint = req.newFingerprint;
  manifest.rotatedFrom = oldKey;
  if (req.newEncryptionKey) {
    manifest.encryptionKey = req.newEncryptionKey;
  }
  if (req.newEndpoint) {
    manifest.endpoint = req.newEndpoint;
  }

  await env.REGISTRY.put(`${safeName}/manifest.json`, JSON.stringify(manifest, null, 2));

  return json({
    ok: true,
    name: safeName,
    oldFingerprint: manifest.rotatedFrom,
    newFingerprint: req.newFingerprint,
    status: "rotated",
  });
}

// --- Ed25519 verification using Web Crypto ---

async function verifyEd25519(message: string, signatureB64: string, publicKeyHex: string): Promise<boolean> {
  try {
    const keyBytes = hexToBytes(publicKeyHex);
    const key = await crypto.subtle.importKey(
      "raw",
      keyBytes,
      { name: "Ed25519" },
      false,
      ["verify"]
    );

    const sigBytes = base64ToBytes(signatureB64);
    const msgBytes = new TextEncoder().encode(message);

    return await crypto.subtle.verify("Ed25519", key, sigBytes, msgBytes);
  } catch {
    return false;
  }
}

function hexToBytes(hex: string): Uint8Array {
  const bytes = new Uint8Array(hex.length / 2);
  for (let i = 0; i < hex.length; i += 2) {
    bytes[i / 2] = parseInt(hex.substring(i, i + 2), 16);
  }
  return bytes;
}

function base64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

function json(data: any, status = 200): Response {
  return new Response(JSON.stringify(data, null, 2) + "\n", {
    status,
    headers: { ...corsHeaders(), "Content-Type": "application/json" },
  });
}

function corsHeaders(): Record<string, string> {
  return {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "GET, POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type",
  };
}
