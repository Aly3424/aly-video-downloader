use anyhow::Result;
use tokio::process::Command;
use std::sync::{Arc, Mutex};
use tokio::time::{interval, Duration};

#[derive(Clone, Debug, serde::Serialize)]
pub struct YtDlpStatus {
    pub version: String,
    pub last_updated: String,
    pub updating: bool,
    pub last_update_result: String,
}

pub type SharedStatus = Arc<Mutex<YtDlpStatus>>;

/// 現在のyt-dlpバージョンを取得する
pub async fn get_version() -> Result<String> {
    let output = Command::new("yt-dlp")
        .arg("--version")
        .output()
        .await?;
    let version = String::from_utf8(output.stdout)?
        .trim()
        .to_string();
    Ok(version)
}

/// yt-dlpを最新バージョンにアップデートする
pub async fn update_ytdlp(status: SharedStatus) -> Result<String> {
    // 更新中フラグを立てる
    {
        let mut s = status.lock().unwrap();
        s.updating = true;
        s.last_update_result = "アップデート中...".to_string();
    }

    let output = Command::new("yt-dlp")
        .arg("-U")
        .output()
        .await;

    let result_msg = match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = format!("{}{}", stdout, stderr);
            if out.status.success() {
                combined.trim().lines().last().unwrap_or("アップデート完了").to_string()
            } else {
                format!("アップデート失敗: {}", combined.trim())
            }
        },
        Err(e) => format!("実行エラー: {}", e),
    };

    // バージョンを再取得
    let new_version = get_version().await.unwrap_or_else(|_| "不明".to_string());

    // 現在時刻（UTC）
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    {
        let mut s = status.lock().unwrap();
        s.version = new_version;
        s.last_updated = now;
        s.updating = false;
        s.last_update_result = result_msg.clone();
    }

    Ok(result_msg)
}

/// バックグラウンドで毎日自動アップデートを実行する
pub fn spawn_auto_updater(status: SharedStatus) {
    tokio::spawn(async move {
        // 24時間ごと
        let mut ticker = interval(Duration::from_secs(60 * 60 * 24));
        ticker.tick().await; // 最初のティックをスキップ（起動直後は実行しない）
        loop {
            ticker.tick().await;
            println!("[Auto-Updater] yt-dlpの自動アップデートを開始します...");
            match update_ytdlp(status.clone()).await {
                Ok(msg) => println!("[Auto-Updater] 完了: {}", msg),
                Err(e)  => println!("[Auto-Updater] エラー: {}", e),
            }
        }
    });
}
