#!/usr/bin/env python3
"""Dump an LLM session log into a human-readable .log for prompt analysis.

The agent runtime (sema-code-core) appends every request body and response to
~/.sema/llm_logs/<date>[_<session>].log as dense single-line JSON. That captures
the full system prompt, tool list, message history and responses — but it is
unreadable. This extractor expands one session into a clean, sectioned .log so
the system prompt and conversation flow can be analyzed and optimized.

Usage:
    python3 scripts/dump_llm_prompt.py                      # latest log in ~/.sema/llm_logs
    python3 scripts/dump_llm_prompt.py <path-to-session.log>
    python3 scripts/dump_llm_prompt.py <path> -o out.log
    SEMA_ROOT=~/.senclaw python3 scripts/dump_llm_prompt.py  # honor custom root
"""
import argparse
import glob
import json
import os
import re
import sys

LINE_RE = re.compile(r"^\[(\d{2}:\d{2}:\d{2})\](.+)$")


def sema_logs_dir() -> str:
    root = os.environ.get("SEMA_ROOT") or "~/.sema"
    return os.path.expanduser(os.path.join(root, "llm_logs"))


def latest_log() -> str:
    files = glob.glob(os.path.join(sema_logs_dir(), "*.log"))
    if not files:
        sys.exit(f"No .log files found in {sema_logs_dir()}")
    return max(files, key=os.path.getmtime)


def parse(path: str):
    """Yield (timestamp, data, is_request) for every parseable line."""
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.rstrip("\n")
            m = LINE_RE.match(line)
            if not m:
                continue
            try:
                data = json.loads(m.group(2))
            except json.JSONDecodeError:
                continue
            yield m.group(1), data, ("messages" in data)


def fmt_content(content) -> str:
    """Render a message content (string or list of blocks) as readable text."""
    if isinstance(content, str):
        return content
    parts = []
    for block in content or []:
        t = block.get("type")
        if t == "text":
            parts.append(block.get("text", ""))
        elif t == "thinking":
            parts.append(f"[thinking] {block.get('thinking', '')}")
        elif t == "tool_use":
            args = json.dumps(block.get("input", {}), ensure_ascii=False)
            parts.append(f"[tool_use {block.get('name')}] {args}")
        elif t == "tool_result":
            inner = block.get("content")
            parts.append(f"[tool_result] {fmt_content(inner)}")
        else:
            parts.append(json.dumps(block, ensure_ascii=False))
    return "\n".join(parts)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("log", nargs="?", help="session log path (default: latest)")
    ap.add_argument("-o", "--out", help="output .log path (default: <log>.readable.log)")
    args = ap.parse_args()

    src = args.log or latest_log()
    if not os.path.isfile(src):
        sys.exit(f"Not a file: {src}")
    out = args.out or os.path.splitext(src)[0] + ".readable.log"

    entries = list(parse(src))
    requests = [e for e in entries if e[2]]
    if not requests:
        sys.exit(f"No request entries (with 'messages') found in {src}")

    # The longest request carries the fullest message history; system + tools
    # are stable across the session, so take them from the first request.
    first = requests[0][1]
    longest = max(requests, key=lambda e: len(e[1].get("messages", [])))[1]

    lines = []
    w = lines.append
    w("=" * 80)
    w(f"SOURCE   : {src}")
    w(f"MODEL    : {first.get('model')}")
    w(f"REQUESTS : {len(requests)}   RESPONSES: {len(entries) - len(requests)}")
    th = first.get("thinking")
    if th is not None:
        w(f"THINKING : {json.dumps(th, ensure_ascii=False)}")
    w("=" * 80)

    # ---- System prompt -----------------------------------------------------
    w("\n" + "#" * 80)
    w("# SYSTEM PROMPT")
    w("#" * 80)
    system = first.get("system")
    if isinstance(system, list):
        for i, block in enumerate(system):
            w(f"\n----- system block {i} ({len(block.get('text',''))} chars) -----")
            w(block.get("text", ""))
    elif isinstance(system, str):
        w(system)

    # ---- Tools -------------------------------------------------------------
    tools = first.get("tools") or []
    w("\n" + "#" * 80)
    w(f"# TOOLS ({len(tools)})")
    w("#" * 80)
    for t in tools:
        desc = (t.get("description") or "").strip().splitlines()
        head = desc[0] if desc else ""
        w(f"- {t.get('name')}: {head}")

    # ---- Conversation ------------------------------------------------------
    w("\n" + "#" * 80)
    w("# CONVERSATION (from longest request + interleaved responses)")
    w("#" * 80)
    for i, msg in enumerate(longest.get("messages", [])):
        role = msg.get("role", "?")
        w(f"\n========== [{i}] {role.upper()} ==========")
        w(fmt_content(msg.get("content")))

    # ---- Raw response stream (to spot loops) -------------------------------
    w("\n" + "#" * 80)
    w("# RESPONSE STREAM (chronological — reveals loops/repetition)")
    w("#" * 80)
    for ts, data, is_req in entries:
        if is_req:
            w(f"\n[{ts}] >>> REQUEST  ({len(data.get('messages', []))} msgs)")
            continue
        calls = data.get("toolCalls") or []
        call_str = ", ".join(
            f"{c.get('name')}({json.dumps(c.get('args', {}), ensure_ascii=False)})" for c in calls
        )
        snippet = (data.get("content") or "").strip().replace("\n", " ")[:200]
        think = (data.get("thinking") or "").strip().replace("\n", " ")[:200]
        w(f"[{ts}] <<< RESPONSE")
        if think:
            w(f"        think: {think}")
        if snippet:
            w(f"        text : {snippet}")
        if call_str:
            w(f"        calls: {call_str}")

    with open(out, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\n")

    print(f"Wrote {out}  ({len(lines)} lines)")
    print(f"System prompt: {sum(len(b.get('text','')) for b in (system or [])) if isinstance(system, list) else len(system or '')} chars across "
          f"{len(system) if isinstance(system, list) else 1} block(s)")
    print(f"Tools: {len(tools)}  |  Longest request: {len(longest.get('messages', []))} messages")


if __name__ == "__main__":
    main()
