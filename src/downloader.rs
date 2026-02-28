
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
    let temp_dir = current_dir.join("temp");

    let mut command = Command::new("yt-dlp");

    command
        .env("TMPDIR", &temp_dir)
        .arg("-o")
        .arg(&output_template)
        .arg("--paths")
        .arg(format!("temp:{}", temp_dir.display()))
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
        // H264 (avc1) を優先。なければ通常のbestを使いffmpegでリエンコード
        let format_selector = match quality {
            "1080p" => "bestvideo[vcodec^=avc1][height<=1080]+bestaudio[acodec^=mp4a]/bestvideo[height<=1080]+bestaudio/best",
            "720p"  => "bestvideo[vcodec^=avc1][height<=720]+bestaudio[acodec^=mp4a]/bestvideo[height<=720]+bestaudio/best",
            "360p"  => "bestvideo[vcodec^=avc1][height<=360]+bestaudio[acodec^=mp4a]/bestvideo[height<=360]+bestaudio/best",
            _       => "bestvideo[vcodec^=avc1]+bestaudio[acodec^=mp4a]/bestvideo+bestaudio/best",
        };
        command
            .arg("--merge-output-format")
            .arg(format) // mp4, webm, etc
            .arg("-f")
            .arg(format_selector)
            // H264でない場合はffmpegでリエンコード
            .arg("--postprocessor-args")
            .arg("ffmpeg:-c:v libx264 -c:a aac -movflags +faststart");
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
    let re = regex::Regex::new(r"(\d+(\.\d+)?)%").unwrap();
    let mut last_update = std::time::Instant::now();

    while stdout_reader.read_line(&mut line).await? > 0 {
        if let Some(caps) = re.captures(&line) {
            if let Some(m) = caps.get(1) {
                if let Ok(p) = m.as_str().parse::<f64>() {
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
