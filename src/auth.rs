use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use bcrypt::{hash, verify};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

const BCRYPT_COST: u32 = 8; // 速度とセキュリティのバランス（DEFAULT_COST=12は重すぎる）

const USERS_FILE: &str = "users.json";
const JWT_SECRET: &str = "aly_video_downloader_jwt_secret_change_in_prod";
const JWT_EXPIRY_HOURS: i64 = 24;
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION: Duration = Duration::from_secs(600); // 10分

// =====================
// データ構造
// =====================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: String,
    pub username: String,
    pub password_hash: String,
    pub role: Role,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,       // user id
    pub username: String,
    pub role: String,
    pub exp: usize,        // expiry timestamp
}

// =====================
// ブルートフォース防止
// =====================

#[derive(Debug, Clone)]
struct LoginAttempt {
    count: u32,
    locked_at: Option<Instant>,
}

pub struct AuthState {
    attempts: Mutex<HashMap<String, LoginAttempt>>,
}

impl AuthState {
    pub fn new() -> Arc<Self> {
        Arc::new(AuthState {
            attempts: Mutex::new(HashMap::new()),
        })
    }

    /// ログイン失敗を記録。ロック中ならtrueを返す
    pub fn record_failure(&self, username: &str) -> bool {
        let mut map = self.attempts.lock().unwrap();
        let entry = map.entry(username.to_string()).or_insert(LoginAttempt {
            count: 0,
            locked_at: None,
        });

        // ロック中か確認
        if let Some(locked_at) = entry.locked_at {
            if locked_at.elapsed() < LOCKOUT_DURATION {
                return true; // まだロック中
            } else {
                // ロック解除
                entry.count = 0;
                entry.locked_at = None;
            }
        }

        entry.count += 1;
        if entry.count >= MAX_LOGIN_ATTEMPTS {
            entry.locked_at = Some(Instant::now());
            return true;
        }
        false
    }

    /// ログイン成功時にリセット
    pub fn reset(&self, username: &str) {
        let mut map = self.attempts.lock().unwrap();
        map.remove(username);
    }

    /// 現在ロック中かチェック（失敗カウントを変更しない）
    pub fn is_locked(&self, username: &str) -> bool {
        let map = self.attempts.lock().unwrap();
        if let Some(entry) = map.get(username) {
            if let Some(locked_at) = entry.locked_at {
                return locked_at.elapsed() < LOCKOUT_DURATION;
            }
        }
        false
    }
}

// =====================
// ユーザー永続化
// =====================

fn load_users() -> Result<Vec<User>> {
    if !std::path::Path::new(USERS_FILE).exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(USERS_FILE)?;
    Ok(serde_json::from_str(&content).unwrap_or_default())
}

fn save_users(users: &[User]) -> Result<()> {
    let content = serde_json::to_string_pretty(users)?;
    fs::write(USERS_FILE, content)?;
    Ok(())
}

/// 初回起動時にadminユーザーを作成する（同期版: main起動時に呼ぶ）
pub fn ensure_admin_exists() -> Result<()> {
    let mut users = load_users()?;
    let admin_exists = users.iter().any(|u| u.username == "admin");
    if !admin_exists {
        let password_hash = hash("admin", BCRYPT_COST).context("Failed to hash password")?;
        let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
        users.push(User {
            id: Uuid::new_v4().to_string(),
            username: "admin".to_string(),
            password_hash,
            role: Role::Admin,
            created_at: now,
        });
        save_users(&users)?;
        println!("[Auth] 初期adminユーザーを作成しました (admin/admin)。ログイン後にパスワードを変更してください。");
    }
    Ok(())
}

// =====================
// 認証操作
// =====================

pub struct LoginResult {
    pub token: String,
    pub username: String,
    pub role: Role,
}

pub async fn login(username: &str, password: &str, auth_state: &AuthState) -> Result<LoginResult> {
    // ロック確認
    if auth_state.is_locked(username) {
        anyhow::bail!("アカウントがロックされています。10分後に再試行してください。");
    }

    let users = load_users()?;
    let user = users.into_iter().find(|u| u.username == username);

    let user = match user {
        Some(u) => u,
        None => {
            auth_state.record_failure(username);
            anyhow::bail!("ユーザー名またはパスワードが正しくありません。");
        }
    };

    let hash_clone = user.password_hash.clone();
    let pass_clone = password.to_string();
    // bcrypt::verify はCPU集約なので spawn_blocking で実行
    let valid = tokio::task::spawn_blocking(move || {
        verify(&pass_clone, &hash_clone)
    })
    .await
    .context("spawn_blocking failed")?
    .context("パスワード検証に失敗しました")?;

    if !valid {
        let locked = auth_state.record_failure(&user.username);
        if locked {
            anyhow::bail!("ログイン試行回数が上限を超えました。10分間ロックします。");
        }
        anyhow::bail!("ユーザー名またはパスワードが正しくありません。");
    }

    // 成功 → リセット
    auth_state.reset(&user.username);

    let token = generate_token(&user.id, &user.username, &user.role)?;
    Ok(LoginResult {
        token,
        username: user.username.clone(),
        role: user.role.clone(),
    })
}

fn generate_token(user_id: &str, username: &str, role: &Role) -> Result<String> {
    let exp = chrono::Utc::now()
        .checked_add_signed(chrono::Duration::hours(JWT_EXPIRY_HOURS))
        .unwrap()
        .timestamp() as usize;

    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        role: format!("{:?}", role).to_lowercase(),
        exp,
    };

    let token = encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(JWT_SECRET.as_bytes()),
    )
    .context("JWTの生成に失敗しました")?;

    Ok(token)
}

pub fn verify_token(token: &str) -> Result<Claims> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(JWT_SECRET.as_bytes()),
        &Validation::default(),
    )
    .context("JWTの検証に失敗しました")?;

    Ok(token_data.claims)
}

// =====================
// ユーザー管理 (Admin操作)
// =====================

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub username: String,
    pub role: Role,
    pub created_at: String,
}

pub fn list_users() -> Result<Vec<UserInfo>> {
    let users = load_users()?;
    Ok(users
        .into_iter()
        .map(|u| UserInfo {
            id: u.id,
            username: u.username,
            role: u.role,
            created_at: u.created_at,
        })
        .collect())
}

pub async fn create_user(username: &str, password: &str, role: Role) -> Result<UserInfo> {
    let mut users = load_users()?;

    // 重複チェック
    if users.iter().any(|u| u.username == username) {
        anyhow::bail!("ユーザー名 '{}' はすでに使用されています。", username);
    }

    // パスワード長チェック
    if password.len() < 8 {
        anyhow::bail!("パスワードは8文字以上にしてください。");
    }

    let pass_clone = password.to_string();
    let password_hash = tokio::task::spawn_blocking(move || hash(&pass_clone, BCRYPT_COST))
        .await
        .context("spawn_blocking failed")?
        .context("パスワードのハッシュ化に失敗")?;
    let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();
    let user = User {
        id: Uuid::new_v4().to_string(),
        username: username.to_string(),
        password_hash,
        role: role.clone(),
        created_at: now.clone(),
    };

    let info = UserInfo {
        id: user.id.clone(),
        username: user.username.clone(),
        role: user.role.clone(),
        created_at: user.created_at.clone(),
    };

    users.push(user);
    save_users(&users)?;

    Ok(info)
}

pub fn delete_user(user_id: &str) -> Result<()> {
    let mut users = load_users()?;
    let before = users.len();
    users.retain(|u| u.id != user_id);
    if users.len() == before {
        anyhow::bail!("ユーザーが見つかりません");
    }
    // 最後のAdminは削除不可
    let admin_count = users.iter().filter(|u| u.role == Role::Admin).count();
    if admin_count == 0 {
        // 削除した後にAdminが0になる場合はNG（rollback相当）
        anyhow::bail!("最後のAdminユーザーは削除できません。");
    }
    save_users(&users)?;
    Ok(())
}

pub async fn change_password(user_id: &str, old_password: &str, new_password: &str) -> Result<()> {
    let mut users = load_users()?;
    let user = users
        .iter_mut()
        .find(|u| u.id == user_id)
        .ok_or_else(|| anyhow::anyhow!("ユーザーが見つかりません"))?
        .clone();

    let old_pass = old_password.to_string();
    let hash_clone = user.password_hash.clone();
    let valid = tokio::task::spawn_blocking(move || verify(&old_pass, &hash_clone))
        .await
        .context("spawn_blocking failed")?
        .context("パスワード検証に失敗")?;
    if !valid {
        anyhow::bail!("現在のパスワードが正しくありません。");
    }

    if new_password.len() < 8 {
        anyhow::bail!("新しいパスワードは8文字以上にしてください。");
    }

    let new_pass = new_password.to_string();
    let new_hash = tokio::task::spawn_blocking(move || hash(&new_pass, BCRYPT_COST))
        .await
        .context("spawn_blocking failed")?
        .context("ハッシュ化に失敗")?;

    // 再度users読み込んで更新
    let mut users = load_users()?;
    if let Some(u) = users.iter_mut().find(|u| u.id == user.id) {
        u.password_hash = new_hash;
    }
    save_users(&users)?;
    Ok(())
}
