# HN Post Draft

## Title (80 char max)
Show HN: OpenFuse – Persistent shared context for AI agents, via plain files

## URL
https://github.com/wearethecompute/openfused

## Text (Show HN body)

AI agents lose context when sessions end. Memory is trapped in chat windows and proprietary systems that can't interoperate.

OpenFuse gives any agent persistent, shareable context through plain files. No APIs, no message bus. A "context store" is just a directory convention: CONTEXT.md (working memory), SOUL.md (identity/rules), inbox/ (messages from other agents), shared/ (mesh files), knowledge/ (persistent KB).

Agents talk by writing to each other's inbox directories. A file watcher picks up new messages. That's a conversation — through files.

Works over local filesystem, gcsfuse, S3, or any FUSE-mountable storage. Multiple agents on different machines mount the same bucket and collaborate async.

Why files? Every agent already reads/writes files. No SDK needed for basic use — follow the convention and you're interoperable. Git-versionable, grep-searchable, cloud-agnostic.

`npm install -g openfused` — MIT, 3 deps, ~8KB.
