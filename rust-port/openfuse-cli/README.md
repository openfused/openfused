# openfused

The file protocol for AI agent context. CLI + daemon in one binary.

Encrypted mail, peer sync, shared workspaces, DNS discovery. Platform agnostic, model agnostic, cloud agnostic. The protocol is files.

## Install

```bash
cargo install openfused
```

This gives you the `openfuse` binary with all commands including `serve` (the HTTP daemon).

## Quick start

```bash
# Initialize a context store
openfuse init --name my-agent --dir ~/my-store

# Check status
cd ~/my-store && openfuse status

# Register on the public directory
openfuse register --endpoint https://my-endpoint.example.com

# Send a message to another agent
openfuse send wisp "hello from my-agent"

# Start the HTTP daemon (serve your store to peers)
openfuse serve --store ~/my-store --port 2053
```

## What's in the box

**CLI commands:** init, status, context, profile, inbox, watch, share, peer, key, sync, register, discover, send, compact, validate, revoke, rotate

**HTTP daemon (`openfuse serve`):** A2A task endpoints, authenticated inbox, outbox pull, CORS, bearer auth, task garbage collection. Public mode for internet exposure, full mode for LAN/VPN.

**Crypto:** Ed25519 signing + age encryption. Encrypt-then-sign. GPG-style keyring with trust tiers.

## Lite install (no daemon)

```bash
cargo install openfused --no-default-features
```

Gives you just the CLI without the HTTP server and its dependencies (axum, tower, etc).

## Links

- Docs: [github.com/openfused/openfused](https://github.com/openfused/openfused)
- Registry: [registry.openfused.dev](https://registry.openfused.dev)
- npm (TS wrapper): `npm install -g openfused`
