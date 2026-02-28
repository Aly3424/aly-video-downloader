use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use anyhow::Result;

const METADATA_FILE: &str = "downloads/metadata.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedFile {
    pub filename: String,
    pub size: u64,
    pub owner: String,
    pub title: String,
    pub downloaded_at: String,
}

// =====================
// メタデータ永続化
// =====================

fn load_metadata() -> HashMap<String, DownloadedFile> {
    if let Ok(content) = fs::read_to_string(METADATA_FILE) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn save_metadata(map: &HashMap<String, DownloadedFile>) {
    if let Ok(content) = serde_json::to_string_pretty(map) {
        let _ = fs::write(METADATA_FILE, content);
    }
}

/// ダウンロード完了時にメタデータを保存する
pub fn save_file_metadata(filename: &str, size: u64, owner: &str, title: &str) {
    let mut map = load_metadata();
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    map.insert(filename.to_string(), DownloadedFile {
        filename: filename.to_string(),
        size,
        owner: owner.to_string(),
        title: if title.is_empty() { filename.to_string() } else { title.to_string() },
        downloaded_at: now,
    });
    save_metadata(&map);
}

/// ダウンロード一覧を取得する（ownerでフィルタリング、downloaded_at降順）
/// - owner が None の場合は全件返す（Admin用）
pub fn list_files(dir: &str, owner_filter: Option<&str>) -> Result<Vec<DownloadedFile>> {
    let path = Path::new(dir);
    if !path.exists() {
        fs::create_dir_all(path)?;
        return Ok(Vec::new());
    }

    let metadata_map = load_metadata();
    let mut files = Vec::new();

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if !meta.is_file() {
            continue;
        }
        let filename_os = entry.file_name();
        let filename = match filename_os.to_str() {
            Some(f) if !f.starts_with('.') && f != "metadata.json" => f.to_string(),
            _ => continue,
        };

        // メタデータがあればそちらを優先
        let file_info = if let Some(m) = metadata_map.get(&filename) {
            m.clone()
        } else {
            // メタデータがない場合はファイルシステムから構築
            DownloadedFile {
                filename: filename.clone(),
                size: meta.len(),
                owner: "unknown".to_string(),
                title: filename.clone(),
                downloaded_at: "".to_string(),
            }
        };

        // ownerフィルタリング
        if let Some(owner) = owner_filter {
            if file_info.owner != owner {
                continue;
            }
        }

        files.push(file_info);
    }

    // downloaded_at 降順ソート（空文字は末尾）
    files.sort_by(|a, b| b.downloaded_at.cmp(&a.downloaded_at));

    Ok(files)
}

/// ファイル削除時にメタデータも削除する
pub fn remove_file_metadata(filename: &str) {
    let mut map = load_metadata();
    map.remove(filename);
    save_metadata(&map);
}
