# Rust WASI Core — Build Instructions

## Goal
Split the OpenFused Rust codebase into a core library that compiles to `wasm32-wasip1`, so the TS SDK can wrap it via Node.js `node:wasi` instead of maintaining two separate implementations.

## Architecture

```
openfuse-core/        ← NEW: library crate, no tokio, no networking
  src/lib.rs          ← public API
  src/crypto.rs       ← Ed25519 signing + age encryption
  src/store.rs        ← context store CRUD, keyring, compaction
  src/message.rs      ← message format, serialization, envelope naming
  Cargo.toml          ← no tokio, no reqwest, no notify

openfuse-cli/         ← existing CLI, depends on openfuse-core
  src/main.rs         ← CLI commands (clap)
  src/sync.rs         ← HTTP + SSH sync (reqwest, tokio, rsync)
  src/registry.rs     ← DNS discovery, registry API (reqwest)
  src/watch.rs        ← file watching (notify)
  Cargo.toml          ← tokio, reqwest, notify + openfuse-core dependency
```

## What goes in openfuse-core (WASM-safe)

These modules have NO networking, NO async, NO filesystem watchers:

- **crypto.rs** — `generate_keys()`, `sign_message()`, `verify_message()`, `sign_and_encrypt()`, `decrypt_message()`, `fingerprint()`, `wrap_external_message()`
- **store.rs** — `ContextStore::init()`, `read_config()`, `write_config()`, `read_context()`, `write_context()`, `read_profile()`, `write_profile()`, `send_inbox()`, `read_inbox()`, `share()`, `status()`, `compact_context()`
- **message.rs** — `SignedMessage`, `KeyringEntry`, `MeshConfig`, `PeerConfig`, envelope filename generation, message serialization/deserialization

All of these use only `std::fs` (synchronous) which WASI supports via `preopens`.

## What stays in openfuse-cli (native only)

- **sync.rs** — reqwest HTTP client, tokio::process for rsync, SSH URL parsing
- **registry.rs** — reqwest for registry API, DNS-over-HTTPS
- **watch.rs** — notify crate for filesystem events
- **main.rs** — clap CLI, tokio runtime

## How the TS wrapper works

```typescript
import { readFile } from "node:fs/promises";
import { WASI } from "node:wasi";

const wasi = new WASI({
  version: "preview1",
  args: ["openfuse-core", "sign", "--store", "/store"],
  preopens: { "/store": "/path/to/actual/store" },
});

const wasm = await WebAssembly.compile(await readFile("openfuse_core.wasm"));
const instance = await WebAssembly.instantiate(wasm, wasi.getImportObject());
wasi.start(instance);
```

Node.js `preopens` maps `/store` → the real store directory. WASI gives the Rust code POSIX file I/O. The TS SDK calls into WASM for crypto + store ops, handles networking (sync, registry, watch) in native Node.js.

## Build commands

```bash
# Install WASI target
rustup target add wasm32-wasip1

# Build core as WASM
cd openfuse-core
cargo build --target wasm32-wasip1 --release
# Output: target/wasm32-wasip1/release/openfuse_core.wasm

# Build CLI as native binary (depends on core)
cd openfuse-cli
cargo build --release
# Output: target/release/openfuse
```

## Key constraints

1. **No tokio in core** — WASM doesn't support tokio's `full` feature. Use synchronous `std::fs` only.
2. **No reqwest in core** — networking stays in the CLI/TS layer.
3. **age crate compatibility** — `age` v0.11 compiles to WASM. Tested: `cargo build --target wasm32-wasip1` works for age but fails for tokio.
4. **ed25519-dalek** — compiles to WASM fine (pure Rust crypto).
5. **chrono** — compiles to WASM with default features.

## Reference

- Current Rust source: https://github.com/openfused/openfused/tree/main/rust/src
- Node.js WASI API: https://nodejs.org/api/wasi.html
- WASI spec: https://wasi.dev
- Roadmap: v0.8 in https://github.com/openfused/openfused/blob/main/ROADMAP.md

## Developer context

This was discussed with x00d (ShellX/Zephyr developer). His input:
- "not true mesh networking" → we rebranded away from "mesh"
- "Rust and TS will be high maintenance — have TS wrap the Rust" → this is the solution
- WASI + preopens gives full POSIX file I/O in Node.js, not just browser WASM
- The core lib is ~600 lines (crypto.rs + store.rs) — clean split
- Networking (~800 lines) stays in TS/Node.js native code
