// WASM entry point for openfuse-core.
// This is an internal CLI called by the TypeScript wrapper via node:wasi.
// No clap — manual arg parsing to keep WASM binary small.
// All output is JSON on stdout. All filesystem access via WASI preopens.

use std::env;
use std::path::Path;
use std::process;

use openfuse_core::{crypto, store, validity};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: openfuse-core <command> [args...]");
        process::exit(1);
    }

    let store_root = env::var("OPENFUSE_STORE").unwrap_or_else(|_| "/store".to_string());
    let root = Path::new(&store_root);

    let result = match args[1].as_str() {
        "init" => cmd_init(root, &args[2..]),
        "init-workspace" => cmd_init_workspace(root, &args[2..]),
        "read-config" => cmd_read_config(root),
        "write-config" => cmd_write_config(root, &args[2..]),
        "read-context" => cmd_read_context(root),
        "write-context" => cmd_write_context(root, &args[2..]),
        "append-context" => cmd_append_context(root, &args[2..]),
        "read-profile" => cmd_read_profile(root),
        "write-profile" => cmd_write_profile(root, &args[2..]),
        "read-inbox" => cmd_read_inbox(root),
        "send-inbox" => cmd_send_inbox(root, &args[2..]),
        "archive-inbox" => cmd_archive_inbox(root, &args[2..]),
        "archive-inbox-all" => cmd_archive_inbox_all(root),
        "list-shared" => cmd_list_shared(root),
        "share" => cmd_share(root, &args[2..]),
        "status" => cmd_status(root),
        "compact" => cmd_compact(root),
        "validate" => cmd_validate(root),
        "prune-stale" => cmd_prune_stale(root),
        "sign-message" => cmd_sign_message(root, &args[2..]),
        "sign-and-encrypt" => cmd_sign_and_encrypt(root, &args[2..]),
        "verify-message" => cmd_verify_message(&args[2..]),
        "decrypt-message" => cmd_decrypt_message(root, &args[2..]),
        "generate-keys" => cmd_generate_keys(root),
        "fingerprint" => cmd_fingerprint(&args[2..]),
        "resolve-keyring" => cmd_resolve_keyring(root, &args[2..]),
        "sign-challenge" => cmd_sign_challenge(root, &args[2..]),
        other => {
            eprintln!("unknown command: {}", other);
            process::exit(1);
        }
    };

    if let Err(e) = result {
        let err = serde_json::json!({ "error": e.to_string() });
        println!("{}", err);
        process::exit(1);
    }
}

// --- Helper: print JSON to stdout ---

fn json_out<T: serde::Serialize>(val: &T) -> anyhow::Result<()> {
    println!("{}", serde_json::to_string(val)?);
    Ok(())
}

// --- Store commands ---

fn cmd_init(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let name = args.first().map(|s| s.as_str()).unwrap_or("agent");
    let id = args.get(1).map(|s| s.as_str()).unwrap_or("auto");
    store::validate_name(name, "Agent name")?;
    let s = store::ContextStore::new(root);
    s.init(name, id)?;
    let config = s.read_config()?;
    json_out(&serde_json::json!({
        "ok": true,
        "name": config.name,
        "id": config.id,
        "publicKey": config.public_key,
        "encryptionKey": config.encryption_key,
    }))
}

fn cmd_init_workspace(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let name = args.first().map(|s| s.as_str()).unwrap_or("workspace");
    let id = args.get(1).map(|s| s.as_str()).unwrap_or("auto");
    store::validate_name(name, "Workspace name")?;
    let s = store::ContextStore::new(root);
    s.init_workspace(name, id)?;
    json_out(&serde_json::json!({ "ok": true, "name": name, "id": id }))
}

fn cmd_read_config(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let config = s.read_config()?;
    json_out(&config)
}

fn cmd_write_config(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let json_str = args.first().ok_or_else(|| anyhow::anyhow!("missing config JSON"))?;
    let config: store::MeshConfig = serde_json::from_str(json_str)?;
    let s = store::ContextStore::new(root);
    s.write_config(&config)?;
    json_out(&serde_json::json!({ "ok": true }))
}

fn cmd_read_context(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let content = s.read_context()?;
    json_out(&serde_json::json!({ "content": content }))
}

fn cmd_write_context(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let content = args.first().ok_or_else(|| anyhow::anyhow!("missing content"))?;
    let s = store::ContextStore::new(root);
    s.write_context(content)?;
    json_out(&serde_json::json!({ "ok": true }))
}

fn cmd_append_context(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let text = args.first().ok_or_else(|| anyhow::anyhow!("missing text"))?;
    let s = store::ContextStore::new(root);
    let existing = s.read_context()?;
    let ts = chrono::Utc::now().to_rfc3339();
    s.write_context(&format!("{}\n<!-- openfuse:added: {} -->\n{}", existing, ts, text))?;
    json_out(&serde_json::json!({ "ok": true, "timestamp": ts }))
}

fn cmd_read_profile(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let content = s.read_profile()?;
    json_out(&serde_json::json!({ "content": content }))
}

fn cmd_write_profile(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let content = args.first().ok_or_else(|| anyhow::anyhow!("missing content"))?;
    let s = store::ContextStore::new(root);
    s.write_profile(content)?;
    json_out(&serde_json::json!({ "ok": true }))
}

fn cmd_read_inbox(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let messages = s.read_inbox()?;
    let out: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| {
            serde_json::json!({
                "file": m.file,
                "from": m.from,
                "time": m.time,
                "content": m.content,
                "wrappedContent": m.wrapped_content,
                "verified": m.verified,
                "encrypted": m.encrypted,
            })
        })
        .collect();
    json_out(&out)
}

fn cmd_send_inbox(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let peer = args.first().ok_or_else(|| anyhow::anyhow!("missing peer"))?;
    let message = args.get(1).ok_or_else(|| anyhow::anyhow!("missing message"))?;
    let from = args.get(2).ok_or_else(|| anyhow::anyhow!("missing from"))?;
    let s = store::ContextStore::new(root);
    s.send_inbox(peer, message, from)?;
    json_out(&serde_json::json!({ "ok": true }))
}

fn cmd_archive_inbox(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let filename = args.first().ok_or_else(|| anyhow::anyhow!("missing filename"))?;
    let s = store::ContextStore::new(root);
    s.archive_inbox(filename)?;
    json_out(&serde_json::json!({ "ok": true }))
}

fn cmd_archive_inbox_all(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let count = s.archive_inbox_all()?;
    json_out(&serde_json::json!({ "ok": true, "count": count }))
}

fn cmd_list_shared(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let files = s.list_shared()?;
    json_out(&files)
}

fn cmd_share(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let filename = args.first().ok_or_else(|| anyhow::anyhow!("missing filename"))?;
    let content = args.get(1).ok_or_else(|| anyhow::anyhow!("missing content"))?;
    let s = store::ContextStore::new(root);
    s.share(filename, content)?;
    json_out(&serde_json::json!({ "ok": true }))
}

fn cmd_status(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let st = s.status()?;
    json_out(&serde_json::json!({
        "id": st.id,
        "name": st.name,
        "peers": st.peers,
        "inboxCount": st.inbox_count,
        "sharedCount": st.shared_count,
    }))
}

fn cmd_compact(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let (moved, kept) = s.compact_context()?;
    json_out(&serde_json::json!({ "moved": moved, "kept": kept }))
}

fn cmd_validate(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let content = s.read_context()?;
    let report = validity::build_validity_report(&content);
    json_out(&report)
}

fn cmd_prune_stale(root: &Path) -> anyhow::Result<()> {
    let s = store::ContextStore::new(root);
    let content = s.read_context()?;
    let (pruned_content, pruned_count) = validity::prune_stale_sections(&content);
    if pruned_count > 0 {
        s.write_context(&pruned_content)?;
    }
    json_out(&serde_json::json!({ "pruned": pruned_count }))
}

// --- Crypto commands ---

fn cmd_sign_message(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let from = args.first().ok_or_else(|| anyhow::anyhow!("missing from"))?;
    let message = args.get(1).ok_or_else(|| anyhow::anyhow!("missing message"))?;
    let signed = crypto::sign_message(root, from, message)?;
    json_out(&signed)
}

fn cmd_sign_and_encrypt(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let from = args.first().ok_or_else(|| anyhow::anyhow!("missing from"))?;
    let message = args.get(1).ok_or_else(|| anyhow::anyhow!("missing message"))?;
    let recipient_key = args.get(2).ok_or_else(|| anyhow::anyhow!("missing recipient age key"))?;
    let signed = crypto::sign_and_encrypt(root, from, message, recipient_key)?;
    json_out(&signed)
}

fn cmd_verify_message(args: &[String]) -> anyhow::Result<()> {
    let json_str = args.first().ok_or_else(|| anyhow::anyhow!("missing message JSON"))?;
    let signed: crypto::SignedMessage = serde_json::from_str(json_str)?;
    let valid = crypto::verify_message(&signed);
    json_out(&serde_json::json!({ "valid": valid }))
}

fn cmd_decrypt_message(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let json_str = args.first().ok_or_else(|| anyhow::anyhow!("missing message JSON"))?;
    let signed: crypto::SignedMessage = serde_json::from_str(json_str)?;
    let plaintext = crypto::decrypt_message(root, &signed)?;
    json_out(&serde_json::json!({ "plaintext": plaintext }))
}

fn cmd_generate_keys(root: &Path) -> anyhow::Result<()> {
    let (public_key, encryption_key) = crypto::generate_keys(root)?;
    json_out(&serde_json::json!({
        "publicKey": public_key,
        "encryptionKey": encryption_key,
        "fingerprint": crypto::fingerprint(&public_key),
    }))
}

fn cmd_fingerprint(args: &[String]) -> anyhow::Result<()> {
    let key = args.first().ok_or_else(|| anyhow::anyhow!("missing public key"))?;
    let fp = crypto::fingerprint(key);
    json_out(&serde_json::json!({ "fingerprint": fp }))
}

fn cmd_resolve_keyring(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let query = args.first().ok_or_else(|| anyhow::anyhow!("missing query"))?;
    let s = store::ContextStore::new(root);
    let config = s.read_config()?;
    let entry = store::resolve_keyring(&config.keyring, query)?;
    json_out(entry)
}

fn cmd_sign_challenge(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let challenge = args.first().ok_or_else(|| anyhow::anyhow!("missing challenge"))?;
    let (signature, public_key) = crypto::sign_challenge(root, challenge)?;
    json_out(&serde_json::json!({
        "signature": signature,
        "publicKey": public_key,
    }))
}
