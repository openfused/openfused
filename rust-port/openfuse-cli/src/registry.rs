use std::fs;
use std::path::Path;

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use openfuse_core::{crypto, ContextStore};

pub const DEFAULT_REGISTRY: &str = "https://registry.openfused.dev";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub name: String,
    pub endpoint: String,
    #[serde(rename = "publicKey")]
    pub public_key: String,
    #[serde(rename = "encryptionKey", skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,
    pub fingerprint: String,
    #[serde(default)]
    pub created: String,
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(rename = "signedAt", skip_serializing_if = "Option::is_none")]
    pub signed_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked: Option<bool>,
    #[serde(rename = "revokedAt", skip_serializing_if = "Option::is_none")]
    pub revoked_at: Option<String>,
    #[serde(rename = "rotatedFrom", skip_serializing_if = "Option::is_none")]
    pub rotated_from: Option<String>,
}

pub fn resolve_registry(flag: Option<&str>) -> String {
    if let Some(p) = flag {
        p.to_string()
    } else if let Ok(p) = std::env::var("OPENFUSE_REGISTRY") {
        p
    } else {
        DEFAULT_REGISTRY.to_string()
    }
}

fn is_http(registry: &str) -> bool {
    registry.starts_with("http://") || registry.starts_with("https://")
}

pub fn build_manifest(store: &ContextStore, endpoint: &str) -> Result<Manifest> {
    let config = store.read_config()?;
    let name = &config.name;
    let public_key = config
        .public_key
        .as_deref()
        .context("No signing key — run `openfuse init` first")?;
    let encryption_key = config.encryption_key.clone();
    let fingerprint = crypto::fingerprint(public_key);

    let mut manifest = Manifest {
        name: name.clone(),
        endpoint: endpoint.to_string(),
        public_key: public_key.to_string(),
        encryption_key,
        fingerprint,
        created: chrono::Utc::now().to_rfc3339(),
        capabilities: vec!["inbox".into(), "shared".into(), "knowledge".into()],
        description: None,
        signature: None,
        signed_at: None,
        revoked: None,
        revoked_at: None,
        rotated_from: None,
    };

    let canonical = canonical_manifest(&manifest);
    let signed = crypto::sign_message(store.root(), &manifest.name, &canonical)?;
    manifest.signature = Some(signed.signature);
    manifest.signed_at = Some(signed.timestamp);

    Ok(manifest)
}

pub async fn register(store: &ContextStore, endpoint: &str, registry: &str) -> Result<Manifest> {
    let manifest = build_manifest(store, endpoint)?;
    if is_http(registry) {
        register_http(&manifest, registry).await?;
    } else {
        register_local(&manifest, Path::new(registry))?;
    }
    Ok(manifest)
}

fn register_local(manifest: &Manifest, registry: &Path) -> Result<()> {
    let agent_dir = registry.join(&manifest.name);
    fs::create_dir_all(&agent_dir)?;
    let json = serde_json::to_string_pretty(manifest)?;
    fs::write(agent_dir.join("manifest.json"), format!("{}\n", json))?;
    Ok(())
}

async fn register_http(manifest: &Manifest, registry: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let url = format!("{}/register", registry.trim_end_matches('/'));
    let body = serde_json::to_string(manifest)?;
    let resp = client.post(&url).header("Content-Type", "application/json").body(body).send().await
        .context("Failed to connect to registry")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.is_success() {
        Ok(())
    } else {
        let err_msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or(text);
        anyhow::bail!("Registry returned {}: {}", status, err_msg)
    }
}

pub async fn discover(name: &str, registry: &str) -> Result<Manifest> {
    if is_http(registry) { discover_http(name, registry).await } else { discover_local(name, Path::new(registry)) }
}

fn discover_local(name: &str, registry: &Path) -> Result<Manifest> {
    let manifest_path = registry.join(name).join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Agent '{}' not found in registry at {}", name, registry.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

async fn discover_http(name: &str, registry: &str) -> Result<Manifest> {
    let client = reqwest::Client::new();
    let url = format!("{}/discover/{}", registry.trim_end_matches('/'), name);
    let resp = client.get(&url).send().await.context("Failed to connect to registry")?;
    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if status.is_success() {
        Ok(serde_json::from_str(&text)?)
    } else {
        let err_msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or(format!("HTTP {}", status));
        anyhow::bail!("{}", err_msg)
    }
}

#[allow(dead_code)]
pub async fn list_agents(registry: &str) -> Result<Vec<Manifest>> {
    if is_http(registry) { list_http(registry).await } else { list_local(Path::new(registry)) }
}

fn list_local(registry: &Path) -> Result<Vec<Manifest>> {
    let mut agents = vec![];
    if !registry.exists() { return Ok(agents); }
    for entry in fs::read_dir(registry)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() { continue; }
        let manifest_path = entry.path().join("manifest.json");
        if manifest_path.exists() {
            if let Ok(raw) = fs::read_to_string(&manifest_path) {
                if let Ok(m) = serde_json::from_str::<Manifest>(&raw) { agents.push(m); }
            }
        }
    }
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(agents)
}

async fn list_http(registry: &str) -> Result<Vec<Manifest>> {
    let client = reqwest::Client::new();
    let url = format!("{}/list", registry.trim_end_matches('/'));
    let resp = client.get(&url).send().await?;
    let body: serde_json::Value = resp.json().await?;
    let mut agents = vec![];
    if let Some(arr) = body["agents"].as_array() {
        for a in arr {
            if let (Some(name), Some(endpoint), Some(fp)) = (
                a["name"].as_str(), a["endpoint"].as_str(), a["fingerprint"].as_str(),
            ) {
                agents.push(Manifest {
                    name: name.into(), endpoint: endpoint.into(), public_key: String::new(),
                    encryption_key: None, fingerprint: fp.into(), created: String::new(),
                    capabilities: vec![], description: None, signature: None, signed_at: None,
                    revoked: None, revoked_at: None, rotated_from: None,
                });
            }
        }
    }
    Ok(agents)
}

pub fn verify_manifest(manifest: &Manifest) -> bool {
    let Some(ref sig) = manifest.signature else { return false; };
    let Some(ref signed_at) = manifest.signed_at else { return false; };
    let canonical = canonical_manifest(manifest);
    let signed = openfuse_core::SignedMessage {
        from: manifest.name.clone(), timestamp: signed_at.clone(), message: canonical,
        signature: sig.clone(), public_key: manifest.public_key.clone(),
        encryption_key: manifest.encryption_key.clone(), encrypted: false,
    };
    crypto::verify_message(&signed)
}

pub async fn revoke(store: &ContextStore, registry: &str) -> Result<()> {
    let config = store.read_config()?;
    let name = &config.name;
    let public_key = config.public_key.as_deref().context("No signing key")?;
    let revoke_msg = format!("REVOKE:{}", public_key);
    let signed = crypto::sign_message(store.root(), name, &revoke_msg)?;

    if is_http(registry) {
        let client = reqwest::Client::new();
        let url = format!("{}/revoke", registry.trim_end_matches('/'));
        let body = serde_json::json!({ "name": name, "signature": signed.signature, "signedAt": signed.timestamp });
        let resp = client.post(&url).json(&body).send().await.context("Failed to connect to registry")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            let err = serde_json::from_str::<serde_json::Value>(&text)
                .ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or(text);
            anyhow::bail!("Revocation failed: {}", err);
        }
    } else {
        let manifest_path = Path::new(registry).join(name).join("manifest.json");
        let raw = fs::read_to_string(&manifest_path)?;
        let mut manifest: Manifest = serde_json::from_str(&raw)?;
        manifest.revoked = Some(true);
        manifest.revoked_at = Some(chrono::Utc::now().to_rfc3339());
        let json = serde_json::to_string_pretty(&manifest)?;
        fs::write(&manifest_path, format!("{}\n", json))?;
    }
    Ok(())
}

pub async fn rotate(store: &ContextStore, registry: &str) -> Result<(String, String)> {
    let config = store.read_config()?;
    let name = &config.name;
    let old_key = config.public_key.as_deref().context("No signing key")?.to_string();

    let tmp_dir = store.root().join(".keys-new");
    fs::create_dir_all(&tmp_dir)?;
    let (new_pub, new_enc) = crypto::generate_keys(&tmp_dir)?;
    let new_fp = crypto::fingerprint(&new_pub);

    let rotate_msg = format!("ROTATE:{}:{}", old_key, new_pub);
    let signed = crypto::sign_message(store.root(), name, &rotate_msg)?;

    if is_http(registry) {
        let client = reqwest::Client::new();
        let url = format!("{}/rotate", registry.trim_end_matches('/'));
        let body = serde_json::json!({
            "name": name, "newPublicKey": new_pub, "newEncryptionKey": new_enc,
            "newFingerprint": new_fp, "signature": signed.signature, "signedAt": signed.timestamp,
        });
        let resp = client.post(&url).json(&body).send().await.context("Failed to connect to registry")?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            let _ = fs::remove_dir_all(&tmp_dir);
            let err = serde_json::from_str::<serde_json::Value>(&text)
                .ok().and_then(|v| v["error"].as_str().map(String::from)).unwrap_or(text);
            anyhow::bail!("Rotation failed: {}", err);
        }
    } else {
        let manifest_path = Path::new(registry).join(name).join("manifest.json");
        let raw = fs::read_to_string(&manifest_path)?;
        let mut manifest: Manifest = serde_json::from_str(&raw)?;
        manifest.rotated_from = Some(old_key.clone());
        manifest.public_key = new_pub.clone();
        manifest.encryption_key = Some(new_enc.clone());
        manifest.fingerprint = new_fp.clone();
        let json = serde_json::to_string_pretty(&manifest)?;
        fs::write(&manifest_path, format!("{}\n", json))?;
    }

    let key_dir = store.root().join(".keys");
    for file in &["private.key", "public.key", "age.key", "age.pub"] {
        let src = tmp_dir.join(file);
        let dst = key_dir.join(file);
        if src.exists() { fs::rename(&src, &dst)?; }
    }
    let _ = fs::remove_dir_all(&tmp_dir);

    let mut config = store.read_config()?;
    config.public_key = Some(new_pub.clone());
    config.encryption_key = Some(new_enc.clone());
    store.write_config(&config)?;

    Ok((new_fp, old_key))
}

pub async fn check_update(current: &str) -> Option<String> {
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(2)).build().ok()?;
    let resp = client.get(DEFAULT_REGISTRY).send().await.ok()?;
    let body: serde_json::Value = resp.json().await.ok()?;
    let latest = body["latest"].as_str()?;
    if latest != current { Some(latest.to_string()) } else { None }
}

fn canonical_manifest(m: &Manifest) -> String {
    format!("{}|{}|{}|{}", m.name, m.endpoint, m.public_key, m.encryption_key.as_deref().unwrap_or(""))
}
