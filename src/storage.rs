use std::fs;
use std::path::Path;
use serde::Serialize;
use anyhow::Result;

#[derive(Debug, Serialize)]
pub struct DownloadedFile {
    pub filename: String,
    pub size: u64,
}

pub fn list_files(dir: &str) -> Result<Vec<DownloadedFile>> {
    let path = Path::new(dir);
    if !path.exists() {
        fs::create_dir_all(path)?;
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        
        if metadata.is_file() {
            if let Some(filename) = entry.file_name().to_str() {
                // 隠しファイルなどは除外してもいいかも
                if !filename.starts_with('.') {
                    files.push(DownloadedFile {
                        filename: filename.to_string(),
                        size: metadata.len(),
                    });
                }
            }
        }
    }
    
    // 新しい順などにソートすると便利だが、一旦そのまま
    Ok(files)
}
