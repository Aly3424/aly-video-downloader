use std::process::Stdio;
use tokio::process::Command;
use tokio::io::{BufReader, AsyncBufReadExt};
use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Debug, Serialize, Deserialize)]
pub struct VideoInfo {
    pub id: String,
    pub title: String,
    pub url: String,
    pub extractor: String,
}

pub async fn download_video<F>(
    url: &str, 
    output_dir: &str, 
    quality: &str, 
    format: &str,
    on_progress: F
) -> Result<String> 
where
    F: Fn(f64) + Send + 'static,
{
    let output_template = format!("{}/%(title)s [%(id)s].%(ext)s", output_dir);

    let mut command = Command::new("yt-dlp");
    
    // 一時ディレクトリを絶対パスで取得
    let current_dir = std::env::current_dir()?;
    let temp_dir = current_dir.join("temp");
    
    command.env("TMPDIR", &temp_dir) // 環境変数で一時ディレクトリを指定 (FFmpeg等が使用)
        .arg("-o")
        .arg(&output_template)
        .arg("--paths") 
        .arg(format!("temp:{}", temp_dir.display())) // yt-dlpの一時ディレクトリ
        .arg("--restrict-filenames")
        .arg("--newline") // 進捗解析用
        .arg("--progress") // 進捗表示
        .arg("--js-runtimes") // JavaScriptランタイムを明示的に指定
        .arg("node")
        .arg("--cookies") // Cookieファイルを指定
        .arg("youtube-cookies.txt");

    if quality == "audio" {
        command.arg("-x")
               .arg("--audio-format")
               .arg(format) // mp3, m4a, etc
               .arg("-f")
               .arg("bestaudio/best");
    } else {
        command.arg("--merge-output-format")
               .arg(format) // mp4, webm, etc
               .arg("-f")
               .arg(match quality {
                    "1080p" => "bestvideo[height<=1080]+bestaudio/best[height<=1080]",
                    "720p" => "bestvideo[height<=720]+bestaudio/best[height<=720]",
                    "360p" => "bestvideo[height<=360]+bestaudio/best[height<=360]",
                    _ => "bestvideo+bestaudio/best",
               });
    }

    command.arg(url);
    
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = command.spawn().context("Failed to spawn yt-dlp")?;
    
    let stdout = child.stdout.take().expect("Failed to open stdout");
    let stderr = child.stderr.take().expect("Failed to open stderr");

    let mut stdout_reader = BufReader::new(stdout);
    let mut stderr_reader = BufReader::new(stderr);

    // stderrを別タスクで読み込む（デッドロック回避 & エラーログ収集 & メモリ制限）
    let stderr_task = tokio::spawn(async move {
        let mut line = String::new();
        let mut error_log = String::new();
        while stderr_reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            // ログが大きくなりすぎたら切り詰める
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
    // 正規表現で進捗をパース (例: [download]  23.5% of 10.00MiB at 1.23MiB/s ETA 00:05)
    let re = regex::Regex::new(r"(\d+(\.\d+)?)%").unwrap();
    let mut last_update = std::time::Instant::now();

    while stdout_reader.read_line(&mut line).await? > 0 {
        if let Some(caps) = re.captures(&line) {
            if let Some(m) = caps.get(1) {
                if let Ok(p) = m.as_str().parse::<f64>() {
                    // スロットリング: 100ms以上経過している場合のみ更新
                    if last_update.elapsed().as_millis() > 100 {
                        on_progress(p);
                        last_update = std::time::Instant::now();
                    }
                }
            }
        }
        line.clear();
    }

    let status = child.wait().await?;
    let error_output = stderr_task.await.unwrap_or_default();

    if status.success() {
        Ok("Download completed successfully".to_string())
    } else {
        Err(anyhow::anyhow!("Download failed: {}", error_output))
    }
}


pub async fn get_video_info(url: &str) -> Result<VideoInfo> {
    let output = Command::new("yt-dlp")
        .arg("-j") // JSON出力
        .arg("--flat-playlist") // プレイリストの場合は動画情報を取得しない（高速化）
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
