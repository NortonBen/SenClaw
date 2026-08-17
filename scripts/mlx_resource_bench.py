#!/usr/bin/env python3
"""Resource benchmark for the native MLX path: throughput + CPU + GPU + RAM.

`scripts/mlx_bench.sh` answers "how fast"; this answers "at what cost". It runs
`examples/mlx_bench` under a sampler that polls, twice a second:

  * process RSS and CPU% (`ps`), plus cumulative CPU time so a phase average can
    be computed from deltas rather than from a decaying estimate;
  * GPU busy-percent and GPU in-use system memory, read from the Apple
    accelerator's IOKit performance counters (`ioreg -c AGXAccelerator`), which
    unlike `powermetrics` need no root.

It also scrapes the MLX allocator figures the bench already prints (active /
cache / peak), because RSS alone cannot separate "MLX is holding weights" from
"the process mapped a file".

Two things about the GPU numbers, both of which the report repeats so nobody
reads them as more than they are:

  * `Device Utilization %` is **system-wide and whole-device**, not per-process
    and not per-core. Anything else using the GPU lands in this number, so the
    harness measures an idle floor first and prints it beside the result.
  * It is a *busy* fraction, not a throughput or occupancy measure. A kernel
    that keeps the GPU busy while starved of parallel work reads the same as one
    that saturates it.

Interleave variants (`--rounds`) rather than running one after the other: on a
thermally-managed laptop, run order alone can manufacture a swing larger than
most candidates.

Usage:

    scripts/mlx_resource_bench.py \\
        --bin current=target/release/examples/mlx_bench \\
        --model-dir ~/.senclaw/local-models/mlx-community__gemma-4-e2b-it-4bit \\
        --model-id mlx-community/gemma-4-e2b-it-4bit \\
        --rounds 2 --out bench-out/

Pass `--bin label=path` more than once to A/B separately-built binaries.
"""

from __future__ import annotations

import argparse
import csv
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path

SAMPLE_INTERVAL_S = 0.5
IDLE_SAMPLE_S = 3.0

# Scenario = (name, prompt tokens, generated tokens). Short/medium/long, because
# a candidate that wins one length routinely loses another.
DEFAULT_SCENARIOS = [
    ("short", 500, 300),
    ("medium", 4000, 800),
    ("long", 12000, 1500),
]


# ---------------------------------------------------------------------------
# Sampling
# ---------------------------------------------------------------------------


def gpu_stats() -> tuple[float | None, float | None]:
    """(device busy %, GPU in-use system memory in MiB) — both system-wide."""
    try:
        out = subprocess.run(
            ["ioreg", "-r", "-d", "1", "-w", "0", "-c", "AGXAccelerator"],
            capture_output=True,
            text=True,
            timeout=5,
        ).stdout
    except Exception:
        return None, None
    util = re.search(r'"Device Utilization %"=(\d+)', out)
    mem = re.search(r'"In use system memory"=(\d+)', out)
    return (
        float(util.group(1)) if util else None,
        float(mem.group(1)) / (1024 * 1024) if mem else None,
    )


def proc_stats(pid: int) -> tuple[float | None, float | None, float | None]:
    """(RSS MiB, ps %cpu, cumulative CPU seconds) for one pid."""
    try:
        out = subprocess.run(
            ["ps", "-o", "rss=,%cpu=,time=", "-p", str(pid)],
            capture_output=True,
            text=True,
            timeout=5,
        ).stdout.strip()
    except Exception:
        return None, None, None
    if not out:
        return None, None, None
    parts = out.split()
    if len(parts) < 3:
        return None, None, None
    rss = float(parts[0]) / 1024.0
    pcpu = float(parts[1])
    if rss <= 0.0:
        # `ps` can still answer for a pid that is mid-exit, with every field
        # zeroed. Kept out of the series entirely: as a sample it drags the RSS
        # and CPU means down, and as the *last* sample it makes the cumulative
        # CPU-time delta negative, which is how a run once reported -0.0% CPU.
        return None, None, None
    # "MM:SS.ss" or "HH:MM:SS.ss"
    bits = parts[2].split(":")
    secs = 0.0
    for b in bits:
        secs = secs * 60 + float(b)
    return rss, pcpu, secs


@dataclass
class Samples:
    rows: list[dict] = field(default_factory=list)

    def summary(self) -> dict:
        def agg(key: str) -> dict:
            vals = [r[key] for r in self.rows if r.get(key) is not None]
            if not vals:
                return {"mean": None, "p95": None, "max": None}
            vals_sorted = sorted(vals)
            idx = min(len(vals_sorted) - 1, int(round(0.95 * (len(vals_sorted) - 1))))
            return {
                "mean": statistics.fmean(vals),
                "p95": vals_sorted[idx],
                "max": max(vals),
            }

        out = {k: agg(k) for k in ("rss_mib", "cpu_pct", "gpu_pct", "gpu_mem_mib")}
        # CPU% from cumulative CPU-time deltas — independent of the kernel's
        # decaying `%cpu` estimate, and the figure to trust for a phase average.
        cpu_times = [
            (r["t"], r["cpu_s"]) for r in self.rows if r.get("cpu_s") is not None
        ]
        # Cumulative CPU time can only rise; anything else is a bad read and
        # must not enter the delta.
        cpu_times = [
            pair
            for i, pair in enumerate(cpu_times)
            if i == 0 or pair[1] >= cpu_times[i - 1][1]
        ]
        if len(cpu_times) >= 2:
            dt = cpu_times[-1][0] - cpu_times[0][0]
            dcpu = cpu_times[-1][1] - cpu_times[0][1]
            out["cpu_pct_from_time"] = (100.0 * dcpu / dt) if dt > 0 else None
            out["cpu_seconds"] = dcpu
        else:
            out["cpu_pct_from_time"] = None
            out["cpu_seconds"] = None
        out["n_samples"] = len(self.rows)
        return out


def sample_until(stop: threading.Event, pid: int, samples: Samples) -> None:
    t0 = time.monotonic()
    while not stop.is_set():
        rss, pcpu, cpu_s = proc_stats(pid)
        gpu_pct, gpu_mem = gpu_stats()
        if rss is None and gpu_pct is None:
            break
        samples.rows.append(
            {
                "t": time.monotonic() - t0,
                "rss_mib": rss,
                "cpu_pct": pcpu,
                "cpu_s": cpu_s,
                "gpu_pct": gpu_pct,
                "gpu_mem_mib": gpu_mem,
            }
        )
        stop.wait(SAMPLE_INTERVAL_S)


def idle_floor() -> dict:
    """GPU busy-percent with nothing of ours running — the number every
    measured GPU figure has to be read against, since the counter is
    system-wide."""
    s = Samples()
    t0 = time.monotonic()
    while time.monotonic() - t0 < IDLE_SAMPLE_S:
        gpu_pct, gpu_mem = gpu_stats()
        s.rows.append(
            {"t": time.monotonic() - t0, "gpu_pct": gpu_pct, "gpu_mem_mib": gpu_mem}
        )
        time.sleep(SAMPLE_INTERVAL_S)
    return s.summary()


# ---------------------------------------------------------------------------
# Bench driving
# ---------------------------------------------------------------------------

THROUGHPUT_RE = {
    "decode_median": re.compile(r"decode tok/s\s+min=[\d.]+\s+median=([\d.]+)"),
    "decode_min": re.compile(r"decode tok/s\s+min=([\d.]+)"),
    "ttft_median": re.compile(r"ttft \(s\)\s+min=[\d.]+\s+median=([\d.]+)"),
    "cold_prefill": re.compile(r"cold prefill:\s+([\d.]+) tok/s"),
    "warm_prefill": re.compile(r"warm prefill: median=([\d.]+) tok/s"),
    # Emitted by the bench's own summary line:
    #   memory  rss=… MiB | mlx active=… cache=… peak=… MiB
    "mlx_active": re.compile(r"mlx active=([\d.]+)"),
    "mlx_cache": re.compile(r"cache=([\d.]+)"),
    "mlx_peak": re.compile(r"peak=([\d.]+) MiB"),
}


def scrape(text: str) -> dict:
    out = {}
    for key, rx in THROUGHPUT_RE.items():
        hits = rx.findall(text)
        out[key] = float(hits[-1]) if hits else None
    return out


def write_settings(cell: Path, max_new: int) -> None:
    # Greedy + no penalty: the point is to measure the runtime, not the
    # sampler's stochasticity, and greedy keeps runs comparable across variants.
    (cell / "settings.json").write_text(
        json.dumps(
            {
                "temperature": 0.0,
                "repetition_penalty": 1.0,
                "max_new_tokens": max_new,
                "enable_thinking": False,
                "mlx_kv_cache_bits": 0,
                "max_kv_tokens": 32000,
                "idle_unload_secs": 0,
            },
            indent=2,
        )
    )


def run_one(
    binary: Path, cell: Path, model_id: str, iters: int, prompt_tokens: int
) -> tuple[dict, Samples, str]:
    env = dict(os.environ, MLX_BENCH_PROMPT_TOKENS=str(prompt_tokens))
    proc = subprocess.Popen(
        [str(binary), str(cell / "model"), model_id, str(iters)],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env=env,
    )
    samples = Samples()
    stop = threading.Event()
    t = threading.Thread(target=sample_until, args=(stop, proc.pid, samples))
    t.start()
    out = proc.communicate()[0] or ""
    stop.set()
    t.join()
    return scrape(out), samples, out


# ---------------------------------------------------------------------------
# Report
# ---------------------------------------------------------------------------


def fmt(v, spec="{:.1f}") -> str:
    return "—" if v is None else spec.format(v)


def render(results: list[dict], idle: dict, meta: dict) -> str:
    lines: list[str] = []
    lines.append("# MLX native path — resource benchmark\n")
    lines.append(f"- Host: {meta['host']} ({meta['cpu_cores']} CPU cores)")
    lines.append(f"- Model: `{meta['model_id']}`")
    lines.append(f"- Sampling: every {SAMPLE_INTERVAL_S}s, greedy decode, {meta['iters']} timed turns/run")
    lines.append(
        f"- GPU idle floor before the run: {fmt(idle['gpu_pct']['mean'])}% busy, "
        f"{fmt(idle['gpu_mem_mib']['mean'], '{:.0f}')} MiB GPU-resident\n"
    )
    lines.append(
        "GPU busy-percent is **whole-device and system-wide** — it is not "
        "per-process and not an occupancy measure. Read it against the idle "
        "floor above.\n"
    )

    header = (
        "| variant | scenario | prompt→gen | decode tok/s | TTFT s | "
        "CPU % (mean) | CPU s | GPU % (mean/peak) | RSS peak MiB | "
        "MLX active/peak MiB | GPU mem peak MiB |"
    )
    lines.append(header)
    lines.append("|" + "---|" * 11)
    for r in results:
        s = r["summary"]
        lines.append(
            "| {v} | {sc} | {pt}→{gt} | {dec} | {ttft} | {cpu} | {cpus} | {gpu}/{gpup} | "
            "{rss} | {ma}/{mp} | {gm} |".format(
                v=r["variant"],
                sc=r["scenario"],
                pt=r["prompt_tokens"],
                gt=r["max_new"],
                dec=fmt(r["throughput"]["decode_median"]),
                ttft=fmt(r["throughput"]["ttft_median"], "{:.2f}"),
                cpu=fmt(s["cpu_pct_from_time"]),
                cpus=fmt(s["cpu_seconds"], "{:.0f}"),
                gpu=fmt(s["gpu_pct"]["mean"], "{:.0f}"),
                gpup=fmt(s["gpu_pct"]["max"], "{:.0f}"),
                rss=fmt(s["rss_mib"]["max"], "{:.0f}"),
                ma=fmt(r["throughput"]["mlx_active"], "{:.0f}"),
                mp=fmt(r["throughput"]["mlx_peak"], "{:.0f}"),
                gm=fmt(s["gpu_mem_mib"]["max"], "{:.0f}"),
            )
        )
    lines.append("")
    lines.append(
        "`CPU %` is derived from cumulative CPU-time deltas, so 100% = one core "
        f"fully busy and the ceiling is {meta['cpu_cores'] * 100}%. `CPU s` is "
        "total CPU seconds for the whole run, which is the figure to compare "
        "when wall time differs between variants."
    )
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--bin",
        action="append",
        required=True,
        help="label=path (repeatable) — variants are interleaved, not batched",
    )
    ap.add_argument("--model-dir", required=True)
    ap.add_argument("--model-id", required=True)
    ap.add_argument("--iters", type=int, default=3)
    ap.add_argument("--rounds", type=int, default=1)
    ap.add_argument("--out", default="bench-out")
    ap.add_argument(
        "--scenario",
        action="append",
        help="name:prompt_tokens:max_new (repeatable); default short/medium/long",
    )
    args = ap.parse_args()

    variants = []
    for spec in args.bin:
        label, _, path = spec.partition("=")
        p = Path(path).expanduser().resolve()
        if not p.exists():
            print(f"error: no such binary: {p}", file=sys.stderr)
            return 1
        variants.append((label, p))

    scenarios = DEFAULT_SCENARIOS
    if args.scenario:
        scenarios = []
        for s in args.scenario:
            name, pt, gt = s.split(":")
            scenarios.append((name, int(pt), int(gt)))

    model_dir = Path(args.model_dir).expanduser().resolve()
    if not model_dir.is_dir():
        print(f"error: no such model dir: {model_dir}", file=sys.stderr)
        return 1

    out_dir = Path(args.out).expanduser().resolve()
    out_dir.mkdir(parents=True, exist_ok=True)

    print(f"==> GPU idle floor ({IDLE_SAMPLE_S}s)…")
    idle = idle_floor()

    results: list[dict] = []
    tmp = Path(tempfile.mkdtemp(prefix="mlxres."))
    try:
        for rnd in range(args.rounds):
            for name, prompt_tokens, max_new in scenarios:
                # Interleave variants *within* a scenario so thermal drift and
                # page-cache state cannot be assigned to one of them.
                for label, binary in variants:
                    cell = tmp / f"{label}-{name}"
                    cell.mkdir(parents=True, exist_ok=True)
                    link = cell / "model"
                    if not link.exists():
                        link.symlink_to(model_dir)
                    write_settings(cell, max_new)
                    print(
                        f"==> round {rnd + 1}/{args.rounds}  {label}  {name} "
                        f"({prompt_tokens}→{max_new})…",
                        flush=True,
                    )
                    tp, samples, raw = run_one(
                        binary, cell, args.model_id, args.iters, prompt_tokens
                    )
                    (out_dir / f"raw-{label}-{name}-r{rnd + 1}.log").write_text(raw)
                    csv_path = out_dir / f"samples-{label}-{name}-r{rnd + 1}.csv"
                    with csv_path.open("w", newline="") as fh:
                        w = csv.DictWriter(
                            fh,
                            fieldnames=[
                                "t",
                                "rss_mib",
                                "cpu_pct",
                                "cpu_s",
                                "gpu_pct",
                                "gpu_mem_mib",
                            ],
                        )
                        w.writeheader()
                        w.writerows(samples.rows)
                    results.append(
                        {
                            "variant": label,
                            "scenario": name,
                            "round": rnd + 1,
                            "prompt_tokens": prompt_tokens,
                            "max_new": max_new,
                            "throughput": tp,
                            "summary": samples.summary(),
                        }
                    )
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

    meta = {
        "host": subprocess.run(
            ["sysctl", "-n", "hw.model"], capture_output=True, text=True
        ).stdout.strip(),
        "cpu_cores": int(
            subprocess.run(
                ["sysctl", "-n", "hw.ncpu"], capture_output=True, text=True
            ).stdout.strip()
            or 0
        ),
        "model_id": args.model_id,
        "iters": args.iters,
    }
    report = render(results, idle, meta)
    (out_dir / "report.md").write_text(report)
    (out_dir / "results.json").write_text(
        json.dumps({"meta": meta, "idle": idle, "results": results}, indent=2)
    )
    print("\n" + report)
    print(f"\nWrote {out_dir}/report.md, results.json, per-run CSVs.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
