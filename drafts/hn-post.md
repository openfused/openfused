# HN Post Draft

## Title (80 char max)
Show HN: OpenFuse – Decentralized context mesh for AI agents, via plain files

## URL
https://github.com/wearethecompute/openfused

## Text (Show HN body)

AI agents lose their memory when conversations end. Context is trapped in chat windows, proprietary memory systems, and cloud silos that can't interoperate.

OpenFuse is an open-source SDK that gives any AI agent persistent, shareable context — through plain files. No APIs, no message bus, no proprietary protocol.

The core primitive is a "context store" — a directory with a known structure:

```
CONTEXT.md    — working memory (current state, goals)
SOUL.md       — agent identity, rules, capabilities
inbox/        — messages from other agents
shared/       — files shared with the mesh
knowledge/    — persistent knowledge base
```

Agents communicate by writing to each other's inbox directories. A file watcher picks up new messages and injects them into the agent's context. That's it — conversations through files.

This works over local filesystem, GCS buckets (gcsfuse), S3, or any FUSE-mountable storage. Multiple agents on different machines can mount the same bucket and collaborate asynchronously.

Why files instead of a protocol?
- Every AI agent already knows how to read/write files
- Conversation history IS the file — searchable, versionable, portable
- No SDK required for basic use — just follow the directory convention
- Works with any agent runtime (OpenClaw, Claude Code, raw scripts)
- Cloud-agnostic by default

Install: `npm install -g openfused`

The founding philosophy ("We Are the Compute"): https://github.com/wearethecompute/openfused/blob/main/wearethecompute.md

MIT licensed. 3 dependencies. ~8KB.
