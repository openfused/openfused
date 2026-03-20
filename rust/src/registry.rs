use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::store::ContextStore;

/// Agent manifest — the "DNS record" for an agent in the registry.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Manifest {
    pub name: String,
    /// Where to reach this agent (http://, ssh://, gs://, s3://)
    pub endpoint: String,
    /// Ed25519 signing public key (hex)
    #[serde(rename = "publicKey")]
    pub public_key: String,
    /// age encryption public key (age1...)
    #[serde(rename = "encryptionKey", skip_serializing_if = "Option::is_none")]
    pub encryption_key: Option<String>,
    /// SHA-256 fingerprint of signing key
    pub fingerprint: String,
    pub created: String,
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Ed25519 signature over the canonical manifest (proves ownership)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Timestamp used during signing (needed to verify)
    #[serde(rename = "signedAt", skip_serializing_if = "Option::is_none")]
    pub signed_at: Option<String>,
}

/// Resolve registry path from flag, env var, or default.
pub fn registry_path(flag: Option<&str>) -> PathBuf {
    if let Some(p) = flag {
        PathBuf::from(p)
    } else if let Ok(p) = std::env::var("OPENFUSE_REGISTRY") {
        PathBuf::from(p)
    } else {
        dirs_home().join(".openfuse").join("registry")
    }
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Register this agent in the registry. Creates <registry>/<name>/manifest.json.
pub fn register(store: &ContextStore, endpoint: &str, registry: &Path) -> Result<Manifest> {
    let config = store.read_config()?;
    let name = &config.name;
    let public_key = config
        .public_key
        .as_deref()
        .context("No signing key — run `openfuse init` first")?;
    let encryption_key = config.encryption_key.clone();
    let fingerprint = crypto::fingerprint(public_key);

    let manifest = Manifest {
        name: name.clone(),
        endpoint: endpoint.to_string(),
        public_key: public_key.to_string(),
        encryption_key,
        fingerprint,
        created: chrono::Utc::now().to_rfc3339(),
        capabilities: vec![
            "inbox".to_string(),
            "shared".to_string(),
            "knowledge".to_string(),
        ],
        description: None,
        signature: None,
        signed_at: None,
    };

    // Sign the manifest (proves we own this key)
    let canonical = canonical_manifest(&manifest);
    let signed = crypto::sign_message(store.root(), &manifest.name, &canonical)?;
    let mut manifest = manifest;
    manifest.signature = Some(signed.signature);
    manifest.signed_at = Some(signed.timestamp);

    // Write to registry
    let agent_dir = registry.join(name);
    fs::create_dir_all(&agent_dir)?;
    let json = serde_json::to_string_pretty(&manifest)?;
    fs::write(agent_dir.join("manifest.json"), format!("{}\n", json))?;

    Ok(manifest)
}

/// Discover an agent by name in the registry.
pub fn discover(name: &str, registry: &Path) -> Result<Manifest> {
    let manifest_path = registry.join(name).join("manifest.json");
    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("Agent '{}' not found in registry at {}", name, registry.display()))?;
    let manifest: Manifest = serde_json::from_str(&raw)?;
    Ok(manifest)
}

/// List all agents in the registry.
pub fn list_agents(registry: &Path) -> Result<Vec<Manifest>> {
    let mut agents = vec![];
    if !registry.exists() {
        return Ok(agents);
    }
    for entry in fs::read_dir(registry)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest_path = entry.path().join("manifest.json");
        if manifest_path.exists() {
            if let Ok(raw) = fs::read_to_string(&manifest_path) {
                if let Ok(m) = serde_json::from_str::<Manifest>(&raw) {
                    agents.push(m);
                }
            }
        }
    }
    agents.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(agents)
}

/// Verify a manifest's signature (proves the registrant owns the key).
pub fn verify_manifest(manifest: &Manifest) -> bool {
    let Some(ref sig) = manifest.signature else {
        return false;
    };
    let Some(ref signed_at) = manifest.signed_at else {
        return false;
    };
    let canonical = canonical_manifest(manifest);
    let signed = crypto::SignedMessage {
        from: manifest.name.clone(),
        timestamp: signed_at.clone(),
        message: canonical,
        signature: sig.clone(),
        public_key: manifest.public_key.clone(),
        encrypted: false,
    };
    crypto::verify_message(&signed)
}

/// Canonical string representation of a manifest for signing.
/// Only includes identity fields, not the signature itself.
fn canonical_manifest(m: &Manifest) -> String {
    format!(
        "{}|{}|{}|{}",
        m.name,
        m.endpoint,
        m.public_key,
        m.encryption_key.as_deref().unwrap_or("")
    )
}
