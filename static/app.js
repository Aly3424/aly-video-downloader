const API_BASE = '/api';

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

// Theme Toggle Logic
function initTheme() {
    const savedTheme = localStorage.getItem('theme') || 'light';
    document.documentElement.setAttribute('data-theme', savedTheme);
    updateThemeIcon(savedTheme);
}

themeToggle.addEventListener('click', () => {
    const currentTheme = document.documentElement.getAttribute('data-theme');
    const newTheme = currentTheme === 'dark' ? 'light' : 'dark';
    document.documentElement.setAttribute('data-theme', newTheme);
    localStorage.setItem('theme', newTheme);
    updateThemeIcon(newTheme);
});

function updateThemeIcon(theme) {
    const icon = themeToggle.querySelector('.toggle-icon');
    icon.textContent = theme === 'dark' ? '☀️' : '🌙';
}

initTheme();
const infoExtractor = document.getElementById('info-extractor');

let debounceTimer;

urlInput.addEventListener('input', () => {
    const url = urlInput.value.trim();
    if (!url) {
        videoInfo.classList.add('hidden');
        return;
    }

    clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => fetchVideoInfo(url), 500);
});

async function fetchVideoInfo(url) {
    try {
        const res = await fetch(`${API_BASE}/info`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ url }),
        });

        if (res.ok) {
            const info = await res.json();
            infoTitle.textContent = info.title;
            infoExtractor.textContent = info.extractor;
            videoInfo.classList.remove('hidden');
            updateQualityOptions(info.extractor);
        } else {
            videoInfo.classList.add('hidden');
        }
    } catch (err) {
        console.error('Info fetch error', err);
        videoInfo.classList.add('hidden');
    }
}

function updateQualityOptions(extractor) {
    const lowerExtractor = extractor.toLowerCase();
    // 音声特化サービスのリスト
    const audioServices = ['soundcloud', 'mixcloud', 'bandcamp', 'audiomack', 'spotify'];

    const isAudioService = audioServices.some(service => lowerExtractor.includes(service));
    const options = qualitySelect.options;

    // フォーマット選択肢の更新
    updateFormatOptions(isAudioService || qualitySelect.value === 'audio');

    for (let i = 0; i < options.length; i++) {
        const opt = options[i];
        if (isAudioService) {
            if (opt.value === 'audio') {
                opt.style.display = '';
                qualitySelect.value = 'audio';
            } else {
                opt.style.display = 'none';
            }
        } else {
            opt.style.display = '';
            // 音声のみが選択されていた場合、デフォルトに戻す（ただしユーザーが意図的に選んだ場合は維持したいが、
            // サービス切り替え時はリセットした方が安全）
            if (qualitySelect.value === 'audio' && i === 0) {
                qualitySelect.value = '1080p';
            }
        }
    }
    // 画質変更時にもフォーマットを更新
    updateFormatOptions(qualitySelect.value === 'audio');
}

qualitySelect.addEventListener('change', () => {
    updateFormatOptions(qualitySelect.value === 'audio');
});

function updateFormatOptions(isAudio) {
    formatSelect.innerHTML = '';
    if (isAudio) {
        addOption(formatSelect, 'mp3', 'MP3');
        addOption(formatSelect, 'm4a', 'M4A');
        addOption(formatSelect, 'wav', 'WAV');
    } else {
        addOption(formatSelect, 'mp4', 'MP4');
        addOption(formatSelect, 'webm', 'WebM');
        addOption(formatSelect, 'mkv', 'MKV');
    }
}

function addOption(select, value, text) {
    const opt = document.createElement('option');
    opt.value = value;
    opt.textContent = text;
    select.appendChild(opt);
}

downloadBtn.addEventListener('click', async () => {
    const url = urlInput.value.trim();
    const quality = qualitySelect.value;
    const format = formatSelect.value;

    if (!url) {
        showStatus('URLを入力してください', 'error');
        return;
    }

    // ボタンは無効化せず、並列実行を許可する
    // setLoading(true);
    showStatus('ダウンロードを開始しました', 'normal');

    try {
        const res = await fetch(`${API_BASE}/download`, {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({ url, quality, format }),
        });

        const data = await res.json();

        if (res.ok) {
            urlInput.value = '';
            videoInfo.classList.add('hidden');
            createTaskElement(data.task_id, url);
            pollTaskStatus(data.task_id);
        } else {
            showStatus(`エラー: ${data.message}`, 'error');
        }
    } catch (err) {
        showStatus(`通信エラー: ${err.message}`, 'error');
    }
});

function createTaskElement(taskId, url) {
    progressContainer.classList.remove('hidden');
    const div = document.createElement('div');
    div.id = `task-${taskId}`;
    div.className = 'task-item';
    div.innerHTML = `
        <div class="task-info">Downloading: ${url}</div>
        <div class="progress-bar-bg">
            <div class="progress-bar-fill" style="width: 0%"></div>
        </div>
        <div class="task-status">Preparing...</div>
    `;
    progressContainer.appendChild(div);
}

async function pollTaskStatus(taskId) {
    const taskElem = document.getElementById(`task-${taskId}`);
    const fill = taskElem.querySelector('.progress-bar-fill');
    const statusText = taskElem.querySelector('.task-status');

    const interval = setInterval(async () => {
        try {
            const res = await fetch(`${API_BASE}/tasks/${taskId}`);
            if (res.ok) {
                const task = await res.json();
                fill.style.width = `${task.progress}%`;
                statusText.textContent = `${task.status} (${task.progress.toFixed(1)}%)`;

                if (task.status === 'completed' || task.status === 'error') {
                    clearInterval(interval);
                    if (task.status === 'completed') {
                        statusText.textContent = '完了';
                        setTimeout(() => taskElem.remove(), 3000); // 3秒後に消す
                        fetchVideos(); // リスト更新
                    } else {
                        statusText.textContent = `エラー: ${task.message}`;
                        statusText.style.color = 'red';
                    }
                }
            } else {
                clearInterval(interval);
            }
        } catch (e) {
            clearInterval(interval);
        }
    }, 1000);
}

refreshBtn.addEventListener('click', fetchVideos);

async function fetchVideos() {
    try {
        const res = await fetch(`${API_BASE}/videos`);
        if (res.ok) {
            const files = await res.json();
            renderList(files);
        }
    } catch (err) {
        console.error('Failed to fetch videos', err);
    }
}

function renderList(files) {
    videoList.innerHTML = '';
    if (files.length === 0) {
        videoList.innerHTML = '<li>ダウンロードされた動画はありません</li>';
        return;
    }

    files.forEach(file => {
        const li = document.createElement('li');
        const sizeMB = (file.size / (1024 * 1024)).toFixed(2);
        const downloadUrl = `/downloads/${encodeURIComponent(file.filename)}`;

        // 基本構造を作成
        li.innerHTML = `
            <div class="file-info">
                <span class="file-name"></span>
                <span class="file-size">${sizeMB} MB</span>
            </div>
            <div class="file-actions">
            </div>
        `;

        // テキストコンテンツを安全に設定
        li.querySelector('.file-name').textContent = file.filename;

        // 保存ボタンを動的に生成して追加
        const saveBtn = document.createElement('a'); // Changed to <a> for download attribute
        saveBtn.className = 'btn-icon';
        saveBtn.title = 'ダウンロード';
        saveBtn.href = downloadUrl; // Set href for download
        saveBtn.download = file.filename; // Add download attribute
        saveBtn.innerHTML = `
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                <polyline points="7 10 12 15 17 10"></polyline>
                <line x1="12" y1="15" x2="12" y2="3"></line>
            </svg>
        `;
        // No onclick needed for <a> with download attribute

        li.querySelector('.file-actions').appendChild(saveBtn);

        // 削除ボタンを動的に生成して追加
        const deleteBtn = document.createElement('button');
        deleteBtn.className = 'delete-btn';
        deleteBtn.title = '削除';
        deleteBtn.textContent = '×';
        deleteBtn.onclick = () => deleteFile(file.filename);

        li.querySelector('.file-actions').appendChild(deleteBtn);

        videoList.appendChild(li);
    });
}

window.deleteFile = async function (filename) {
    if (!confirm(`本当に「${filename}」を削除しますか？`)) return;

    try {
        const res = await fetch(`${API_BASE}/files/${encodeURIComponent(filename)}`, {
            method: 'DELETE'
        });
        if (res.ok) {
            fetchVideos();
        } else {
            alert('削除に失敗しました');
        }
    } catch (e) {
        alert('通信エラー');
    }
};

function showStatus(msg, type) {
    statusMsg.textContent = msg;
    statusMsg.className = type;
    if (type === 'normal') statusMsg.style.color = '#333';
}

function setLoading(isLoading) {
    downloadBtn.disabled = isLoading;
    urlInput.disabled = isLoading;
    if (isLoading) {
        downloadBtn.textContent = '処理中...';
    } else {
        downloadBtn.textContent = 'ダウンロード';
    }
}

// 初期ロード時にリスト取得
fetchVideos();
