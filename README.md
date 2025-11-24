# Aly Video Downloader

A modern, powerful, and universal video downloader built with Rust and Vanilla JS.
Designed for speed, stability, and a premium user experience.

![UI Preview](https://via.placeholder.com/800x450.png?text=Aly+Video+Downloader+UI)

## ✨ Features

- **Universal Support**: Downloads videos and audio from YouTube, SoundCloud, Twitch, Twitter, and more (powered by `yt-dlp`).
- **Modern UI**: Sleek, dark-themed interface with a responsive design and smooth animations.
- **Format Selection**: Choose between Video (MP4) or Audio (MP3) with quality options (4K, 1080p, 720p, etc.).
- **Real-time Progress**: Live progress bars and status updates for all downloads.
- **Robust Architecture**:
  - **Rust Backend**: High-performance asynchronous server using Actix-web.
  - **Smart Queue**: Handles multiple downloads concurrently without freezing.
  - **Cookie Support**: Bypasses bot detection for YouTube using browser cookies.

## 🚀 Getting Started

### Prerequisites

- **Rust**: Latest stable version
- **yt-dlp**: Must be installed and in your PATH
- **FFmpeg**: Required for video merging and audio conversion
- **Node.js**: Required for `yt-dlp` JavaScript execution

### Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/Aly3424/aly-video-downloader.git
   cd aly-video-downloader
   ```

2. Build the project:
   ```bash
   cargo build --release
   ```

3. Run the server:
   ```bash
   ./target/release/youtube_downloader
   ```

4. Open your browser and visit:
   `http://localhost:3000`

## 🛠️ Configuration

- **Cookies**: Place your `youtube-cookies.txt` in the root directory to enable YouTube downloads.
- **Port**: Default runs on port 3000.

## 📦 Tech Stack

- **Backend**: Rust, Actix-web, Tokio
- **Frontend**: HTML5, CSS3 (Variables, Flexbox/Grid), Vanilla JavaScript
- **Core**: yt-dlp, FFmpeg

## 📝 License

This project is open source.
