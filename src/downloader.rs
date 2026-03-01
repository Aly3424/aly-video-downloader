
use std::process::Stdio;
use tokio::process::Command;
use tokio::io::{BufReader, AsyncBufReadExt};
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    pub extractor: String,
}

/// 動画タイトルを事前取得する（メタデータ保存用）
pub async fn get_title(url: &str) -> String {
    let output = Command::new("yt-dlp")
        .arg("--get-title")
        .arg("--no-playlist")
        .arg("--cookies")
        .arg("youtube-cookies.txt")
        .arg(url)
        .output()
        .await;

    match output {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => String::new(),
    }
}

pub async fn download_video<F>(
    url: &str,
    task_id: &str, // タスクIDを追加
    output_dir: &str,
    quality: &str,
    format: &str,
    on_progress: F,
) -> Result<String>
where
    F: Fn(f64) + Send + 'static,
{
    // ファイル名テンプレート（日本語OK・重複回避のためIDを付与）
    let output_template = format!("{}/%(title)s [%(id)s].%(ext)s", output_dir);

    let current_dir = std::env::current_dir()?;
    let task_temp_dir = current_dir.join("temp").join(task_id);
    std::fs::create_dir_all(&task_temp_dir)?;

    let mut command = Command::new("yt-dlp");

    command
        .env("TMPDIR", &task_temp_dir)
        .arg("-o")
        .arg(&output_template)
        .arg("--paths")
        .arg(format!("temp:{}", task_temp_dir.display()))
        // ファイル名は制限しない（日本語OK）
        .arg("--newline")
        .arg("--progress")
        .arg("--js-runtimes")
        .arg("node")
        .arg("--cookies")
        .arg("youtube-cookies.txt");

    if quality == "audio" {
        command
            .arg("-x")
            .arg("--audio-format")
            .arg(format) // mp3, m4a, etc
            // SoundCloudのHLS（プレビュー30秒）を回避。HTTPストリームを優先。
            .arg("-f")
            .arg("bestaudio[protocol!=hls][protocol!=m3u8_native]/bestaudio[protocol=https]/bestaudio");
    } else {
        // フォーマット選択ロジックの精緻化
        let format_selector = if format == "mp4" {
            // ユーザー指定の厳密な形式: AVC1ビデオ + M4Aオーディオ優先
            match quality {
                "1080p" => "bestvideo[height<=1080][vcodec^=avc1]+bestaudio[ext=m4a]/best[height<=1080][ext=mp4]/best[height<=1080]",
                "720p"  => "bestvideo[height<=720][vcodec^=avc1]+bestaudio[ext=m4a]/best[height<=720][ext=mp4]/best[height<=720]",
                "360p"  => "bestvideo[height<=360][vcodec^=avc1]+bestaudio[ext=m4a]/best[height<=360][ext=mp4]/best[height<=360]",
                _       => "bestvideo[vcodec^=avc1]+bestaudio[ext=m4a]/best[ext=mp4]/best",
            }
        } else {
            // WebM / MKV 等: AV1 や VP9 を含めて最高画質を選択
            match quality {
                "1080p" => "bestvideo[height<=1080]+bestaudio/best[height<=1080]",
                "720p"  => "bestvideo[height<=720]+bestaudio/best[height<=720]",
                "360p"  => "bestvideo[height<=360]+bestaudio/best[height<=360]",
                _       => "bestvideo+bestaudio/best",
            }
        };
        command
            .arg("--merge-output-format")
            .arg(format) // mp4, webm, etc
            .arg("-f")
            .arg(format_selector);
    }

    command.arg(url);
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().context("Failed to spawn yt-dlp")?;

    let stdout = child.stdout.take().expect("Failed to open stdout");
    let stderr = child.stderr.take().expect("Failed to open stderr");

    let mut stdout_reader = BufReader::new(stdout);
    let mut stderr_reader = BufReader::new(stderr);

    // stderrを別タスクで読み込む（デッドロック回避）
    let stderr_task = tokio::spawn(async move {
        let mut line = String::new();
        let mut error_log = String::new();
        while stderr_reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            if error_log.len() > 10000 {
                let new_start = error_log.len().saturating_sub(5000);
                error_log = error_log.split_off(new_start);
                error_log.insert_str(0, "...(truncated)...\n");
            }
            error_log.push_str(&line);
            line.clear();
        }
        error_log
    });

    let mut line = String::new();
    let re_progress = regex::Regex::new(r"(\d+(\.\d+)?)%").unwrap();
    // ファイル名抽出用
    let re_dest = regex::Regex::new(r"\[download\] Destination: (.*)").unwrap();
    let re_merger = regex::Regex::new(r"\[Merger\] Merging formats into (.*)").unwrap();
    let re_ffmpeg = regex::Regex::new(r"\[VideoConvertor\] Converting video from .* to (.*)").unwrap();
    let re_already = regex::Regex::new(r"\[download\] (.*) has already been downloaded").unwrap();
    
    let mut final_filename = String::new();
    let mut last_update = std::time::Instant::now();

    while stdout_reader.read_line(&mut line).await? > 0 {
        // 進捗
        if let Some(caps) = re_progress.captures(&line) {
            if let Some(m) = caps.get(1) {
                if let Ok(p) = m.as_str().parse::<f64>() {
                    if last_update.elapsed().as_millis() > 100 {
                        on_progress(p);
                        last_update = std::time::Instant::now();
                    }
                }
            }
        }
        
        // ファイル名抽出
        let trimmed = line.trim();
        if let Some(caps) = re_dest.captures(trimmed) {
            final_filename = caps.get(1).map(|m| m.as_str().trim_matches('"').to_string()).unwrap_or(final_filename);
        } else if let Some(caps) = re_merger.captures(trimmed) {
            final_filename = caps.get(1).map(|m| m.as_str().trim_matches('"').to_string()).unwrap_or(final_filename);
        } else if let Some(caps) = re_ffmpeg.captures(trimmed) {
            final_filename = caps.get(1).map(|m| m.as_str().trim_matches('"').to_string()).unwrap_or(final_filename);
        } else if let Some(caps) = re_already.captures(trimmed) {
            final_filename = caps.get(1).map(|m| m.as_str().trim_matches('"').to_string()).unwrap_or(final_filename);
        }

        line.clear();
    }

    let status = child.wait().await?;
    let error_output = stderr_task.await.unwrap_or_default();

    // 一時フォルダを削除
    let _ = std::fs::remove_dir_all(&task_temp_dir);

    if status.success() {
        if final_filename.is_empty() {
            Ok("Download completed successfully".to_string())
        } else {
            // パスからファイル名のみを抽出
            let path = std::path::Path::new(&final_filename);
            let fname = path.file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "Download completed successfully".to_string());
            Ok(fname)
        }
    } else {
        Err(anyhow::anyhow!("Download failed: {}", error_output))
    }
}

pub async fn get_video_info(url: &str) -> Result<VideoInfo> {
    let output = Command::new("yt-dlp")
        .arg("-j")
        .arg("--flat-playlist")
        .arg(url)
        .output()
        .await
        .context("Failed to execute yt-dlp for metadata")?;

    if output.status.success() {
        let json_str = String::from_utf8(output.stdout)?;
        let info: serde_json::Value = serde_json::from_str(&json_str)?;

        Ok(VideoInfo {
            id: info["id"].as_str().unwrap_or("").to_string(),
            title: info["title"].as_str().unwrap_or("").to_string(),
            url: url.to_string(),
            extractor: info["extractor"].as_str().unwrap_or("Unknown").to_string(),
        })
    } else {
        Err(anyhow::anyhow!("Failed to get video info"))
    }
}
