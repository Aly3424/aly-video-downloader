use std::collections::HashMap;
use std::fs;
use std::path::Path;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use std::sync::{Mutex, OnceLock};

static METADATA_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn get_lock() -> &'static Mutex<()> {
    METADATA_LOCK.get_or_init(|| Mutex::new(()))
}

const BASE_DOWNLOAD_DIR: &str = "downloads";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedFile {
    pub filename: String,
    pub size: u64,
    pub owner: String,
    pub title: String,
    pub downloaded_at: String,
}

// =====================
// ユーザーフォルダパス
// =====================

/// ユーザー別ダウンロードディレクトリを返す（作成もする）
pub fn user_dir(username: &str) -> String {
    // ディレクトリトラバーサル防止
    let safe_name: String = username
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    format!("{}/{}", BASE_DOWNLOAD_DIR, safe_name)
}

/// downloadディレクトリ（フォルダ作成）
pub fn ensure_user_dir(username: &str) -> Result<String> {
    let dir = user_dir(username);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

// =====================
// メタデータ永続化 (ユーザーごと)
// =====================

fn metadata_path(username: &str) -> String {
    format!("{}/metadata.json", user_dir(username))
}

fn load_metadata(username: &str) -> HashMap<String, DownloadedFile> {
    if let Ok(content) = fs::read_to_string(metadata_path(username)) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        HashMap::new()
    }
}

fn save_metadata(username: &str, map: &HashMap<String, DownloadedFile>) {
    if let Ok(content) = serde_json::to_string_pretty(map) {
        let _ = fs::write(metadata_path(username), content);
    }
}

/// ダウンロード完了時にメタデータを保存する
pub fn save_file_metadata(filename: &str, size: u64, owner: &str, title: &str) {
    let _lock = get_lock().lock().unwrap();
    let mut map = load_metadata(owner);
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    map.insert(filename.to_string(), DownloadedFile {
        filename: filename.to_string(),
        size,
        owner: owner.to_string(),
        title: if title.is_empty() { filename.to_string() } else { title.to_string() },
        downloaded_at: now,
    });
    save_metadata(owner, &map);
}

/// ダウンロード一覧を取得する
/// - is_admin=true の場合は全ユーザー分を返す
pub fn list_files(owner: &str, is_admin: bool, show_all: bool) -> Result<Vec<DownloadedFile>> {
    let base = Path::new(BASE_DOWNLOAD_DIR);
    if !base.exists() {
        fs::create_dir_all(base)?;
        return Ok(Vec::new());
    }

    let mut all_files: Vec<DownloadedFile> = Vec::new();

    if is_admin && show_all {
        // 全ユーザーのサブディレクトリを走査
        for entry in fs::read_dir(base)?.flatten() {
            let meta = entry.metadata()?;
            if !meta.is_dir() { continue; }
            let uname = entry.file_name().to_string_lossy().to_string();
            all_files.extend(list_files_for_user(&uname)?);
        }
    } else {
        all_files.extend(list_files_for_user(owner)?);
    }

    // downloaded_at 降順ソート
    all_files.sort_by(|a, b| b.downloaded_at.cmp(&a.downloaded_at));
    Ok(all_files)
}

fn list_files_for_user(username: &str) -> Result<Vec<DownloadedFile>> {
    let dir_path = user_dir(username);
    let path = Path::new(&dir_path);
    if !path.exists() {
        return Ok(Vec::new());
    }

    let metadata_map = load_metadata(username);
    let mut files = Vec::new();

    for entry in fs::read_dir(path)?.flatten() {
        let meta = entry.metadata()?;
        if !meta.is_file() { continue; }
        let fname = entry.file_name().to_string_lossy().to_string();
        if fname.starts_with('.') || fname == "metadata.json" { continue; }

        let file_info = if let Some(m) = metadata_map.get(&fname) {
            m.clone()
        } else {
            DownloadedFile {
                filename: fname.clone(),
                size: meta.len(),
                owner: username.to_string(),
                title: fname.clone(),
                downloaded_at: "".to_string(),
            }
        };
        files.push(file_info);
    }
    Ok(files)
}

/// ファイル削除時にメタデータも削除する
pub fn remove_file_metadata(owner: &str, filename: &str) {
    let _lock = get_lock().lock().unwrap();
    let mut map = load_metadata(owner);
    map.remove(filename);
    save_metadata(owner, &map);
}
