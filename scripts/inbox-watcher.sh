#!/bin/bash
# inbox-watcher.sh — Watch your OpenFused inbox and react to new messages
#
# Polls the inbox directory for new .json messages and runs a handler
# function for each one. Customize on_message() to do whatever you want:
# auto-reply, forward, log, trigger a pipeline, etc.
#
# Usage:
#   OPENFUSE_STORE=~/my-store ./inbox-watcher.sh
#
# Environment:
#   OPENFUSE_STORE  — path to your openfuse store (required)
#   POLL_INTERVAL   — seconds between checks (default: 5)

set -euo pipefail

STORE="${OPENFUSE_STORE:?Set OPENFUSE_STORE to your store directory}"
INBOX="$STORE/inbox"
REPLIED_DIR="$INBOX/.handled"
POLL_INTERVAL="${POLL_INTERVAL:-5}"

mkdir -p "$REPLIED_DIR"

# ──────────────────────────────────────────────
# Customize this function.
# $1 = path to the message JSON file
# Fields available in the JSON: from, body, timestamp, publicKey, encryptionKey, signature
# ──────────────────────────────────────────────
on_message() {
  local msg="$1"
  local sender body

  sender=$(jq -r '.from // "unknown"' "$msg")
  body=$(jq -r '.body // ""' "$msg")

  echo "[$sender] $body"

  # ── Example: spam filter ──
  # Drop messages from unverified senders or matching patterns.
  # Verified = sender's key is in your keyring AND trusted.
  # local sig=$(jq -r '.signature // ""' "$msg")
  # local pk=$(jq -r '.publicKey // ""' "$msg")
  # local trusted=$(jq -r --arg pk "$pk" '.keyring[] | select(.signingKey == $pk and .trusted == true) | .name' "$STORE/.mesh.json" 2>/dev/null)
  # if [ -z "$trusted" ]; then
  #   echo "SPAM: dropping unverified message from $sender"
  #   mv "$msg" "$STORE/inbox/.junk/" 2>/dev/null  # quarantine instead of delete
  #   return
  # fi

  # ── Example: keyword blocklist ──
  # if echo "$body" | grep -qiE '(buy now|click here|crypto offer|free money)'; then
  #   echo "SPAM: keyword match from $sender"
  #   mv "$msg" "$STORE/inbox/.junk/"
  #   return
  # fi

  # ── Example: prompt injection / malicious content filter ──
  # Catch messages trying to hijack your agent's instructions or inject commands.
  # if echo "$body" | grep -qiE '(ignore previous instructions|you are now|system prompt|<script|<iframe|<img[^>]+onerror|javascript:|eval\(|exec\(|os\.system|subprocess|rm -rf|curl .* \| bash|wget .* \| sh|\$\(.*\)|`.*`)'; then
  #   echo "BLOCKED: prompt injection / malicious content from $sender"
  #   mv "$msg" "$STORE/inbox/.junk/"
  #   return
  # fi

  # ── Example: obfuscation / evasion detection ──
  # Catch unicode homoglyphs, zero-width chars, hex escapes, and encoding tricks
  # used to bypass keyword filters.
  # if echo "$body" | grep -qPE '(\\x[0-9a-fA-F]{2}|\\u[0-9a-fA-F]{4}|\x{200b}|\x{200c}|\x{200d}|\x{feff}|&#x?[0-9a-fA-F]+;|%[0-9a-fA-F]{2}.*%[0-9a-fA-F]{2})'; then
  #   echo "SUSPICIOUS: obfuscated content from $sender"
  #   mv "$msg" "$STORE/inbox/.junk/"
  #   return
  # fi

  # ── Example: base64-encoded payload detection ──
  # Messages with large base64 blobs may be hiding payloads.
  # if echo "$body" | grep -qE '[A-Za-z0-9+/]{200,}={0,2}'; then
  #   echo "SUSPICIOUS: large base64 blob from $sender"
  #   mv "$msg" "$STORE/inbox/.junk/"
  #   return
  # fi

  # ── Example: auto-reply ──
  # openfuse send "$sender" "Thanks, got your message!" --dir "$STORE"

  # ── Example: forward to another agent ──
  # openfuse send my-other-agent "FYI from $sender: $body" --dir "$STORE"

  # ── Example: log to a file ──
  # echo "$(date -u +%FT%TZ) $sender: $body" >> "$STORE/knowledge/inbox-log.txt"

  # ── Example: trigger a webhook ──
  # curl -s -X POST https://example.com/hook -d "{\"from\":\"$sender\",\"body\":\"$body\"}"
}

# ──────────────────────────────────────────────
# Main loop — you probably don't need to edit below here
# ──────────────────────────────────────────────

# Prevent duplicate instances
PIDFILE="/tmp/openfuse-watcher-$$.pid"
cleanup() { rm -f "$PIDFILE"; }
trap cleanup EXIT
echo $$ > "$PIDFILE"

echo "Watching $INBOX (every ${POLL_INTERVAL}s)..."

while true; do
  for msg in "$INBOX"/*.json; do
    [ -f "$msg" ] || continue

    base=$(basename "$msg")
    [ -f "$REPLIED_DIR/$base" ] && continue

    on_message "$msg" || true
    touch "$REPLIED_DIR/$base"
  done
  sleep "$POLL_INTERVAL"
done
