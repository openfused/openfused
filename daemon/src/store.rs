use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

#[derive(Debug, Serialize, Deserialize)]
pub struct MeshConfig {
    pub id: String,
    pub name: String,
    pub created: String,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default, rename = "encryptionKey")]
    pub encryption_key: Option<String>,
    #[serde(default)]
    pub peers: Vec<PeerConfig>,
    #[serde(default)]
    pub trusted_keys: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerConfig {
    pub id: String,
    pub name: String,
    pub url: String,
    pub access: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

pub struct ContextStore {
    pub root: PathBuf,
}

impl ContextStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub async fn config(&self) -> Option<MeshConfig> {
        let path = self.root.join(".mesh.json");
        let data = fs::read_to_string(&path).await.ok()?;
        serde_json::from_str(&data).ok()
    }

    pub async fn list_dir(&self, subdir: &str) -> Vec<FileEntry> {
        let dir = self.root.join(subdir);
        let mut entries = Vec::new();

        if let Ok(mut reader) = fs::read_dir(&dir).await {
            while let Ok(Some(entry)) = reader.next_entry().await {
                if let Ok(meta) = entry.metadata().await {
                    entries.push(FileEntry {
                        name: entry.file_name().to_string_lossy().to_string(),
                        is_dir: meta.is_dir(),
                        size: meta.len(),
                    });
                }
            }
        }

        entries
    }

    pub async fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        // SOUL.md is private — only visible if copied to shared/
        let allowed_prefixes = ["shared/", "knowledge/", "CONTEXT.md"];
        if !allowed_prefixes.iter().any(|p| path.starts_with(p)) {
            tracing::warn!("Blocked read of restricted path: {}", path);
            return None;
        }

        if path.contains("..") {
            tracing::warn!("Blocked path traversal attempt: {}", path);
            return None;
        }

        let full_path = self.root.join(path);
        fs::read(&full_path).await.ok()
    }

    pub async fn list_root(&self) -> Vec<FileEntry> {
        let mut entries = Vec::new();

        // SOUL.md not listed — private unless agent copies it to shared/
        for name in &["CONTEXT.md", "shared", "knowledge"] {
            let path = self.root.join(name);
            if let Ok(meta) = fs::metadata(&path).await {
                entries.push(FileEntry {
                    name: name.to_string(),
                    is_dir: meta.is_dir(),
                    size: meta.len(),
                });
            }
        }

        entries
    }

    /// Write a message to the inbox directory
    pub async fn write_inbox(&self, filename: &str, content: &str) -> Result<(), std::io::Error> {
        let inbox_dir = self.root.join("inbox");
        fs::create_dir_all(&inbox_dir).await?;

        // Sanitize filename
        let safe_name = filename
            .replace(['/', '\\'], "_")
            .replace("..", "_");
        fs::write(inbox_dir.join(safe_name), content).await
    }
}
