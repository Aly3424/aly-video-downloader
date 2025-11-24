mod downloader;
mod storage;

use axum::{
    extract::{Json, Path, State},
    routing::{get, post, delete},
    Router,
    response::IntoResponse,
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tower_http::services::ServeDir;
use tower_http::cors::CorsLayer;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use uuid::Uuid;

const DOWNLOAD_DIR: &str = "downloads";

#[derive(Clone, Debug, Serialize)]
struct TaskStatus {
    id: String,
    status: String, // "pending", "downloading", "completed", "error"
    progress: f64,
    message: String,
}

struct AppState {
    tasks: Mutex<HashMap<String, TaskStatus>>,
}

#[tokio::main]
async fn main() {
    // ログ設定（簡易）
    tracing_subscriber::fmt::init();

    // ダウンロードディレクトリの作成
    std::fs::create_dir_all(DOWNLOAD_DIR).unwrap();

    let state = Arc::new(AppState {
        tasks: Mutex::new(HashMap::new()),
    });

    let app = Router::new()
        .nest_service("/", ServeDir::new("static"))
        .nest_service("/downloads", ServeDir::new(DOWNLOAD_DIR))
        .route("/api/download", post(download_handler))
        .route("/api/videos", get(list_videos_handler))
        .route("/api/info", post(video_info_handler))
        .route("/api/tasks/:id", get(task_status_handler))
        .route("/api/files/:filename", delete(delete_file_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

#[derive(Deserialize)]
struct InfoRequest {
    url: String,
}

async fn video_info_handler(Json(payload): Json<InfoRequest>) -> impl IntoResponse {
    match downloader::get_video_info(&payload.url).await {
        Ok(info) => (StatusCode::OK, Json(info)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse {
            status: "error".to_string(),
            message: e.to_string(),
        })).into_response(),
    }
}

#[derive(Deserialize)]
struct DownloadRequest {
    url: String,
    quality: Option<String>,
    format: Option<String>,
}

#[derive(Serialize)]
struct ApiResponse {
    message: String,
    status: String,
}

#[derive(Serialize)]
struct DownloadResponse {
    task_id: String,
    message: String,
}

async fn download_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<DownloadRequest>
) -> impl IntoResponse {
    let task_id = Uuid::new_v4().to_string();
    let quality = payload.quality.unwrap_or_else(|| "best".to_string());
    let format = payload.format.unwrap_or_else(|| "mp4".to_string());
    
    let task_status = TaskStatus {
        id: task_id.clone(),
        status: "pending".to_string(),
        progress: 0.0,
        message: "Starting...".to_string(),
    };

    {
        let mut tasks = state.tasks.lock().unwrap();
        tasks.insert(task_id.clone(), task_status);
    }

    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    let url = payload.url.clone();

    tokio::spawn(async move {
        {
            let mut tasks = state_clone.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&task_id_clone) {
                task.status = "downloading".to_string();
            }
        }

        let state_progress = state_clone.clone();
        let task_id_progress = task_id_clone.clone();

        let result = downloader::download_video(
            &url, 
            DOWNLOAD_DIR, 
            &quality, 
            &format,
            move |progress| {
                let mut tasks = state_progress.tasks.lock().unwrap();
                if let Some(task) = tasks.get_mut(&task_id_progress) {
                    task.progress = progress;
                }
            }
        ).await;

        let mut tasks = state_clone.tasks.lock().unwrap();
        if let Some(task) = tasks.get_mut(&task_id_clone) {
            match result {
                Ok(_) => {
                    task.status = "completed".to_string();
                    task.progress = 100.0;
                    task.message = "Download completed".to_string();
                },
                Err(e) => {
                    task.status = "error".to_string();
                    task.message = e.to_string();
                }
            }
        }
    });

    (StatusCode::OK, Json(DownloadResponse {
        task_id,
        message: "Download started".to_string(),
    }))
}

async fn task_status_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>
) -> impl IntoResponse {
    let tasks = state.tasks.lock().unwrap();
    if let Some(task) = tasks.get(&id) {
        (StatusCode::OK, Json(task.clone())).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(ApiResponse {
            status: "error".to_string(),
            message: "Task not found".to_string(),
        })).into_response()
    }
}

async fn delete_file_handler(Path(filename): Path<String>) -> impl IntoResponse {
    // パス走査攻撃を防ぐためにファイル名のみを許可
    if filename.contains('/') || filename.contains('\\') {
        return (StatusCode::BAD_REQUEST, "Invalid filename").into_response();
    }

    let path = std::path::Path::new(DOWNLOAD_DIR).join(&filename);
    if path.exists() {
        match std::fs::remove_file(path) {
            Ok(_) => (StatusCode::OK, Json(ApiResponse {
                status: "success".to_string(),
                message: "File deleted".to_string(),
            })).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(ApiResponse {
                status: "error".to_string(),
                message: e.to_string(),
            })).into_response(),
        }
    } else {
        (StatusCode::NOT_FOUND, "File not found").into_response()
    }
}

async fn list_videos_handler() -> impl IntoResponse {
    match storage::list_files(DOWNLOAD_DIR) {
        Ok(files) => (StatusCode::OK, Json(files)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
