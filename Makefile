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

# ===== Desktop app (Tauri 2.0) =====
# Requires: cargo install tauri-cli --version "^2"

app-icons:
	cargo tauri icon "senclaw_logo_simple_1_1777475846377.png"

app-dev:
	cd web && npx vite build
	cargo tauri dev

# Full installer build: web UI + CLI sidecar + bundle.
app-build:
	cd web && npx vite build
	cargo build --release --features local-mlx --features local-embed-metal --features local-embed --features local-mlx-whisper --bin senclaw
	mkdir -p src-tauri/binaries
	cp target/release/senclaw src-tauri/binaries/senclaw
	cargo tauri build
	@$(MAKE) app-clean-cache

# Install the freshly-built .app into /Applications and launch it.
app-install:
	@test -d target/release/bundle/macos/SemaClaw.app || (echo "no .app — run 'make app-build' first" && exit 1)
	@pkill -f "SemaClaw.app/Contents/MacOS/senclaw-app" 2>/dev/null || true
	@sleep 1
	rm -rf /Applications/SemaClaw.app
	cp -R target/release/bundle/macos/SemaClaw.app /Applications/
	open /Applications/SemaClaw.app

# Reclaim disk: dev-profile artefacts + incremental caches. Safe — release
# bundle in target/release/bundle/ and /Applications/ are untouched.
app-clean-cache:
	@echo "[clean] removing target/debug and incremental caches"
	@rm -rf target/debug target/release/incremental target/release/build/*-*/incremental 2>/dev/null || true
	@du -sh target 2>/dev/null || true