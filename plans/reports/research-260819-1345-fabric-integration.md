# Fabric → SenClaw: tích hợp được gì, và tích hợp thế nào

Nguồn: https://github.com/danielmiessler/fabric (MIT). Khảo sát 19/08/2026.

## Trả lời ngắn

**Tích hợp được — nhưng chỉ đúng một phần của Fabric, và phần đó không phải là code.**

Toàn bộ tầng *engine* của Fabric (CLI Go, vendor abstraction 20+ provider,
`--serve` REST, `--serveOllama`, session/context, YouTube/scrape/transcribe)
SenClaw đã có bản mạnh hơn. Port sang là công không sinh lợi.

Thứ đáng lấy là **thư viện Patterns**: 200+ system-prompt viết sẵn, đã được
cộng đồng mài, giấy phép MIT — cộng với **hai ý tưởng thiết kế** SenClaw
đang thiếu: *strategies* (bọc kỹ thuật suy luận quanh prompt) và UX
*one-shot transform* (đưa văn bản vào → ra kết quả có cấu trúc, không vòng
tool nào).

## 1. Fabric thực chất là gì

| Thành phần | Nội dung |
|---|---|
| CLI (Go) | `fabric -p <pattern>`, đọc stdin, đẩy 1 lượt lên LLM |
| Patterns | `data/patterns/<name>/system.md` — markdown thuần, 200+ cái |
| Cấu trúc pattern | `# IDENTITY and PURPOSE` → `# STEPS` → `# OUTPUT INSTRUCTIONS` → `# INPUT` |
| Biến | `{{input}}` = stdin; `-v=#role:expert` chèn biến tuỳ ý |
| Strategies | `data/strategies/*.json` — cot, cod, tot, aot, ltm, self-refine, reflexion, self-consistent. Sửa system prompt **trước** khi gửi |
| Contexts / Sessions | thư mục ngữ cảnh + lịch sử hội thoại |
| Vendor | 20+ provider (OpenAI, Anthropic, Google, Ollama, Azure, Bedrock…) |
| Server | `--serve` REST; `--serveOllama` giả lập Ollama, **mỗi pattern là một "model"** (`summarize:latest`) |
| Helper | to_pdf, code2context, generate_changelog, YouTube transcript, scrape qua Jina |

Điểm cốt lõi: pattern là **văn bản tĩnh**, không gọi tool, không vòng lặp.
Một pattern = một phép biến đổi văn bản xác định.

## 2. Đối chiếu với SenClaw

### Đã có, mạnh hơn — KHÔNG port

| Fabric | SenClaw tương đương |
|---|---|
| vendor abstraction | `src/zen_core/` + `llm_provider` + Space App `llm` block (app cũng làm provider được) |
| `--serve` REST | daemon axum 18788, toàn bộ `/api/*` |
| sessions / contexts | group session, `memory/`, `PersonaRegistry`, cognitive graph |
| `-y` YouTube, `-u` scrape | `agent-browser`, `web-research`, `senclaw-browser` (30 tool) |
| `--transcribe-file` | sidecar `senclaw-media` (Whisper MLX) |
| pattern update từ GitHub | ClawHub (`search/publish/download_skill_zip` + lockfile), marketplace sources |
| to_pdf / code2context | `apps/drawio`, `apps/deepwiki`, `ak:repomix` |

### Chưa có — ĐÁNG lấy

| Thiếu ở SenClaw | Giá trị |
|---|---|
| **Thư viện 200+ prompt tác vụ** | `analyze_threat_report`, `extract_wisdom`, `analyze_paper`, `create_prd`, `analyze_logs`… — nội dung thuần, MIT, dùng ngay |
| **Strategies** | grep `src/` chỉ ra `adaptive_thinking` trong `query_llm.rs` — đó là *mức* suy nghĩ, không phải *kỹ thuật* prompt. Không có CoT/ToT/reflexion đóng gói tái dùng |
| **One-shot transform UX** | Skill của SenClaw là agentic (có tool, có vòng). Không có đường "dán text → ra bản tóm tắt theo khuôn" mà không tốn một vòng agent |

## 3. Bẫy lớn nhất: **pattern không được là skill**

Cách làm hiển nhiên — convert 200 pattern thành 200 `SKILL.md` — là cách
làm **sai**, và sai theo kiểu hỏng cả hệ thống:

- `src/skills/scan.rs` quét 4–5 nguồn rồi nạp *tất cả* vào `SkillRegistry`.
  Mỗi skill đóng góp `name` + `description` + `triggers` vào bộ đối sánh
  trước mỗi lượt. Thêm 200 mục làm loãng trigger matcher tới mức skill thật
  (`web-research`, `agent-browser`) mất quyền được chọn.
- 200 skill `user-invocable` = 200 slash command đổ vào cùng namespace với
  `/wiki`, `/schedule`, `/space`.
- Repo đã có tiền lệ đau: whitelist `groups.allowed_tools` làm rỗng roster
  (`docs/tool-skill-name-lookup.md`). Roster là tài nguyên khan hiếm.

⇒ Patterns phải là **primitive riêng**, có registry riêng, và chỉ chiếm
**+1 skill / +3 tool** trong roster.

## 4. Bốn phương án

| | Cách làm | Chi phí | Đánh giá |
|---|---|---|---|
| **A** | Convert 200 pattern → 200 skill | thấp | ❌ phá roster (mục 3) |
| **B** | **Primitive `patterns` trong core + MCP server `senclaw-patterns`** | trung bình | ✅ **khuyến nghị** |
| **C** | Đóng thành 1 Zen Kit | thấp | ⚠️ `KitSkill` = skill thật → quay lại bẫy A. Chỉ hợp nếu kit chỉ cài 1 skill + 1 thư mục dữ liệu |
| **D** | Space App `apps/fabric` (port riêng, `fabric-mcp`, UI gallery) | cao | ⚠️ đúng triết lý "app = thứ gỡ được", nhưng pattern nhẹ tới mức nuôi hẳn một tiến trình là thừa; và pattern cần dùng được từ *mọi* lượt chat, không chỉ khi app chạy |

## 5. Thiết kế phương án B

### 5.1 Lưu trữ

```
~/.senclaw/patterns/
  <name>/system.md          # đồng bộ từ repo fabric (upstream)
  strategies/<name>.json
~/.senclaw/patterns-custom/
  <name>/system.md          # người dùng tự viết — ĐÈ lên upstream cùng tên
```

Đúng quy ước Fabric (custom không bị `--updatepatterns` ghi đè) và đúng quy
ước SenClaw (mọi thứ dưới `~/.senclaw/`).

**Không nhúng vào binary.** `assets/templates` hiện 184K, `skills/` 252K;
200 pattern ≈ 600K–1M sẽ gấp ba phần asset. Fetch-on-demand + cache, kèm
một tập nhỏ (~15 cái hay dùng) nhúng sẵn cho trường hợp offline.

### 5.2 Module

`src/patterns/` — song song `src/kits/`, `src/skills/`:

| File | Việc |
|---|---|
| `registry.rs` | quét 2 thư mục, custom đè upstream, cache tên + mô tả (dòng đầu `# IDENTITY`) |
| `render.rs` | thay `{{input}}`, biến `-v` kiểu `#key:value`, giữ nguyên placeholder lạ (như `scaffold`) |
| `strategy.rs` | nạp `*.json`, ghép vào system prompt |
| `sync.rs` | tải/cập nhật từ GitHub — dùng lại đường HTTP của `clawhub/client.rs` |

Chạy thật qua `agent::isolated_runner::run_one_shot` (đã có,
`src/agent/isolated_runner.rs:177`) — không vòng tool, không tốn phiên agent.

### 5.3 MCP server

Theo đúng công ước `CLAUDE.md`: server `senclaw-patterns`, prefix `pattern_`.

```
mcp__senclaw-patterns__pattern_list      # tên + mô tả, lọc theo từ khoá
mcp__senclaw-patterns__pattern_run       # {name, input, strategy?, vars?, model?}
mcp__senclaw-patterns__pattern_sync      # cập nhật từ upstream
```

Ba tool — không phải 200. `from_env() -> Result<Option<Self>>` + `vis = "pub"`
trên `#[rmcp::tool_router]` để `senclaw core-server` gộp được vào một tiến
trình (mục "Built-in MCP servers run in ONE process").

### 5.4 Skill cầu nối (+1 roster)

`skills/pattern/SKILL.md` — dạy agent: khi người dùng nói "tóm tắt bài này
theo kiểu…", "trích insight", "phân tích log này" thì `pattern_list` để tìm
pattern hợp rồi `pattern_run`. Trigger đặt hẹp, tránh giẫm `web-research`.

### 5.5 Slash command

Namespace riêng để không đụng skill: `/pattern <name>` và `/p:<name>`.
Xử lý ở `src/gateway/command_dispatcher.rs`, cùng chỗ `/skill` `#skill`.
Input = phần còn lại của tin nhắn, hoặc file đính kèm (đã có đường
`documents.rs` trích text + lưu path).

### 5.6 Web UI + Desktop

Trang `Plugins → Patterns`: danh sách + tìm kiếm, xem system.md, nút "Chạy"
với ô input, chọn strategy, nút Sync. REST `/api/patterns/*`. Đây là phần
Fabric làm tốt mà SenClaw chưa có mặt tương đương.

### 5.7 Strategies

Bê nguyên 8 file JSON của Fabric (MIT) vào `~/.senclaw/patterns/strategies/`.
Giá trị vượt ra ngoài patterns: cùng cơ chế có thể áp cho persona và
sub-agent sau này. Đừng trộn với `adaptive_thinking` trong `query_llm.rs` —
một cái chỉnh *ngân sách* suy nghĩ, một cái chỉnh *phương pháp*.

## 6. Khối lượng & rủi ro

| Hạng mục | Ước lượng |
|---|---|
| `src/patterns/` (4 file + test) | ~600–800 LoC |
| MCP server 3 tool | ~250 LoC |
| REST `/api/patterns/*` | ~200 LoC |
| Web UI page | ~300 LoC TSX |
| Desktop (Flutter) | ~250 LoC Dart |
| Skill + docs | ~150 dòng md |

Rủi ro:

1. **Giấy phép** — MIT, vendoring hợp lệ, nhưng phải giữ `LICENSE` + ghi
   nguồn trong thư mục pattern và trong `docs/`.
2. **Chất lượng pattern không đều.** 200 cái là số lượng cộng đồng, không
   phải số lượng đã kiểm định. Nên có allowlist mặc định (~30 cái) và bật
   phần còn lại theo yêu cầu, thay vì đổ hết vào `pattern_list`.
3. **Pattern viết cho tiếng Anh.** `OUTPUT INSTRUCTIONS` thường ép output
   tiếng Anh — người dùng SenClaw chủ yếu tiếng Việt. Cần lớp phủ ngôn ngữ
   (thêm chỉ thị "trả lời bằng ngôn ngữ của input") ở `render.rs`, không sửa
   file upstream (sẽ mất khi sync).
4. **Prompt injection.** Pattern là văn bản tải từ mạng và được đặt vào vị
   trí *system prompt*. Sync phải qua đường tin cậy (tag/commit ghim, không
   phải `main` trôi nổi), và pattern tuỳ chỉnh của app/marketplace phải bị
   đối xử như dữ liệu không tin cậy giống hook lệnh trong kits.

## 7. Việc KHÔNG nên làm

- Không port CLI Go.
- Không làm `--serveOllama` (SenClaw đã là host model qua Space App `llm` block).
- Không thêm session/context của Fabric — trùng hoàn toàn với group session.
- Không convert pattern thành skill (mục 3).

## Câu hỏi mở

1. Patterns nên là **core** (đề xuất B) hay **Space App** (D)? B khiến pattern
   dùng được ở mọi lượt chat; D giữ daemon gọn và gỡ được. Cần quyết định trước
   khi viết dòng code nào.
2. Đồng bộ từ upstream fabric trực tiếp, hay mirror qua ClawHub để kiểm duyệt
   được (và cho phép người Việt publish pattern tiếng Việt)?
3. Tập mặc định bao nhiêu pattern — tất cả 200+, hay allowlist ~30?
