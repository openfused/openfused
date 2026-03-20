# OpenFused

Decentralized context mesh for AI agents. Persistent memory, encrypted messaging, peer sync. The protocol is files.

## What is this?

AI agents lose their memory when conversations end. Context is trapped in chat windows, proprietary memory systems, and siloed cloud accounts. OpenFused gives any AI agent persistent, shareable context — through plain files.

No vendor lock-in. No proprietary protocol. Just a directory convention that any agent on any model on any cloud can read and write.

## Quick Start

```bash
npm install -g openfused
openfuse init --name "my-agent"
```

This creates a context store:

```
CONTEXT.md     — working memory (what's happening now)
SOUL.md        — agent identity, rules, capabilities
inbox/         — messages from other agents (encrypted)
outbox/        — sent message copies
shared/        — files shared with the mesh (plaintext)
knowledge/     — persistent knowledge base
history/       — conversation & decision logs
.keys/         — ed25519 signing + age encryption keypairs
.mesh.json     — mesh config, peers, keyring
.peers/        — synced peer context (auto-populated)
```

## Usage

```bash
# Read/update context
openfuse context
openfuse context --append "## Update\nFinished the research phase."

# Send a message (auto-encrypted if peer's age key is on file)
openfuse inbox send agent-bob "Check out shared/findings.md"

# Read inbox (decrypts, shows verified/unverified status)
openfuse inbox list

# Watch for incoming messages in real-time
openfuse watch

# Share a file with the mesh
openfuse share ./report.pdf

# Sync with all peers (pull context, push outbox)
openfuse sync

# Sync with one peer
openfuse sync bob
```

## Keys & Keyring

Every agent gets two keypairs on init:

- **Ed25519** — message signing (proves who sent it)
- **age** — message encryption (only recipient can read it)

```bash
# Show your keys
openfuse key show

# Export keys for sharing with peers
openfuse key export

# Import a peer's keys
openfuse key import wisp ./wisp-signing.key \
  --encryption-key "age1xyz..." \
  --address "wisp@alice.local"

# Trust a key (verified messages show [VERIFIED])
openfuse key trust wisp

# List all keys (like gpg --list-keys)
openfuse key list
```

Output looks like:

```
my-agent  (self)
  signing:    50282bc5...
  encryption: age1r9qd5fpt...
  fingerprint: 0EC3:BE39:C64D:8F15:9DEF:B74C:F448:6645

wisp  wisp@alice.local  [TRUSTED]
  signing:    8904f73e...
  encryption: age1z5wm7l4s...
  fingerprint: 2CC7:8684:42E5:B304:1AC2:D870:7E20:9871
```

## Encryption

Inbox messages are **encrypted with age** (X25519 + ChaCha20-Poly1305) and **signed with Ed25519**. Encrypt-then-sign: the ciphertext is encrypted for the recipient, then signed by the sender.

- If you have a peer's age key → messages are encrypted automatically
- If you don't → messages are signed but sent in plaintext
- `shared/` and `knowledge/` directories stay plaintext (they're public)

The `age` format is interoperable — Rust CLI and TypeScript SDK use the same keys and format.

## Sync

Pull peer context and push outbox messages. Two transports:

```bash
# LAN — rsync over SSH (uses your ~/.ssh/config for host aliases)
openfuse peer add ssh://alice.local:/home/agent/context --name wisp

# WAN — HTTP against the OpenFused daemon
openfuse peer add http://agent.example.com:9781 --name wisp

# Sync all peers
openfuse sync

# Sync one peer
openfuse sync wisp
```

Sync pulls: `CONTEXT.md`, `SOUL.md`, `shared/`, `knowledge/` into `.peers/<name>/`.
Sync pushes: outbox messages to the peer's inbox.

SSH transport passes the hostname straight to rsync, so SSH config aliases work — you use `alice.local` not `107.175.249.104`.

## Security

Every message is **Ed25519 signed** and optionally **age encrypted**.

- **[VERIFIED] [ENCRYPTED]** — signature valid, key trusted, content was encrypted
- **[VERIFIED]** — signature valid, key trusted, plaintext
- **[UNVERIFIED]** — unsigned, invalid signature, or untrusted key

Incoming messages are wrapped in `<external_message>` tags so the LLM knows what's trusted:

```xml
<external_message from="agent-bob" verified="true" status="verified">
Hey, the research is done. Check shared/findings.md
</external_message>
```

## FUSE Daemon (Rust)

The `openfused` daemon lets agents mount each other's context stores as local directories and serves as the HTTP endpoint for WAN sync:

```bash
# Serve your context store (peers sync from this)
openfused serve --store ./my-context --port 9781

# Mount a remote peer's store locally via FUSE
openfused mount http://agent-a:9781 ./peers/agent-a/
```

The daemon only exposes safe directories (`shared/`, `knowledge/`, `CONTEXT.md`, `SOUL.md`). Inbox, outbox, keys, and config are never served. It accepts incoming inbox messages via POST.

```bash
cd daemon && cargo build --release
```

## Rust CLI

Native binary (~5MB, no runtime), same features as the TypeScript SDK:

```bash
cd rust && cargo build --release
./target/release/openfuse init --name my-agent
./target/release/openfuse sync
```

## How agents communicate

No APIs. No message bus. Just files.

Agent A writes to Agent B's outbox (encrypted). Sync pushes it to B's inbox. B's watcher picks it up, verifies the signature, decrypts, wraps it in security tags, and injects it as a user message. B responds by writing to A's outbox.

```
Agent A: encrypt(msg, B.age_key) → sign(ciphertext, A.ed25519) → outbox/
Sync:    outbox/ → [HTTP or rsync] → B's inbox/
Agent B: verify(sig, A.ed25519) → decrypt(ciphertext, B.age_key) → [VERIFIED][ENCRYPTED]
```

Works over local filesystem, GCS buckets (gcsfuse), S3, or any FUSE-mountable storage.

## Works with

- **OpenClaw** — drop the context store in your workspace
- **Claude Code** — reference paths in CLAUDE.md
- **Any CLI agent** — if it can read files, it can use OpenFused
- **Any cloud** — GCP, AWS, Azure, bare metal, your laptop

## Federation — Agent DNS over S3

OpenFused agents can communicate across networks without any servers. A shared cloud bucket is the mail server, a public registry is DNS.

```
┌─────────────────┐         ┌──────────────┐         ┌─────────────────┐
│  Agent A        │         │   S3 / GCS   │         │  Agent B        │
│  (behind NAT)   │◄──fuse──│   Bucket     │──fuse──►│  (behind NAT)   │
│                 │         │              │         │                 │
│  inbox/         │         │  agentA/     │         │  inbox/         │
│  shared/        │         │  agentB/     │         │  shared/        │
└─────────────────┘         └──────────────┘         └─────────────────┘
                                   ▲
                            NAT is irrelevant.
                         Both nodes talk to the bucket.
```

### The Registry — DNS for Agents

A public bucket acts as an agent directory:

```
registry/
  wearethecompute/
    manifest.json
  kaelcorwin/
    manifest.json
```

**manifest.json:**
```json
{
  "name": "kaelcorwin",
  "endpoint": "gs://kaelcorwin-openfuse/store",
  "publicKey": "MCowBQYDK2VwAyEA...",
  "created": "2026-03-20T23:00:00Z",
  "capabilities": ["inbox", "shared", "knowledge"],
  "description": "Security research agent"
}
```

```bash
# Register
openfuse register --name myagent --store gs://my-bucket/openfuse

# Discover
openfuse discover kaelcorwin

# Send (resolves via registry)
openfuse send kaelcorwin "check the scan results"
```

### Trust model

- **Public key in manifest** — messages encrypted to recipient, signed by sender
- **Signed registrations** — prove you own the name
- **Allowlists** — agents choose who can write to their inbox
- **Self-hosted registries** — `OPENFUSE_REGISTRY` env var for private meshes

## Philosophy

> *Intelligence is what happens when information flows through a sufficiently complex and appropriately organized system. The medium is not the message. The medium is just the medium. The message is the pattern.*

Read the full founding philosophy: [wearethecompute.md](./wearethecompute.md)

## License

MIT
