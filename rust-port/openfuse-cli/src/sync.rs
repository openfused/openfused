use anyhow::{Context as _, Result};
use std::fs;
use std::net::ToSocketAddrs;
use std::path::Path;

use openfuse_core::{crypto, ContextStore, PeerConfig};

/// Block SSRF: reject URLs pointing to private/reserved IP ranges.
/// Resolves the hostname and checks the IP before allowing the request.
fn check_ssrf(url: &str) -> Result<()> {
    let after_scheme = url.split("://").nth(1).unwrap_or("");
    let host_port = after_scheme.split('/').next().unwrap_or("");
    let host = if host_port.starts_with('[') {
        host_port.split(']').next().unwrap_or("").trim_start_matches('[')
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    let port = host_port.rsplit(':').next().and_then(|p| p.parse::<u16>().ok()).unwrap_or(443);
    if let Ok(addrs) = format!("{}:{}", host, port).to_socket_addrs() {
        for addr in addrs {
            let ip = addr.ip();
            if ip.is_loopback() || ip.is_unspecified() {
                anyhow::bail!("SSRF blocked: {} resolves to loopback/unspecified address", host);
            }
            match ip {
                std::net::IpAddr::V4(v4) => {
                    if v4.is_private() || v4.is_link_local() || v4.octets()[0] == 169 {
                        anyhow::bail!("SSRF blocked: {} resolves to private/link-local address {}", host, v4);
                    }
                }
                std::net::IpAddr::V6(v6) => {
                    let segs = v6.segments();
                    if (segs[0] & 0xfe00) == 0xfc00 || (segs[0] & 0xffc0) == 0xfe80 {
                        anyhow::bail!("SSRF blocked: {} resolves to private IPv6 address {}", host, v6);
                    }
                    if let Some(v4) = v6.to_ipv4_mapped() {
                        if v4.is_private() || v4.is_loopback() || v4.is_link_local() {
                            anyhow::bail!("SSRF blocked: {} resolves to IPv4-mapped private address {}", host, v4);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub struct SyncResult {
    pub peer_name: String,
    pub pulled: Vec<String>,
    pub pushed: Vec<String>,
    pub errors: Vec<String>,
}

enum Transport {
    Http { base_url: String },
    Ssh { host: String, path: String },
}

fn parse_url(url: &str) -> Result<Transport> {
    if url.starts_with("http://") || url.starts_with("https://") {
        Ok(Transport::Http {
            base_url: url.trim_end_matches('/').to_string(),
        })
    } else if let Some(rest) = url.strip_prefix("ssh://") {
        let (host, path) = rest
            .split_once(':')
            .context("SSH URL must be ssh://host:/path")?;
        if host.starts_with('-') || path.starts_with('-') {
            anyhow::bail!("Invalid SSH URL: host/path cannot start with '-'");
        }
        if host.contains(';') || host.contains('|') || host.contains('`') || host.contains('$')
            || host.contains('&') || host.contains(' ') || host.contains('\n') || host.contains('\r')
            || host.contains('(') || host.contains(')') {
            anyhow::bail!("Invalid SSH URL: host contains shell metacharacters");
        }
        if path.contains(';') || path.contains('|') || path.contains('`') || path.contains('$')
            || path.contains('&') || path.contains(' ') || path.contains('\n') || path.contains('\r')
            || path.contains('(') || path.contains(')') {
            anyhow::bail!("Invalid SSH URL: path contains shell metacharacters");
        }
        Ok(Transport::Ssh {
            host: host.to_string(),
            path: path.to_string(),
        })
    } else {
        anyhow::bail!("Unknown URL scheme: {url}. Use http:// or ssh://")
    }
}

pub async fn sync_all(store: &ContextStore) -> Result<Vec<SyncResult>> {
    let config = store.read_config()?;
    let mut results = vec![];
    for peer in &config.peers {
        match sync_peer(store, peer).await {
            Ok(result) => results.push(result),
            Err(e) => results.push(SyncResult {
                peer_name: peer.name.clone(),
                pulled: vec![],
                pushed: vec![],
                errors: vec![format!("sync failed: {}", e)],
            }),
        }
    }
    Ok(results)
}

pub async fn sync_one(store: &ContextStore, peer_name: &str) -> Result<SyncResult> {
    let config = store.read_config()?;
    let peer = config
        .peers
        .iter()
        .find(|p| p.name == peer_name || p.id == peer_name)
        .context(format!("Peer not found: {}", peer_name))?
        .clone();
    sync_peer(store, &peer).await
}

async fn sync_peer(store: &ContextStore, peer: &PeerConfig) -> Result<SyncResult> {
    let transport = parse_url(&peer.url)?;
    let peer_dir = store.root().join(".peers").join(&peer.name);
    fs::create_dir_all(&peer_dir)?;
    match transport {
        Transport::Http { base_url } => sync_http(store, peer, &base_url, &peer_dir).await,
        Transport::Ssh { host, path } => sync_ssh(store, peer, &host, &path, &peer_dir).await,
    }
}

async fn sync_http(
    store: &ContextStore,
    peer: &PeerConfig,
    base_url: &str,
    peer_dir: &Path,
) -> Result<SyncResult> {
    check_ssrf(base_url)?;
    let client = reqwest::Client::new();
    let mut pulled = vec![];
    let mut pushed = vec![];
    let mut errors = vec![];

    // Pull root files — try /profile for PROFILE.md (public mode), fallback to /read/
    for file in &["CONTEXT.md", "PROFILE.md"] {
        let url = if *file == "PROFILE.md" {
            format!("{}/profile", base_url)
        } else {
            format!("{}/read/{}", base_url, file)
        };
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let raw = resp.text().await.unwrap_or_default();
                let sanitized = sanitize_peer_content(&raw);
                let wrapped = wrap_unverified_content(&peer.name, file, &sanitized);
                fs::write(peer_dir.join(file), &wrapped)?;
                pulled.push(file.to_string());
            }
            Ok(_resp) if *file == "PROFILE.md" => {
                // Fallback to /read/ path for full-mode daemons
                if let Ok(r2) = client.get(format!("{}/read/{}", base_url, file)).send().await {
                    if r2.status().is_success() {
                        let raw = r2.text().await.unwrap_or_default();
                        let sanitized = sanitize_peer_content(&raw);
                        let wrapped = wrap_unverified_content(&peer.name, file, &sanitized);
                        fs::write(peer_dir.join(file), &wrapped)?;
                        pulled.push(file.to_string());
                    }
                }
            }
            Ok(_) => {} // Don't report 404s — peer may be in public mode
            Err(e) => errors.push(format!("{}: {}", file, e)),
        }
    }

    for dir in &["shared", "knowledge"] {
        match pull_http_dir(&client, base_url, dir, peer_dir, &peer.name).await {
            Ok(files) => pulled.extend(files),
            Err(e) => errors.push(format!("{}/: {}", dir, e)),
        }
    }

    // Pull peer's outbox for messages addressed to us (authenticated)
    match pull_http_outbox(store, &client, base_url).await {
        Ok(files) => pulled.extend(files),
        Err(e) => errors.push(format!("pull outbox: {}", e)),
    }

    // Push our outbox → peer inbox
    match push_http_outbox(store, &client, base_url, peer).await {
        Ok(files) => pushed.extend(files),
        Err(e) => errors.push(format!("push: {}", e)),
    }

    fs::write(peer_dir.join(".last-sync"), chrono::Utc::now().to_rfc3339())?;

    Ok(SyncResult { peer_name: peer.name.clone(), pulled, pushed, errors })
}

#[derive(serde::Deserialize)]
struct FileEntry {
    name: String,
    is_dir: bool,
    #[allow(dead_code)]
    size: u64,
}

async fn pull_http_dir(
    client: &reqwest::Client,
    base_url: &str,
    dir: &str,
    peer_dir: &Path,
    peer_name: &str,
) -> Result<Vec<String>> {
    let mut pulled = vec![];
    let resp = client.get(format!("{}/ls/{}", base_url, dir)).send().await?;
    if !resp.status().is_success() {
        return Ok(pulled);
    }
    let files: Vec<FileEntry> = resp.json().await?;
    let local_dir = peer_dir.join(dir);
    fs::create_dir_all(&local_dir)?;
    for f in files {
        if f.is_dir { continue; }
        let safe_name = match std::path::Path::new(&f.name)
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty() && !n.contains(".."))
        {
            Some(n) => n.to_string(),
            None => continue,
        };
        let resp = client.get(format!("{}/read/{}/{}", base_url, dir, safe_name)).send().await?;
        if resp.status().is_success() {
            let raw = resp.text().await.unwrap_or_default();
            let sanitized = sanitize_peer_content(&raw);
            let wrapped = wrap_unverified_content(peer_name, &safe_name, &sanitized);
            fs::write(local_dir.join(&safe_name), &wrapped)?;
            pulled.push(format!("{}/{}", dir, safe_name));
        }
    }
    Ok(pulled)
}

/// Pull peer's outbox for messages addressed to us (authenticated).
/// Signs a challenge proving we own this name, pulls messages, then ACKs each one.
async fn pull_http_outbox(
    store: &ContextStore,
    client: &reqwest::Client,
    base_url: &str,
) -> Result<Vec<String>> {
    let mut pulled = vec![];
    let config = store.read_config()?;
    let my_name = &config.name;
    let inbox_dir = store.root().join("inbox");
    fs::create_dir_all(&inbox_dir)?;

    // Sign challenge: OUTBOX:{name}:{timestamp}
    let timestamp = chrono::Utc::now().to_rfc3339();
    let challenge = format!("OUTBOX:{}:{}", my_name, timestamp);
    let (signature, public_key) = crypto::sign_challenge(store.root(), &challenge)?;

    let resp = client
        .get(format!("{}/outbox/{}", base_url, my_name))
        .header("X-OpenFuse-PublicKey", &public_key)
        .header("X-OpenFuse-Signature", &signature)
        .header("X-OpenFuse-Timestamp", &timestamp)
        .send()
        .await;

    let resp = match resp {
        Ok(r) if r.status().is_success() => r,
        _ => return Ok(pulled),
    };

    let messages: Vec<serde_json::Value> = resp.json().await.unwrap_or_default();
    for msg in &messages {
        let ts = msg["timestamp"].as_str().unwrap_or("")
            .replace([':', '.'], "-")
            .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "");
        let from = msg["from"].as_str().unwrap_or("unknown")
            .replace(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_', "");
        let fname = format!("{}_from-{}_to-{}.json", ts, from, my_name);
        let dest = inbox_dir.join(&fname);

        if !dest.exists() {
            // Strip _outboxFile metadata before saving
            let mut clean = msg.clone();
            if let Some(obj) = clean.as_object_mut() {
                obj.remove("_outboxFile");
            }
            fs::write(&dest, serde_json::to_string_pretty(&clean)?)?;
            pulled.push(format!("outbox→{}", fname));

            // ACK: tell sender to move this message to .sent/
            if let Some(outbox_file) = msg["_outboxFile"].as_str() {
                let ack_ts = chrono::Utc::now().to_rfc3339();
                let ack_challenge = format!("ACK:{}:{}:{}", my_name, outbox_file, ack_ts);
                if let Ok((ack_sig, ack_pk)) = crypto::sign_challenge(store.root(), &ack_challenge) {
                    let _ = client
                        .delete(format!("{}/outbox/{}/{}", base_url, my_name, outbox_file))
                        .header("X-OpenFuse-PublicKey", &ack_pk)
                        .header("X-OpenFuse-Signature", &ack_sig)
                        .header("X-OpenFuse-Timestamp", &ack_ts)
                        .send()
                        .await; // best-effort ACK
                }
            }
        }
    }

    Ok(pulled)
}

async fn push_http_outbox(
    store: &ContextStore,
    client: &reqwest::Client,
    base_url: &str,
    peer: &PeerConfig,
) -> Result<Vec<String>> {
    let mut pushed = vec![];
    let outbox_dir = store.root().join("outbox");
    if !outbox_dir.exists() {
        return Ok(pushed);
    }

    // New subdir format: outbox/{name}-{fp8}/*.json
    let peer_prefix = format!("{}-", peer.name);
    for entry in fs::read_dir(&outbox_dir)? {
        let entry = entry?;
        let dir_name = entry.file_name().to_string_lossy().to_string();

        if entry.path().is_dir() && dir_name.starts_with(&peer_prefix) {
            for sub_entry in fs::read_dir(entry.path())? {
                let sub_entry = sub_entry?;
                let fname = sub_entry.file_name().to_string_lossy().to_string();
                if !fname.ends_with(".json") { continue; }
                let body = fs::read(sub_entry.path())?;
                match client
                    .post(format!("{}/inbox", base_url))
                    .header("Content-Type", "application/json")
                    .body(body)
                    .send()
                    .await
                {
                    Ok(r) if r.status().is_success() => {
                        archive_sent_subdir(&outbox_dir, &dir_name, &fname)?;
                        pushed.push(format!("{}/{}", dir_name, fname));
                    }
                    Ok(r) => anyhow::bail!("{}/{}: HTTP {}", dir_name, fname, r.status()),
                    Err(e) => anyhow::bail!("{}/{}: {}", dir_name, fname, e),
                }
            }
            continue;
        }

        // Legacy flat format: outbox/{ts}_from-{name}_to-{peer}.json
        if !dir_name.ends_with(".json") { continue; }
        if !dir_name.contains(&format!("_to-{}.json", peer.name)) && !dir_name.contains(&peer.id) {
            continue;
        }
        let body = fs::read(entry.path())?;
        match client
            .post(format!("{}/inbox", base_url))
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                archive_sent(&outbox_dir, &dir_name)?;
                pushed.push(dir_name);
            }
            Ok(r) => anyhow::bail!("{}: HTTP {}", dir_name, r.status()),
            Err(e) => anyhow::bail!("{}: {}", dir_name, e),
        }
    }
    Ok(pushed)
}

async fn sync_ssh(
    store: &ContextStore,
    peer: &PeerConfig,
    host: &str,
    remote_path: &str,
    peer_dir: &Path,
) -> Result<SyncResult> {
    let mut pulled = vec![];
    let mut pushed = vec![];
    let mut errors = vec![];

    for file in &["CONTEXT.md", "PROFILE.md"] {
        let src = format!("{}:{}/{}", host, remote_path, file);
        let dst = peer_dir.join(file);
        match rsync(&src, &dst.to_string_lossy()).await {
            Ok(_) => pulled.push(file.to_string()),
            Err(e) => errors.push(format!("{}: {}", file, e)),
        }
    }

    for dir in &["shared", "knowledge"] {
        let local = peer_dir.join(dir);
        fs::create_dir_all(&local)?;
        let src = format!("{}:{}/{}/", host, remote_path, dir);
        let dst = format!("{}/", local.to_string_lossy());
        match rsync_dir(&src, &dst).await {
            Ok(_) => pulled.push(format!("{}/", dir)),
            Err(e) => errors.push(format!("{}/: {}", dir, e)),
        }
    }

    let config = store.read_config()?;
    let my_name = &config.name;
    let inbox_dir = store.root().join("inbox");
    fs::create_dir_all(&inbox_dir)?;

    // New subdir format: pull outbox/{myName}-*/*.json into temp dir, flatten into inbox/
    let tmp_pull = store.root().join(".tmp-outbox-pull");
    {
        let _ = fs::create_dir_all(&tmp_pull);
        let src = format!("{}:{}/outbox/", host, remote_path);
        let dst = format!("{}/", tmp_pull.to_string_lossy());
        let output = tokio::process::Command::new("rsync")
            .args([
                "-az", "--ignore-existing",
                "--include", &format!("{}-*/", my_name),
                "--include", &format!("{}-*/*.json", my_name),
                "--exclude", "*",
                &src, &dst,
            ])
            .output()
            .await;
        match output {
            Ok(o) if o.status.success() => {
                // Flatten: move .json files from subdirs into inbox/
                if let Ok(entries) = fs::read_dir(&tmp_pull) {
                    for sub_entry in entries.flatten() {
                        let sub_name = sub_entry.file_name().to_string_lossy().to_string();
                        if !sub_entry.path().is_dir() || !sub_name.starts_with(&format!("{}-", my_name)) {
                            continue;
                        }
                        if let Ok(files) = fs::read_dir(sub_entry.path()) {
                            for f in files.flatten() {
                                let fname = f.file_name().to_string_lossy().to_string();
                                if !fname.ends_with(".json") { continue; }
                                let dest = inbox_dir.join(&fname);
                                if !dest.exists() {
                                    let _ = fs::rename(f.path(), &dest);
                                }
                            }
                        }
                    }
                }
                pulled.push("outbox→inbox".to_string());
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr);
                if !stderr.contains("No such file") {
                    errors.push(format!("pull outbox (subdir): {}", stderr.trim()));
                }
            }
            Err(e) => errors.push(format!("pull outbox (subdir): {}", e)),
        }
        let _ = fs::remove_dir_all(&tmp_pull);
    }

    // Legacy flat format: outbox/*_to-{name}*.json
    {
        let src = format!("{}:{}/outbox/", host, remote_path);
        let dst = format!("{}/", inbox_dir.to_string_lossy());
        let output = tokio::process::Command::new("rsync")
            .args([
                "-az", "--ignore-existing",
                "--include", &format!("*_to-{}-*.json", my_name),
                "--include", &format!("*_to-{}.json", my_name),
                "--include", "*_to-all.json",
                "--exclude", "*",
                &src, &dst,
            ])
            .output()
            .await;
        match output {
            Ok(_) => {}
            Err(e) => {
                let msg = e.to_string();
                if !msg.contains("No such file") {
                    errors.push(format!("pull outbox (legacy): {}", msg));
                }
            }
        }
    }

    let outbox_dir = store.root().join("outbox");
    let peer_prefix = format!("{}-", peer.name);
    if outbox_dir.exists() {
        for entry in fs::read_dir(&outbox_dir)? {
            let entry = entry?;
            let dir_name = entry.file_name().to_string_lossy().to_string();

            // New subdir format: outbox/{name}-{fp8}/*.json
            if entry.path().is_dir() && dir_name.starts_with(&peer_prefix) {
                for sub_entry in fs::read_dir(entry.path())? {
                    let sub_entry = sub_entry?;
                    let fname = sub_entry.file_name().to_string_lossy().to_string();
                    if !fname.ends_with(".json") { continue; }
                    let src = sub_entry.path().to_string_lossy().to_string();
                    let dst = format!("{}:{}/inbox/{}", host, remote_path, fname);
                    match rsync(&src, &dst).await {
                        Ok(_) => {
                            archive_sent_subdir(&outbox_dir, &dir_name, &fname)?;
                            pushed.push(format!("{}/{}", dir_name, fname));
                        }
                        Err(e) => errors.push(format!("push {}/{}: {}", dir_name, fname, e)),
                    }
                }
                continue;
            }

            // Legacy flat format
            if !dir_name.ends_with(".json") { continue; }
            if !dir_name.contains(&format!("_to-{}.json", peer.name)) && !dir_name.contains(&peer.id) {
                continue;
            }
            let src = entry.path().to_string_lossy().to_string();
            let dst = format!("{}:{}/inbox/{}", host, remote_path, dir_name);
            match rsync(&src, &dst).await {
                Ok(_) => {
                    archive_sent(&outbox_dir, &dir_name)?;
                    pushed.push(dir_name);
                }
                Err(e) => errors.push(format!("push {}: {}", entry.file_name().to_string_lossy(), e)),
            }
        }
    }

    fs::write(peer_dir.join(".last-sync"), chrono::Utc::now().to_rfc3339())?;

    Ok(SyncResult { peer_name: peer.name.clone(), pulled, pushed, errors })
}

fn archive_sent(outbox_dir: &Path, fname: &str) -> Result<()> {
    // Path traversal defense: verify resolved path stays under outbox_dir
    let full = outbox_dir.join(fname).canonicalize().unwrap_or_else(|_| outbox_dir.join(fname));
    let root = outbox_dir.canonicalize().unwrap_or_else(|_| outbox_dir.to_path_buf());
    if !full.starts_with(&root) {
        anyhow::bail!("Path traversal blocked: {}", fname);
    }
    let sent_dir = outbox_dir.join(".sent");
    fs::create_dir_all(&sent_dir)?;
    fs::rename(outbox_dir.join(fname), sent_dir.join(fname))?;
    Ok(())
}

fn archive_sent_subdir(outbox_dir: &Path, subdir: &str, fname: &str) -> Result<()> {
    let full = outbox_dir.join(subdir).join(fname).canonicalize()
        .unwrap_or_else(|_| outbox_dir.join(subdir).join(fname));
    let root = outbox_dir.canonicalize().unwrap_or_else(|_| outbox_dir.to_path_buf());
    if !full.starts_with(&root) {
        anyhow::bail!("Path traversal blocked: {}/{}", subdir, fname);
    }
    let sent_dir = outbox_dir.join(subdir).join(".sent");
    fs::create_dir_all(&sent_dir)?;
    fs::rename(outbox_dir.join(subdir).join(fname), sent_dir.join(fname))?;
    Ok(())
}

async fn rsync(src: &str, dst: &str) -> Result<()> {
    let output = tokio::process::Command::new("rsync")
        .args(["-az", src, dst])
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

async fn rsync_dir(src: &str, dst: &str) -> Result<()> {
    let output = tokio::process::Command::new("rsync")
        .args(["-az", "--delete", src, dst])
        .output()
        .await?;
    if !output.status.success() {
        anyhow::bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

/// Strip dangerous HTML from peer content before writing to disk.
/// Peer-synced files get read by agents/LLMs — malicious HTML could execute
/// if rendered in a browser or trick an LLM into acting on injected instructions.
fn sanitize_peer_content(raw: &str) -> String {
    use std::sync::LazyLock;
    use regex::Regex;
    static RE_SCRIPT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<script[\s\S]*?</script>").unwrap());
    static RE_IFRAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<iframe[\s\S]*?</iframe>").unwrap());
    static RE_OBJECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<object[\s\S]*?</object>").unwrap());
    static RE_EMBED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<embed[\s\S]*?>").unwrap());
    static RE_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<link[\s\S]*?>").unwrap());
    static RE_EVENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)on\w+\s*=\s*["'][^"']*["']"#).unwrap());
    static RE_JSURI: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)javascript\s*:").unwrap());

    let s = RE_SCRIPT.replace_all(raw, "[REMOVED: script tag]");
    let s = RE_IFRAME.replace_all(&s, "[REMOVED: iframe tag]");
    let s = RE_OBJECT.replace_all(&s, "[REMOVED: object tag]");
    let s = RE_EMBED.replace_all(&s, "[REMOVED: embed tag]");
    let s = RE_LINK.replace_all(&s, "[REMOVED: link tag]");
    let s = RE_EVENT.replace_all(&s, "[REMOVED: event handler]");
    let s = RE_JSURI.replace_all(&s, "[REMOVED: javascript URI]");
    s.into_owned()
}

/// Wrap peer content in unverified tags so LLMs know it's untrusted external input.
fn wrap_unverified_content(peer_name: &str, file: &str, content: &str) -> String {
    let esc = |s: &str| s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<external_content_unverified from=\"{}\" file=\"{}\">\n{}\n</external_content_unverified>",
        esc(peer_name), esc(file), content
    )
}
