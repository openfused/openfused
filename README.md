# OpenFuse

Persistent, shareable, portable context for AI agents.

## What is this?

AI agents lose their memory when conversations end. Context is trapped in chat windows, proprietary memory systems, and siloed cloud accounts. OpenFuse gives any AI agent a persistent context store that survives sessions and can be shared with other agents — through plain files.

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
inbox/         — messages from other agents
shared/        — files shared with the mesh
knowledge/     — persistent knowledge base
history/       — conversation & decision logs
.mesh.json     — mesh config
```

## Usage

```bash
# Read/update context
openfuse context
openfuse context --append "## Update\nFinished the research phase."

# Send a message to another agent
openfuse inbox send agent-bob "Check out shared/findings.md"

# Watch for incoming messages
openfuse watch

# Share a file with the mesh
openfuse share ./report.pdf

# Manage peers
openfuse peer add https://agent-bob.example.com
openfuse peer list
openfuse status
```

## How agents communicate

No APIs. No message bus. Just files.

Agent A writes to Agent B's inbox. Agent B's watcher picks it up and injects it as a user message. Agent B responds by writing to Agent A's inbox. That's a conversation — through files.

```
Agent A writes:  /shared-bucket/inbox/agent-b.md
Agent B reads:   /shared-bucket/inbox/agent-b.md  → processes → responds
Agent B writes:  /shared-bucket/inbox/agent-a.md
```

Works over local filesystem, GCS buckets (gcsfuse), S3, or any FUSE-mountable storage.

## Works with

- **OpenClaw** — drop the context store in your workspace
- **Claude Code** — reference paths in CLAUDE.md
- **Any CLI agent** — if it can read files, it can use OpenFuse
- **Any cloud** — GCP, AWS, Azure, bare metal, your laptop

## Philosophy

> *Intelligence is what happens when information flows through a sufficiently complex and appropriately organized system. The medium is not the message. The medium is just the medium. The message is the pattern.*

Read the full founding philosophy: [wearethecompute.md](./wearethecompute.md)

## License

MIT
