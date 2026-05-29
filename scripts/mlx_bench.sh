#!/usr/bin/env bash
# Native MLX decode benchmark harness (Apple Silicon, `local-mlx` feature).
#
# Builds the `mlx_bench` example, then runs it over a model × scenario matrix
# with generation params pinned via isolated settings.json dirs (so results are
# reproducible regardless of the user's ~/.senclaw config). Each cell reports
# min/median/mean decode tok/s and a determinism check.
#
# Usage:
#   scripts/mlx_bench.sh [iters]
#
# Env:
#   MODELS        space-separated "dirname:model_id" pairs under ~/.senclaw/local-models
#   ITERS         timed turns per throughput cell (default: arg1 or 5)
#   MEM_REQUESTS  if >0, also run a memory-growth pass of N distinct requests per
#                 model (tracks RSS + MLX pool; flat = bounded, climbing = leak)
#   PROMPT_SIZES  space-separated approx prompt-token sizes to sweep (e.g.
#                 "1000 2000") — reports cold/warm prefill tok/s + ttft + decode
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
LM="$HOME/.senclaw/local-models"
ITERS="${1:-${ITERS:-5}}"
MEM_REQUESTS="${MEM_REQUESTS:-0}"
PROMPT_SIZES="${PROMPT_SIZES:-}"

# dirname:model_id — edit / override via $MODELS. Skipped if the dir is absent.
MODELS="${MODELS:-Qwen__Qwen3-0.6B:Qwen/Qwen3-0.6B mlx-community__Qwen3-4B-4bit:Qwen/Qwen3-4B}"

echo "==> building mlx_bench (release, local-mlx)…"
BUILD_LOG="$(mktemp "${TMPDIR:-/tmp}/mlxbench-build.XXXXXX.log")"
if ! cargo build --release --manifest-path "$REPO/Cargo.toml" \
        --features local-mlx --example mlx_bench >"$BUILD_LOG" 2>&1; then
    echo "build failed:"; cat "$BUILD_LOG"; rm -f "$BUILD_LOG"; exit 1
fi
rm -f "$BUILD_LOG"
BIN="$REPO/target/release/examples/mlx_bench"

# Two scenarios: A = greedy / no penalty (async-lookahead fast path);
#                B = greedy + repetition_penalty 1.1 (synchronous path).
write_settings() { # $1=dir $2=penalty
    cat >"$1/settings.json" <<JSON
{
  "temperature": 0.0,
  "repetition_penalty": $2,
  "max_new_tokens": 400,
  "enable_thinking": false,
  "mlx_kv_cache_bits": 0,
  "max_kv_tokens": 8192,
  "idle_unload_secs": 0
}
JSON
}

BENCH_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/mlxbench.XXXXXX")"
trap 'rm -rf "$BENCH_ROOT"' EXIT

for pair in $MODELS; do
    dirname="${pair%%:*}"; model_id="${pair#*:}"
    real="$LM/$dirname"
    if [ ! -d "$real" ]; then
        echo "-- skip $dirname (not found at $real)"; continue
    fi
    for scen in A B; do
        [ "$scen" = A ] && pen=1.0 || pen=1.1
        cell="$BENCH_ROOT/$dirname-$scen"
        mkdir -p "$cell"
        ln -sfn "$real" "$cell/model"
        write_settings "$cell" "$pen"
        echo
        echo "######## $dirname  scenario $scen (penalty=$pen) ########"
        RUST_LOG="senclaw::local_model=info" \
            "$BIN" "$cell/model" "$model_id" "$ITERS" 2>&1 \
            | grep -E "decode tok/s|ttft|determinism|run [0-9]|warm_up"
    done

    if [ -n "$PROMPT_SIZES" ]; then
        cell="$BENCH_ROOT/$dirname-prompt"
        mkdir -p "$cell"
        ln -sfn "$real" "$cell/model"
        write_settings "$cell" 1.0
        for psz in $PROMPT_SIZES; do
            echo
            echo "######## $dirname  prompt sweep ~$psz tok ########"
            MLX_BENCH_PROMPT_TOKENS="$psz" RUST_LOG=error \
                "$BIN" "$cell/model" "$model_id" "$ITERS" 2>&1 \
                | grep -E "prompt [0-9]+ tok|cold prefill|warm prefill|decode tok/s|ttft|determinism"
        done
    fi

    if [ "$MEM_REQUESTS" -gt 0 ]; then
        echo
        echo "######## $dirname  memory pass ($MEM_REQUESTS distinct requests) ########"
        cell="$BENCH_ROOT/$dirname-mem"
        mkdir -p "$cell"
        ln -sfn "$real" "$cell/model"
        write_settings "$cell" 1.0
        MLX_BENCH_REQUESTS="$MEM_REQUESTS" RUST_LOG=error \
            "$BIN" "$cell/model" "$model_id" 2>&1 \
            | grep -vE "model_id|model_dir|^iters"
    fi
done
