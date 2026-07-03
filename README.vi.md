<p align="center">
  <img src="docs/images/senclaw-logo.png" alt="SenClaw logo" width="160" />
</p>

<h1 align="center">SenClaw</h1>

<p align="center">
  <em>Một framework đa năng để xây dựng AI agent cá nhân.</em>
</p>

<p align="center">
  <a href="https://github.com/NortonBen/SenClaw/releases/latest"><img src="https://img.shields.io/github/v/tag/NortonBen/SenClaw?label=version" alt="Latest version" /></a>
  <a href="https://github.com/NortonBen/SenClaw/actions/workflows/desktop.yml"><img src="https://github.com/NortonBen/SenClaw/actions/workflows/desktop.yml/badge.svg" alt="Build status" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT" /></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <strong>Tiếng Việt</strong>
</p>

SenClaw cung cấp lớp hạ tầng chạy quanh mô hình ngôn ngữ lớn: phân quyền, bộ nhớ, tác vụ định kỳ, điều phối nhiều agent, kết nối nhiều kênh chat, Space Apps, mô hình chạy cục bộ và ứng dụng desktop. Mục tiêu là biến một model provider hoặc local model thành một hệ thống AI cá nhân có thể dùng hằng ngày.

---

## Về dự án

SenClaw là một **trạm làm việc AI cá nhân, local-first**: một daemon Rust duy nhất chạy toàn bộ agent, và app desktop Flutter native giám sát nó. Dữ liệu của bạn — hội thoại, ghi chú, lịch, bộ nhớ, wiki — nằm trong SQLite tại `~/.senclaw/` trên máy của bạn; model có thể là cloud provider **hoặc chạy hoàn toàn offline** trên Apple Silicon qua MLX.

Cụ thể gồm:

- **Một trợ lý, mọi nơi** — trò chuyện với cùng một bộ agent từ app desktop, Telegram / Feishu / QQ, app di động (qua relay), hoặc extension trình duyệt.
- **Agent làm việc thật** — phân quyền tool với phê duyệt human-in-the-loop, chế độ Plan, và team nhiều agent theo DAG (Cowork) cho các tác vụ lớn.
- **Bộ nhớ tích lũy** — knowledge graph nhận thức cộng với các file `memory/*.md` được tự động chắt lọc khi nén ngữ cảnh, và gợi nhớ lại trong các lượt sau.
- **Không gian cá nhân (Space)** — ghi chú, lịch với nhắc hẹn đẩy thông báo hệ thống, và schedule định kỳ chạy agent theo cron.
- **Space Apps** — mini-app full-stack cài thêm được (SSH Manager, DeepWiki, Email, …) mang theo UI, MCP tools và skills riêng.
- **Local models** — inference MLX native cho LLM (Gemma, Qwen, DeepSeek, …), Whisper speech-to-text, TTS, OCR và embeddings — không cần GPU cloud.

### Ảnh màn hình

| Dashboard | Chat |
| --- | --- |
| ![Dashboard](docs/images/screenshots/senclaw-dashboard.png) | ![Chat](docs/images/screenshots/senclaw-chat.png) |

| Plugins (Skills / MCP / Subagents) | Space (Notes · Calendar · Schedules) |
| --- | --- |
| ![Plugins](docs/images/screenshots/senclaw-plugins.png) | ![Space](docs/images/screenshots/senclaw-space.png) |

---

## Điểm nổi bật

- **Runtime cho agent cá nhân**: quản lý vòng đời agent, quyền dùng tool, luồng hỏi lại người dùng, trạng thái workspace và persona riêng cho từng agent.
- **Bộ nhớ và tri thức**: bộ nhớ lai FTS/vector, curated auto-memory, nhật ký hằng ngày và wiki cá nhân quản lý bằng Git.
- **Điều phối nhiều agent**: chạy team theo DAG, virtual workers, dispatch bridge và hỗ trợ subagent.
- **Tác vụ định kỳ**: hỗ trợ chế độ thông báo, script, agent và script kết hợp agent.
- **Gateway đa kênh**: Telegram, Feishu/Lark, QQ, WeChat, WebSocket, HTTP API và Web UI.
- **Space Apps**: các micro-app tách biệt như SSH Manager, DeepWiki, Email, Google Workspace và Test Manager, cung cấp tool qua MCP.
- **Tùy chọn AI cục bộ**: inference bằng MLX/Candle, embedding cục bộ, OCR, Whisper speech-to-text và TTS local.
- **Ứng dụng desktop**: app Flutter native (macOS/Windows/Linux/web), giám sát daemon như tiến trình con.

---

## Chạy nhanh

### 1. Tải mã nguồn

```bash
git clone https://github.com/NortonBen/SenClaw.git
cd SenClaw
```

### 2. Build giao diện Web

```bash
cd web
npm install
npm run build
cd ..
```

### 3. Build và chạy daemon

```bash
cargo run
```

Để build bản tối ưu:

```bash
cargo build --release
./target/release/senclaw
```

Sau đó mở Web UI:

```bash
open http://127.0.0.1:18788
```

Trên Linux, nếu không có lệnh `open`, hãy mở URL trên bằng trình duyệt.

---

## Cấu hình

SenClaw có thể chạy ở chế độ chỉ Web UI, nhưng các phiên agent cần ít nhất một cấu hình LLM. Sau khi chạy lần đầu, mở:

```text
Settings -> LLM
```

Thêm một provider như OpenAI, Anthropic, DeepSeek, Qwen, OpenRouter, Ollama hoặc endpoint tương thích. Cấu hình được lưu tại:

```text
~/.senclaw/config.json
```

Các thiết lập kênh chat và runtime có thể cấu hình qua `.env`:

```bash
cp .env.example .env
```

Một số giá trị thường dùng:

```env
TELEGRAM_BOT_TOKEN=
ADMIN_TELEGRAM_USER_ID=
FEISHU_APP_ID=
FEISHU_APP_SECRET=
QQ_APP_ID=
QQ_APP_SECRET=
GATEWAY_UI_PORT=18788
GATEWAY_PORT=18789
MAX_CONCURRENT_AGENTS=5
MAX_MESSAGES_PER_GROUP=100
SCHEDULER_INTERVAL_SEC=60
```

Nếu không cấu hình token cho các kênh chat, SenClaw vẫn có thể dùng qua Web UI.

---

## Lệnh thường dùng

```bash
# Kiểm tra code Rust
cargo check

# Chạy test Rust
cargo test

# Chạy daemon
cargo run

# Chạy với các feature local model theo Makefile
make run

# Chạy bản tối ưu cho local model
make run-release

# Chạy Web UI ở chế độ dev
make run-web

# Build browser extension
make build-extension
```

---

## Ứng dụng desktop

SenClaw có app desktop **Flutter** native trong `desktop_app/` (macOS / Windows / Linux / web). Nó thay thế Tauri shell cũ: kết nối trực tiếp tới daemon qua HTTP/WebSocket và **giám sát daemon `senclaw` như tiến trình con** (spawn binary đi kèm, stream log, restart khi cần).

Khi mở app có **cổng khởi động (startup gate)**: nếu daemon đã chạy sẵn thì vào thẳng giao diện; nếu chưa, app spawn daemon đi kèm, hiện màn hình "Starting daemon" cho tới khi HTTP API phản hồi rồi mới chuyển vào màn hình chính (kèm màn hình lỗi có nút Retry nếu daemon không khởi động được).

```bash
# Development (chạy app Flutter; tự adopt daemon đang chạy hoặc spawn mới)
make app-dev

# Bundle production (build daemon với đầy đủ feature Apple Silicon
# — MLX LLM, Whisper ASR, TTS, OCR Metal, embeddings — và nhúng binary
# vào Contents/Resources để supervisor khởi chạy)
make app-build          # macOS
make app-build-windows  # Windows
make app-build-linux    # Linux
make app-build-web      # web

# Cài bản .app vừa build vào /Applications và mở (macOS)
make app-install
```

---

## Cấu trúc dữ liệu runtime

Mặc định SenClaw lưu dữ liệu runtime trong home directory:

```text
~/.senclaw/
├── senclaw.db
├── config.json
├── dispatch-state.json
└── workspace-state-{folder}.json

~/senclaw/
├── agents/{folder}/
│   ├── SOUL.md
│   ├── memory/
│   └── .sema/sessions/
├── workspace/{folder}/
└── wiki/
```

Hầu hết đường dẫn có thể ghi đè qua `.env` hoặc `~/.senclaw/config.json`.

---

## Cấu trúc dự án

```text
SenClaw/
├── src/                    # Daemon Rust và core runtime
│   ├── agent/              # Vòng đời agent, phân quyền, personas, dispatch
│   ├── channels/           # Adapter Telegram, Feishu/Lark, QQ, WeChat
│   ├── gateway/            # HTTP, WebSocket, routing, UI server
│   ├── mcp/                # Các MCP server cung cấp cho agent
│   ├── memory/             # Bộ nhớ FTS/vector, curated memory, nhật ký
│   ├── scheduler/          # Task cron, interval và một lần
│   ├── local_model/        # Local model qua MLX/Candle
│   ├── code_graph/         # Lập chỉ mục code bằng Tree-sitter
│   ├── code_engine/        # Runtime agent chuyên code
│   ├── plugins/            # Hỗ trợ plugin
│   └── wiki/               # Knowledge base quản lý bằng Git
├── web/                    # Web UI React + Vite (legacy; daemon phục vụ)
├── desktop_app/            # App desktop Flutter (macOS/Windows/Linux/web)
├── apps/                   # Space Apps
├── app-space-sdk/          # SDK xây dựng Space Apps
├── examples/               # Ví dụ app và cách dùng SDK
├── skills/                 # Skills đi kèm
├── docs/                   # Tài liệu kiến trúc và tính năng
└── senclaw-extension-chrome/ # Extension Chrome
```

---

## Tài liệu

| Tài liệu | Nội dung |
|---|---|
| [Quick Start](docs/QUICK_START.md) | Cài đặt, cấu trúc runtime và ghi chú sử dụng. |
| [Architecture](docs/ARCHITECTURE.md) | Các lớp hệ thống, luồng khởi động và luồng dữ liệu. |
| [Memory](docs/memory.md) | Thiết kế bộ nhớ và luồng truy xuất. |
| [DAG Team](docs/DAG_Team.md) | Phân rã và thực thi tác vụ nhiều agent. |
| [Space Apps](docs/workspace-feature-design.md) | Cách thiết kế và đăng ký Space Apps. |
| [Code Knowledge Graph](docs/code-knowledge-graph.md) | Lập chỉ mục code bằng Tree-sitter. |
| [Prompt Injection Security](docs/prompt-injection-security.md) | Ghi chú bảo mật cho ranh giới tool và prompt. |

---

## Ghi chú phát triển

SenClaw chủ yếu là một Rust workspace. Web UI là ứng dụng React/Vite trong `web/`, còn một số Space Apps có manifest riêng trong `apps/`.

Một số build feature hữu ích:

```bash
# Embedding cục bộ
cargo build --features local-embed

# Embedding cục bộ với Metal trên Apple Silicon
cargo build --features local-embed-metal

# Runtime Candle cục bộ
cargo build --features local-candle

# Runtime MLX cục bộ
cargo build --features local-mlx
```

Một số feature cho local model, OCR, audio và TTS cần dependency riêng theo nền tảng. Nên bắt đầu bằng `cargo check` hoặc `cargo run` mặc định trước khi bật các feature nặng.

---

## Đóng góp

Bạn có thể đóng góp bằng issue, pull request, thử nghiệm hoặc thảo luận thiết kế. Hãy giữ thay đổi gọn theo mục tiêu, ghi lại hành vi có ảnh hưởng tới người dùng và thêm test cho các thay đổi runtime có rủi ro.

---

## Giấy phép

[MIT](LICENSE) © AIRC Sema Team

---

## Ghi nhận

SenClaw là bản viết lại bằng Rust, phát triển từ — và lấy cảm hứng sâu sắc từ — [**SemaClaw**](https://github.com/midea-ai/SemaClaw) (midea-ai), dự án gốc bằng TypeScript về gateway AI agent đa kênh. SenClaw chạy trên agent runtime [sema-code-core](https://github.com/midea-ai/sema-code-core).

SenClaw cũng tích hợp với marketplace plugin [ClaWHub](https://github.com/openclaw/clawhub), lấy cảm hứng từ [OpenClaw](https://github.com/openclaw/openclaw), [Model Context Protocol](https://modelcontextprotocol.io) và hệ sinh thái công cụ agent mã nguồn mở.
