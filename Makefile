HUB_BACKEND_IMAGE ?= senclaw-hub-backend:latest
HUB_BACKEND_TAR ?= senclaw-hub-backend.tar.gz
HUB_BACKEND_COMPOSE ?= docker compose -f hub-backend/docker-compose.yml

.PHONY: hub-build hub-save hub-up hub-down hub-run

hub-build:
	docker build -t $(HUB_BACKEND_IMAGE) ./hub-backend

hub-save:
	docker save $(HUB_BACKEND_IMAGE) | gzip > $(HUB_BACKEND_TAR)

hub-up:
	$(HUB_BACKEND_COMPOSE) up -d --build

hub-down:
	$(HUB_BACKEND_COMPOSE) down

hub-run:
	$(HUB_BACKEND_COMPOSE) run --rm --service-ports hub-backend

run-backend:
	cargo run 

run-web:
	cd web && npm run dev

run:
	cargo run --features local-mlx --features local-embed-metal --features local-embed

# Release build of the daemon. Strongly preferred when using the native MLX
# local models: the gated-delta scan is host-dispatch-bound, so an optimized
# build is ~3.5-5x faster on prefill (and keeps the GPU fed) vs. `make run`.
run-release:
	cargo run --release --features local-mlx --features local-embed-metal --features local-embed --features local-mlx-whisper --features local-mlx-tts --features ocr-paddle-metal

build-extension:
	cd senclaw-extension-chrome && npm run build

# ===== Desktop app (Flutter — replaces the old Tauri shell) =====
# The app is a Flutter project in desktop_app/ that SUPERVISES the senclaw
# daemon as a child process (it spawns the bundled `senclaw` binary, streams
# its logs, and restarts it on demand). Multi-platform: macOS / Windows / Linux
# / web. Requires the Flutter SDK on PATH.
DESKTOP_DIR := desktop_app
# Full Apple-Silicon feature set — keep in sync with `run-release`.
DAEMON_FEATURES := local-mlx,local-embed-metal,local-embed,local-mlx-whisper,local-mlx-tts,ocr-paddle-metal

app-dev:
	cd $(DESKTOP_DIR) && flutter run -d macos

# Build the release .app and bundle the daemon binary into its Resources, so
# the supervisor finds it at Contents/Resources/senclaw.
app-build:
	MACOSX_DEPLOYMENT_TARGET=26.0 cargo build --release --features $(DAEMON_FEATURES) --bin senclaw
	cd $(DESKTOP_DIR) && flutter build macos --release
	cp target/release/senclaw \
	    "$(DESKTOP_DIR)/build/macos/Build/Products/Release/SenClaw Desktop.app/Contents/Resources/senclaw"
	@echo "[app-build] bundled daemon into 'SenClaw Desktop.app/Contents/Resources/senclaw'"

# Install the freshly-built .app into /Applications and launch it.
app-install:
	@test -d "$(DESKTOP_DIR)/build/macos/Build/Products/Release/SenClaw Desktop.app" \
	    || (echo "no .app — run 'make app-build' first" && exit 1)
	@pkill -f "SenClaw Desktop.app/Contents/MacOS/SenClaw Desktop" 2>/dev/null || true
	@sleep 1
	rm -rf "/Applications/SenClaw Desktop.app"
	cp -R "$(DESKTOP_DIR)/build/macos/Build/Products/Release/SenClaw Desktop.app" "/Applications/SenClaw Desktop.app"
	open "/Applications/SenClaw Desktop.app"

# Windows / Linux desktop builds (run on the matching host).
# Both bundle the release `senclaw` binary ALONGSIDE the app executable, which is
# the first path the supervisor's _resolveBinary() checks (exeDir/senclaw[.exe]),
# so the packaged app self-hosts the daemon with no SENCLAW_BIN needed.
# NOTE: the MLX/Metal features are Apple-only, so these omit DAEMON_FEATURES.
app-build-windows:
	cargo build --release --bin senclaw
	cd $(DESKTOP_DIR) && flutter build windows --release
	@dir=$$(ls -d $(DESKTOP_DIR)/build/windows/*/runner/Release 2>/dev/null | head -1); \
	    test -n "$$dir" || (echo "no flutter windows Release dir" && exit 1); \
	    cp target/release/senclaw.exe "$$dir/senclaw.exe"; \
	    echo "[app-build-windows] bundled daemon into $$dir/senclaw.exe"

app-build-linux:
	cargo build --release --bin senclaw
	cd $(DESKTOP_DIR) && flutter build linux --release
	@dir=$$(ls -d $(DESKTOP_DIR)/build/linux/*/release/bundle 2>/dev/null | head -1); \
	    test -n "$$dir" || (echo "no flutter linux bundle dir" && exit 1); \
	    cp target/release/senclaw "$$dir/senclaw"; \
	    echo "[app-build-linux] bundled daemon into $$dir/senclaw"

# Web build (served by the daemon's static dir, or any static host).
app-build-web:
	cd $(DESKTOP_DIR) && flutter build web --release

app-clean-cache:
	@echo "[clean] removing target/debug and incremental caches"
	@rm -rf target/debug target/release/incremental target/release/build/*-*/incremental 2>/dev/null || true
	@du -sh target 2>/dev/null || true