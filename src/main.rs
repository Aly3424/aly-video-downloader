mod auth;
mod downloader;
mod storage;
mod updater;

use axum::{
    extract::{Json, Path, Query, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    routing::{delete, get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use updater::{SharedStatus, YtDlpStatus};
use uuid::Uuid;

const DOWNLOAD_DIR: &str = "downloads";

// =====================
// AppState
// =====================

#[derive(Clone, Debug, Serialize)]
struct TaskStatus {
    id: String,
    status: String,
    progress: f64,
    message: String,
    owner: String,
    title: String,
}

struct AppState {
    tasks: Mutex<HashMap<String, TaskStatus>>,
    ytdlp_status: SharedStatus,
    auth_state: Arc<auth::AuthState>,
}

// =====================
// 認証ミドルウェア
// =====================

#[derive(Clone)]
struct AuthUser {
    user_id: String,
    username: String,
    role: String,
}

/// JWTを検証してリクエストにユーザー情報を付与する
async fn auth_middleware(
    headers: HeaderMap,
    mut request: axum::extract::Request,
    next: Next,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse>)> {
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|v| v.to_string());

    let token = token.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiResponse {
                status: "error".to_string(),
                message: "認証が必要です。ログインしてください。".to_string(),
            }),
        )
    })?;

    let claims = auth::verify_token(&token).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ApiResponse {
                status: "error".to_string(),
                message: "トークンが無効または期限切れです。再ログインしてください。".to_string(),
            }),
        )
    })?;

    request.extensions_mut().insert(AuthUser {
        user_id: claims.sub,
        username: claims.username,
        role: claims.role,
    });

    Ok(next.run(request).await)
}

// =====================
// エントリーポイント
// =====================

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    std::fs::create_dir_all(DOWNLOAD_DIR).unwrap();
    std::fs::create_dir_all("temp").unwrap();

    // 初期adminユーザーを確保
    auth::ensure_admin_exists().unwrap();

    // yt-dlpバージョン取得
    let initial_version = updater::get_version()
        .await
        .unwrap_or_else(|_| "不明".to_string());
    println!("yt-dlp バージョン: {}", initial_version);

    let ytdlp_status: SharedStatus = Arc::new(Mutex::new(YtDlpStatus {
        version: initial_version,
        last_updated: "未実行".to_string(),
        updating: false,
        last_update_result: "自動アップデート待機中...".to_string(),
    }));

    updater::spawn_auto_updater(ytdlp_status.clone());

    let state = Arc::new(AppState {
        tasks: Mutex::new(HashMap::new()),
        ytdlp_status,
        auth_state: auth::AuthState::new(),
    });

    // 認証不要のルート
    let public = Router::new()
        .route("/api/auth/login", post(login_handler))
        .nest_service("/login", ServeDir::new("static").append_index_html_on_directories(false))
        .nest_service("/", ServeDir::new("static"));

    // 認証必要のルート
    let protected = Router::new()
        .route("/api/auth/me", get(me_handler))
        .route("/api/auth/password", post(change_password_handler))
        .route("/api/download", post(download_handler))
        .route("/api/videos", get(list_videos_handler))
        .route("/api/info", post(video_info_handler))
        .route("/api/tasks/:id", get(task_status_handler))
        .route("/api/files/:filename", delete(delete_file_handler))
        .route("/api/ytdlp/version", get(ytdlp_version_handler))
        .route("/api/ytdlp/update", post(ytdlp_update_handler))
        .route("/api/admin/users", get(list_users_handler))
        .route("/api/admin/users", post(create_user_handler))
        .route("/api/admin/users/:id", delete(delete_user_handler))
        .layer(middleware::from_fn(auth_middleware));

    let app = public
        .merge(protected)
        .nest_service("/downloads", ServeDir::new(DOWNLOAD_DIR))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// =====================
// 共通型
// =====================

#[derive(Serialize)]
struct ApiResponse {
    message: String,
    status: String,
}

// =====================
// 認証API
// =====================

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
}

#[derive(Serialize)]
struct LoginResponse {
    token: String,
    username: String,
    role: String,
}

async fn login_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    match auth::login(&payload.username, &payload.password, &state.auth_state).await {
        Ok(result) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "token": result.token,
                "username": result.username,
                "role": format!("{:?}", result.role).to_lowercase(),
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "status": "error",
                "message": e.to_string(),
            })),
        )
            .into_response(),
    }
}

async fn me_handler(
    axum::Extension(user): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "user_id": user.user_id,
            "username": user.username,
            "role": user.role,
        })),
    )
}

#[derive(Deserialize)]
struct ChangePasswordRequest {
    old_password: String,
    new_password: String,
}

async fn change_password_handler(
    axum::Extension(user): axum::Extension<AuthUser>,
    Json(payload): Json<ChangePasswordRequest>,
) -> impl IntoResponse {
    match auth::change_password(&user.user_id, &payload.old_password, &payload.new_password).await {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse {
                status: "ok".to_string(),
                message: "パスワードを変更しました。".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                status: "error".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// =====================
// ユーザー管理API (Admin)
// =====================

async fn list_users_handler(
    axum::Extension(user): axum::Extension<AuthUser>,
) -> impl IntoResponse {
    if user.role != "admin" {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"status": "error", "message": "Admin権限が必要です"})),
        )
            .into_response();
    }
    match auth::list_users() {
        Ok(users) => (StatusCode::OK, Json(users)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"status": "error", "message": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct CreateUserRequest {
    username: String,
    password: String,
    role: Option<String>,
}

async fn create_user_handler(
    axum::Extension(user): axum::Extension<AuthUser>,
    Json(payload): Json<CreateUserRequest>,
) -> impl IntoResponse {
    if user.role != "admin" {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"status": "error", "message": "Admin権限が必要です"})),
        )
            .into_response();
    }
    let role = if payload.role.as_deref() == Some("admin") {
        auth::Role::Admin
    } else {
        auth::Role::User
    };
    match auth::create_user(&payload.username, &payload.password, role).await {
        Ok(info) => (StatusCode::CREATED, Json(info)).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"status": "error", "message": e.to_string()})),
        )
            .into_response(),
    }
}

async fn delete_user_handler(
    axum::Extension(user): axum::Extension<AuthUser>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if user.role != "admin" {
        return (
            StatusCode::FORBIDDEN,
            Json(ApiResponse {
                status: "error".to_string(),
                message: "Admin権限が必要です".to_string(),
            }),
        )
            .into_response();
    }
    match auth::delete_user(&id) {
        Ok(_) => (
            StatusCode::OK,
            Json(ApiResponse {
                status: "ok".to_string(),
                message: "ユーザーを削除しました。".to_string(),
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse {
                status: "error".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response(),
    }
}

// =====================
// ダウンロードAPI
// =====================

#[derive(Deserialize)]
struct InfoRequest {
    url: String,
}

async fn video_info_handler(Json(payload): Json<InfoRequest>) -> impl IntoResponse {
    match downloader::get_video_info(&payload.url).await {
        Ok(info) => (StatusCode::OK, Json(info)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse {
                status: "error".to_string(),
                message: e.to_string(),
            }),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
struct DownloadRequest {
    url: String,
    quality: Option<String>,
    format: Option<String>,
}

#[derive(Serialize)]
struct DownloadResponse {
    task_id: String,
    message: String,
}

async fn download_handler(
    State(state): State<Arc<AppState>>,
    axum::Extension(user): axum::Extension<AuthUser>,
    Json(payload): Json<DownloadRequest>,
) -> impl IntoResponse {
    let task_id = Uuid::new_v4().to_string();
    let quality = payload.quality.unwrap_or_else(|| "best".to_string());
    let format = payload.format.unwrap_or_else(|| "mp4".to_string());
    let owner = user.username.clone();

    // タイトルを事前取得（非同期でバックグラウンド）
    let title_url = payload.url.clone();

    let task_status = TaskStatus {
        id: task_id.clone(),
        status: "pending".to_string(),
        progress: 0.0,
        message: "Starting...".to_string(),
        owner: owner.clone(),
        title: String::new(),
    };

    {
        let mut tasks = state.tasks.lock().unwrap();
        tasks.insert(task_id.clone(), task_status);
    }

    let state_clone = state.clone();
    let task_id_clone = task_id.clone();
    let url = payload.url.clone();

    tokio::spawn(async move {
        // ユーザーフォルダを作成してダウンロード先を決定
        let user_dir = match storage::ensure_user_dir(&owner) {
            Ok(d) => d,
            Err(e) => {
                let mut tasks = state_clone.tasks.lock().unwrap();
                if let Some(task) = tasks.get_mut(&task_id_clone) {
                    task.status = "error".to_string();
                    task.message = format!("ディレクトリ作成失敗: {}", e);
                }
                return;
            }
        };

        // タイトルを取得
        let title = downloader::get_title(&title_url).await;

        {
            let mut tasks = state_clone.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&task_id_clone) {
                task.status = "downloading".to_string();
                task.title = title.clone();
            }
        }

        let state_progress = state_clone.clone();
        let task_id_progress = task_id_clone.clone();

        let result = downloader::download_video(
            &url,
            &user_dir,
            &quality,
            &format,
            move |progress| {
                let mut tasks = state_progress.tasks.lock().unwrap();
                if let Some(task) = tasks.get_mut(&task_id_progress) {
                    task.progress = progress;
                }
            },
        )
        .await;

        {
            let mut tasks = state_clone.tasks.lock().unwrap();
            if let Some(task) = tasks.get_mut(&task_id_clone) {
                match result {
                    Ok(filename) => {
                        task.status = "completed".to_string();
                        task.progress = 100.0;
                        task.message = "Download completed".to_string();

                        // ファイルサイズを取得してメタデータを保存
                        let scan_dir = storage::user_dir(&task.owner);
                        let path = std::path::Path::new(&scan_dir).join(&filename);
                        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                        
                        storage::save_file_metadata(&filename, size, &task.owner, &task.title);
                    }
                    Err(e) => {
                        task.status = "error".to_string();
                        task.message = e.to_string();
                    }
                }
            }
        }
    });

    (
        StatusCode::OK,
        Json(DownloadResponse {
            task_id,
            message: "Download started".to_string(),
        }),
    )
}

async fn task_status_handler(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let tasks = state.tasks.lock().unwrap();
    if let Some(task) = tasks.get(&id) {
        (StatusCode::OK, Json(task.clone())).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(ApiResponse {
                status: "error".to_string(),
                message: "Task not found".to_string(),
            }),
        )
            .into_response()
    }
}

async fn delete_file_handler(
    axum::Extension(user): axum::Extension<AuthUser>,
    Path(filename): Path<String>,
) -> impl IntoResponse {
    // パストラバーサル防止
    if filename.contains('/') || filename.contains('\\') || filename.contains("..") {
        return (StatusCode::BAD_REQUEST, "Invalid filename").into_response();
    }

    // 自分のフォルダ内でのみ削除（Adminでも自分のフォルダのみ）
    // 注意: Adminが他ユーザーのファイルを削除する場合は別エンドポイントが必要だが、
    //       今回はユーザー自身のフォルダのみとする
    let owner = &user.username;
    let dir = storage::user_dir(owner);
    let path = std::path::Path::new(&dir).join(&filename);
    if path.exists() {
        match std::fs::remove_file(&path) {
            Ok(_) => {
                storage::remove_file_metadata(owner, &filename);
                (
                    StatusCode::OK,
                    Json(ApiResponse {
                        status: "success".to_string(),
                        message: "File deleted".to_string(),
                    }),
                )
                    .into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiResponse {
                    status: "error".to_string(),
                    message: e.to_string(),
                }),
            )
                .into_response(),
        }
    } else {
        (StatusCode::NOT_FOUND, "File not found").into_response()
    }
}

#[derive(Deserialize)]
struct ListVideosQuery {
    all: Option<String>, // Adminが全件取得するとき ?all=1
}

async fn list_videos_handler(
    axum::Extension(user): axum::Extension<AuthUser>,
    Query(query): Query<ListVideosQuery>,
) -> impl IntoResponse {
    let is_admin = user.role == "admin";
    let show_all = is_admin && query.all.as_deref() == Some("1");

    match storage::list_files(&user.username, is_admin, show_all) {
        Ok(files) => (StatusCode::OK, Json(files)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// =====================
// yt-dlp管理API
// =====================

async fn ytdlp_version_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status = state.ytdlp_status.lock().unwrap().clone();
    (StatusCode::OK, Json(status))
}

async fn ytdlp_update_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let status_clone = state.ytdlp_status.clone();
    {
        let s = status_clone.lock().unwrap();
        if s.updating {
            return (
                StatusCode::OK,
                Json(ApiResponse {
                    status: "skip".to_string(),
                    message: "すでにアップデート中です".to_string(),
                }),
            );
        }
    }
    tokio::spawn(async move {
        updater::update_ytdlp(status_clone).await.ok();
    });
    (
        StatusCode::OK,
        Json(ApiResponse {
            status: "ok".to_string(),
            message: "アップデートを開始しました".to_string(),
        }),
    )
}
