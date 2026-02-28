const API_BASE = '/api';

// =====================
// 認証ユーティリティ
// =====================

function getToken() {
    return localStorage.getItem('token');
}

function getRole() {
    return localStorage.getItem('role');
}

function getUsername() {
    return localStorage.getItem('username');
}

async function apiFetch(path, options = {}) {
    const token = getToken();
    const headers = {
        'Content-Type': 'application/json',
        ...(token ? { 'Authorization': `Bearer ${token}` } : {}),
        ...(options.headers || {}),
    };
    const res = await fetch(`${API_BASE}${path}`, { ...options, headers });
    // 401なら強制ログアウト
    if (res.status === 401) {
        localStorage.clear();
        window.location.href = '/login.html';
        return null;
    }
    return res;
}

// 未ログインならログイン画面へ
if (!getToken()) {
    window.location.href = '/login.html';
}

// =====================
// DOM要素
// =====================

const downloadBtn = document.getElementById('download-btn');
const urlInput = document.getElementById('video-url');
const qualitySelect = document.getElementById('quality-select');
const statusMsg = document.getElementById('status-message');
const videoList = document.getElementById('video-list');
const refreshBtn = document.getElementById('refresh-btn');
const formatSelect = document.getElementById('format-select');
const progressContainer = document.getElementById('progress-container');
const themeToggle = document.getElementById('theme-toggle');
const videoInfo = document.getElementById('video-info');
const infoTitle = document.getElementById('info-title');
const infoExtractor = document.getElementById('info-extractor');
const headerUsername = document.getElementById('header-username');
const adminBtn = document.getElementById('admin-btn');
const logoutBtn = document.getElementById('logout-btn');
const showAllLabel = document.getElementById('show-all-label');
const showAllCheckbox = document.getElementById('show-all-checkbox');

// =====================
// テーマ
// =====================

function initTheme() {
    const savedTheme = localStorage.getItem('theme') || 'light';
    document.documentElement.setAttribute('data-theme', savedTheme);
    updateThemeIcon(savedTheme);
}

themeToggle.addEventListener('click', () => {
    const current = document.documentElement.getAttribute('data-theme');
    const next = current === 'dark' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', next);
    localStorage.setItem('theme', next);
    updateThemeIcon(next);
});

function updateThemeIcon(theme) {
    const icon = themeToggle.querySelector('.toggle-icon');
    icon.textContent = theme === 'dark' ? '☀️' : '🌙';
}

initTheme();

// =====================
// ヘッダー初期化
// =====================

headerUsername.textContent = `👤 ${getUsername()}`;

if (getRole() === 'admin') {
    adminBtn.classList.remove('hidden');
    showAllLabel.classList.remove('hidden');
}

logoutBtn.addEventListener('click', () => {
    localStorage.clear();
    window.location.href = '/login.html';
});

// =====================
// yt-dlp バージョン管理
// =====================

const ytdlpVersionLabel = document.getElementById('ytdlp-version-label');
const ytdlpLastUpdated = document.getElementById('ytdlp-last-updated');
const ytdlpUpdateBtn = document.getElementById('ytdlp-update-btn');
const ytdlpUpdateMsg = document.getElementById('ytdlp-update-msg');

async function fetchYtdlpVersion() {
    try {
        const res = await apiFetch('/ytdlp/version');
        if (!res || !res.ok) return;
        const data = await res.json();
        ytdlpVersionLabel.textContent = `yt-dlp: v${data.version}`;
        if (data.last_updated !== '未実行') {
            ytdlpLastUpdated.textContent = `最終更新: ${data.last_updated}`;
        }
        if (data.updating) {
            ytdlpUpdateBtn.disabled = true;
            ytdlpUpdateBtn.textContent = '更新中...';
        } else {
            ytdlpUpdateBtn.disabled = false;
            ytdlpUpdateBtn.textContent = '今すぐ更新';
            if (data.last_update_result && data.last_update_result !== '自動アップデート待機中...') {
                ytdlpUpdateMsg.textContent = data.last_update_result;
            }
        }
    } catch (e) {
        ytdlpVersionLabel.textContent = 'yt-dlp: 取得失敗';
    }
}

ytdlpUpdateBtn.addEventListener('click', async () => {
    ytdlpUpdateBtn.disabled = true;
    ytdlpUpdateBtn.textContent = '更新中...';
    ytdlpUpdateMsg.textContent = 'アップデートを開始しています...';
    try {
        await apiFetch('/ytdlp/update', { method: 'POST' });
        const poll = setInterval(async () => {
            const res = await apiFetch('/ytdlp/version');
            if (!res || !res.ok) return;
            const data = await res.json();
            ytdlpUpdateMsg.textContent = data.last_update_result;
            if (!data.updating) {
                clearInterval(poll);
                ytdlpVersionLabel.textContent = `yt-dlp: v${data.version}`;
                ytdlpLastUpdated.textContent = `最終更新: ${data.last_updated}`;
                ytdlpUpdateBtn.disabled = false;
                ytdlpUpdateBtn.textContent = '今すぐ更新';
            }
        }, 2000);
    } catch {
        ytdlpUpdateMsg.textContent = 'エラーが発生しました';
        ytdlpUpdateBtn.disabled = false;
        ytdlpUpdateBtn.textContent = '今すぐ更新';
    }
});

fetchYtdlpVersion();
setInterval(fetchYtdlpVersion, 30000);

// =====================
// 動画情報取得
// =====================

let debounceTimer;

urlInput.addEventListener('input', () => {
    const url = urlInput.value.trim();
    if (!url) {
        videoInfo.classList.add('hidden');
        return;
    }
    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => fetchVideoInfo(url), 800);
});

async function fetchVideoInfo(url) {
    try {
        const res = await apiFetch('/info', {
            method: 'POST',
            body: JSON.stringify({ url }),
        });
        if (!res || !res.ok) return;
        const data = await res.json();
        infoTitle.textContent = data.title || '';
        infoExtractor.textContent = data.extractor || '';
        updateQualityOptions(data.extractor || '');
        videoInfo.classList.remove('hidden');
    } catch {
        videoInfo.classList.add('hidden');
    }
}

function updateQualityOptions(extractor) {
    const ext = extractor.toLowerCase();
    const isAudioOnly = ['soundcloud', 'bandcamp', 'audiomack'].some(s => ext.includes(s));

    if (isAudioOnly) {
        qualitySelect.innerHTML = '<option value="audio" selected>音声のみ</option>';
        formatSelect.innerHTML = '<option value="mp3">MP3</option><option value="m4a">M4A</option>';
    } else {
        qualitySelect.innerHTML = `
            <option value="1080p" selected>1080p (推奨)</option>
            <option value="best">最高画質</option>
            <option value="720p">720p</option>
            <option value="360p">360p</option>
            <option value="audio">音声のみ</option>
        `;
        formatSelect.innerHTML = `
            <option value="mp4">MP4</option>
            <option value="webm">WebM</option>
            <option value="mkv">MKV</option>
        `;
    }
}

// =====================
// ダウンロード
// =====================

downloadBtn.addEventListener('click', startDownload);

async function startDownload() {
    const url = urlInput.value.trim();
    if (!url) {
        statusMsg.textContent = 'URLを入力してください。';
        return;
    }
    const quality = qualitySelect.value;
    const format = formatSelect.value;

    downloadBtn.disabled = true;
    statusMsg.textContent = 'ダウンロードを開始しています...';

    try {
        const res = await apiFetch('/download', {
            method: 'POST',
            body: JSON.stringify({ url, quality, format }),
        });
        if (!res || !res.ok) {
            const d = await res.json();
            statusMsg.textContent = `エラー: ${d.message}`;
            downloadBtn.disabled = false;
            return;
        }
        const data = await res.json();
        statusMsg.textContent = '';
        trackProgress(data.task_id);
        downloadBtn.disabled = false;
    } catch (err) {
        statusMsg.textContent = `エラー: ${err.message}`;
        downloadBtn.disabled = false;
    }
}

function trackProgress(taskId) {
    progressContainer.classList.remove('hidden');
    const wrapper = document.createElement('div');
    wrapper.id = `task-${taskId}`;
    wrapper.className = 'progress-item';
    wrapper.innerHTML = `
        <div class="progress-label">ダウンロード中...</div>
        <div class="progress-bar-bg"><div class="progress-bar-fill" style="width:0%"></div></div>
        <div class="progress-pct">0%</div>
    `;
    progressContainer.appendChild(wrapper);

    const interval = setInterval(async () => {
        try {
            const res = await apiFetch(`/tasks/${taskId}`);
            if (!res || !res.ok) { clearInterval(interval); return; }
            const data = await res.json();

            const fill = wrapper.querySelector('.progress-bar-fill');
            const pct = wrapper.querySelector('.progress-pct');
            const label = wrapper.querySelector('.progress-label');

            fill.style.width = `${data.progress}%`;
            pct.textContent = `${Math.round(data.progress)}%`;

            if (data.title) label.textContent = data.title;

            if (data.status === 'completed') {
                clearInterval(interval);
                label.textContent = `✅ 完了: ${data.title || 'ダウンロード完了'}`;
                fill.style.width = '100%';
                pct.textContent = '100%';
                setTimeout(() => wrapper.remove(), 5000);
                fetchVideos();
            } else if (data.status === 'error') {
                clearInterval(interval);
                label.textContent = `❌ エラー: ${data.message}`;
                pct.textContent = '';
            }
        } catch {
            clearInterval(interval);
        }
    }, 1000);
}

// =====================
// ダウンロードリスト
// =====================

refreshBtn.addEventListener('click', fetchVideos);
showAllCheckbox && showAllCheckbox.addEventListener('change', fetchVideos);

async function fetchVideos() {
    try {
        const showAll = showAllCheckbox && showAllCheckbox.checked ? '?all=1' : '';
        const res = await apiFetch(`/videos${showAll}`);
        if (!res || !res.ok) return;
        const files = await res.json();
        renderList(files);
    } catch (err) {
        console.error('Failed to fetch videos', err);
    }
}

function renderList(files) {
    videoList.innerHTML = '';
    if (!files || files.length === 0) {
        videoList.innerHTML = '<li>ダウンロードされた動画はありません</li>';
        return;
    }

    const showAll = showAllCheckbox && showAllCheckbox.checked;

    files.forEach(file => {
        const sizeMB = (file.size / (1024 * 1024)).toFixed(2);
        const downloadUrl = `/downloads/${encodeURIComponent(file.owner)}/${encodeURIComponent(file.filename)}`;
        const displayTitle = file.title && file.title !== file.filename ? file.title : file.filename;
        const dateStr = file.downloaded_at ? file.downloaded_at : '';

        const li = document.createElement('li');
        li.innerHTML = `
            <div class="file-info">
                <span class="file-name"></span>
                <div class="file-meta">
                    ${showAll ? `<span class="file-owner">👤 ${file.owner}</span>` : ''}
                    <span class="file-date">${dateStr}</span>
                    <span class="file-size">${sizeMB} MB</span>
                </div>
            </div>
            <div class="file-actions"></div>
        `;

        li.querySelector('.file-name').textContent = displayTitle;

        // ダウンロードボタン
        const saveBtn = document.createElement('a');
        saveBtn.className = 'btn-icon';
        saveBtn.title = 'ダウンロード';
        saveBtn.href = downloadUrl;
        saveBtn.download = file.filename;
        saveBtn.innerHTML = `
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                <polyline points="7 10 12 15 17 10"></polyline>
                <line x1="12" y1="15" x2="12" y2="3"></line>
            </svg>
        `;

        // 削除ボタン
        const deleteBtn = document.createElement('button');
        deleteBtn.className = 'delete-btn';
        deleteBtn.title = '削除';
        deleteBtn.innerHTML = '🗑';
        deleteBtn.onclick = async () => {
            if (!confirm(`「${displayTitle}」を削除しますか？`)) return;
            const res = await apiFetch(`/files/${encodeURIComponent(file.filename)}`, { method: 'DELETE' });
            if (res && res.ok) fetchVideos();
        };

        const actions = li.querySelector('.file-actions');
        actions.appendChild(saveBtn);
        actions.appendChild(deleteBtn);

        videoList.appendChild(li);
    });
}

// 初期ロード
fetchVideos();

// =====================
// Admin: ユーザー管理モーダル
// =====================

const adminModal = document.getElementById('admin-modal');
const closeModal = document.getElementById('close-modal');
const createUserBtn = document.getElementById('create-user-btn');
const createUserMsg = document.getElementById('create-user-msg');
const userListEl = document.getElementById('user-list');

adminBtn && adminBtn.addEventListener('click', () => {
    adminModal.classList.remove('hidden');
    loadUserList();
});

closeModal && closeModal.addEventListener('click', () => {
    adminModal.classList.add('hidden');
});

createUserBtn && createUserBtn.addEventListener('click', async () => {
    const username = document.getElementById('new-username').value.trim();
    const password = document.getElementById('new-password').value;
    const role = document.getElementById('new-role').value;
    createUserMsg.textContent = '';

    if (!username || !password) {
        createUserMsg.textContent = 'ユーザー名とパスワードを入力してください。';
        return;
    }

    const res = await apiFetch('/admin/users', {
        method: 'POST',
        body: JSON.stringify({ username, password, role }),
    });
    if (!res) return;
    const data = await res.json();
    if (res.ok) {
        createUserMsg.textContent = `✅ ユーザー「${data.username}」を作成しました。`;
        createUserMsg.style.color = 'var(--text-color)';
        document.getElementById('new-username').value = '';
        document.getElementById('new-password').value = '';
        loadUserList();
    } else {
        createUserMsg.textContent = `❌ ${data.message}`;
        createUserMsg.style.color = '#e74c3c';
    }
});

async function loadUserList() {
    const res = await apiFetch('/admin/users');
    if (!res || !res.ok) return;
    const users = await res.json();
    userListEl.innerHTML = '';
    users.forEach(u => {
        const li = document.createElement('li');
        li.style.cssText = 'display:flex;align-items:center;gap:0.8rem;padding:0.5rem 0;border-bottom:1px solid var(--border-color)';
        li.innerHTML = `
            <span style="flex:1;font-weight:600">${u.username}</span>
            <span style="font-size:0.8rem;color:var(--text-secondary)">${u.role}</span>
            <span style="font-size:0.75rem;color:var(--text-secondary)">${u.created_at}</span>
        `;
        // 自分自身とadminユーザーは削除不可表示
        if (u.username !== getUsername()) {
            const delBtn = document.createElement('button');
            delBtn.textContent = '削除';
            delBtn.style.cssText = 'padding:0.2rem 0.6rem;border-radius:5px;border:none;background:#e74c3c;color:white;cursor:pointer;font-size:0.8rem';
            delBtn.onclick = async () => {
                if (!confirm(`「${u.username}」を削除しますか？`)) return;
                const res = await apiFetch(`/admin/users/${u.id}`, { method: 'DELETE' });
                if (res && res.ok) loadUserList();
            };
            li.appendChild(delBtn);
        }
        userListEl.appendChild(li);
    });
}

// =====================
// パスワード変更モーダル
// =====================

const changePassModal = document.getElementById('change-pass-modal');
const closePassModal = document.getElementById('close-pass-modal');
const doChangePass = document.getElementById('do-change-pass');
const changePassMsg = document.getElementById('change-pass-msg');

document.getElementById('change-pass-btn').addEventListener('click', () => {
    changePassModal.classList.remove('hidden');
    changePassMsg.textContent = '';
});

closePassModal.addEventListener('click', () => {
    changePassModal.classList.add('hidden');
});

doChangePass.addEventListener('click', async () => {
    const oldPassword = document.getElementById('old-password').value;
    const newPassword = document.getElementById('new-password-field').value;
    const confirmPassword = document.getElementById('confirm-password').value;
    changePassMsg.textContent = '';

    if (newPassword !== confirmPassword) {
        changePassMsg.textContent = '新しいパスワードが一致しません。';
        changePassMsg.style.color = '#e74c3c';
        return;
    }

    const res = await apiFetch('/auth/password', {
        method: 'POST',
        body: JSON.stringify({ old_password: oldPassword, new_password: newPassword }),
    });
    if (!res) return;
    const data = await res.json();
    if (res.ok) {
        changePassMsg.textContent = '✅ パスワードを変更しました。再ログインしてください。';
        changePassMsg.style.color = 'green';
        setTimeout(() => {
            localStorage.clear();
            window.location.href = '/login.html';
        }, 2000);
    } else {
        changePassMsg.textContent = `❌ ${data.message}`;
        changePassMsg.style.color = '#e74c3c';
    }
});
