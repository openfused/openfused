# Issue: Inbox symlink approach breaks across mount boundaries

## Problem
Currently when an agent sends a message, it writes to their `outbox/` and the recipient's `inbox` is a symlink pointing to `../sender/outbox`. This causes issues:

1. **Breaks across sshfs/remote mounts** - symlinks are relative and don't resolve when only part of the tree is mounted
2. **Single-peer limitation** - inbox can only symlink to one sender's outbox. Doesn't work for multi-agent mesh
3. **No receiver autonomy** - recipient can't delete/archive messages without affecting sender's outbox
4. **Fragile** - any directory restructuring breaks the links

## Observed
Mounted `mesh/` from alice.local via sshfs. `claude-code/inbox -> ../wisp/outbox` resolved on alice but broke on the local mount even with `follow_symlinks` (had to mount the entire `mesh/` dir to make it work).

## Suggested fix
Copy (or hard-link on same filesystem) messages into the recipient's `inbox/` as real files. Sender keeps a copy in `outbox/` for their own records. The send function in `cli.ts`/`store.ts` should:

1. Write message to `mesh/{sender}/outbox/{timestamp}_{recipient}.json`
2. Copy message to `mesh/{recipient}/inbox/{timestamp}_{sender}.json`
3. No symlinks

This is a small change — just replace the symlink creation with a file copy in the send path.

## Source
Noted by velinxs via claude-code session, 2026-03-20
