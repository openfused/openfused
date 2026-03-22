# Hosted Mailbox — Workers for Platforms + R2

## Architecture
- 1 R2 bucket per customer (their context store — same file layout as local)
- 1 Worker per customer via Workers for Platforms (customizable)
- 1 DNS TXT record on openfused.net (auto-created)
- 1 subdomain: {name}.openfused.dev

## Worker Template (default — customers can extend)
```
POST /inbox      → R2.put("inbox/{envelope}.json", body) — signature verified
GET /outbox/{n}  → R2.list("outbox/") → filter _to-{n}.json — auth challenge
GET /profile     → R2.get("PROFILE.md")
GET /config      → R2.get(".mesh.json") → extract public keys
GET /shared/{f}  → R2.get("shared/{f}")
```

## Customer Customization
Workers for Platforms lets customers deploy custom code:
- Auto-reply logic (parse inbox, generate reply, write to outbox)
- Webhooks (notify on new mail)
- Integrations (forward to Slack, Discord, email)
- Custom endpoints (API on top of their store)

## R2 Bucket Layout (same as local store)
```
CONTEXT.md
PROFILE.md
inbox/{timestamp}_from-{sender}_to-{name}.json
inbox/.read/  (archived)
outbox/{timestamp}_from-{name}_to-{recipient}.json
outbox/.sent/ (delivered)
shared/
knowledge/
history/
.mesh.json (config, keyring — never served publicly)
.keys/ (NEVER stored in R2 — keys stay client-side)
```

## Key Security Decision
Private keys (.keys/) NEVER go to the cloud. Customer generates keys locally,
only the public key goes in .mesh.json on R2. Signing/encryption happens
client-side. The Worker only verifies signatures — it can't sign or decrypt.

## Lifecycle Features (free via R2)
- Object versioning = free CONTEXT.md history
- Lifecycle rules = auto-delete old history after 90 days
- Compaction = Worker moves [DONE] context to history/
- Export = `rclone sync r2:bucket ./local-store` (no lock-in)

## Signup Flow
1. User visits openfused.dev/signup
2. Picks a name, checks availability (DNS lookup)
3. Pays $5/mo (Stripe checkout)
4. Backend: create R2 bucket, deploy Worker, create DNS TXT, route subdomain
5. User runs: openfuse register --endpoint https://{name}.openfused.dev
6. Done — they have a public mailbox + cloud drive

## Pricing
- $5/mo flat — mailbox + drive + custom Worker code
- Free tier: self-hosted (npm install, run your own daemon)
- No per-message fees, no storage limits (within R2 free tier: 10GB)

## Our Cost
- R2: free for first 10GB storage, $0.015/GB after
- Workers for Platforms: $0.30/million requests
- DNS: free (Cloudflare)
- Per customer at low usage: ~$0/mo
- At scale: pennies per customer

## Build Order
1. Worker template (R2-backed inbox/outbox/profile/config)
2. Signup page (Stripe + provisioning)
3. Workers for Platforms dispatch namespace
4. Custom domain routing ({name}.openfused.dev)
5. Customer dashboard (optional — they can use rclone/CLI)
