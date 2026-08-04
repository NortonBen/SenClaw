---
name: sandbox-runner
description: >-
  Run the user's shell commands and source code isolated from the real machine,
  through the Sandbox Space App. Use it when they want to try a Python /
  JavaScript / Bash snippet, compute something with code, check whether a
  command works, run code they pasted from somewhere and do not yet trust,
  install a library and then run something, or when they say outright that they
  want it "in a sandbox / a VM / docker". Examples: "run this python for me",
  "work it out with code", "is this script safe? try it", "install pandas then
  run this file", "run this but don't touch my machine". Vietnamese users say
  the same things as "chạy thử đoạn python này", "chạy trong sandbox", "cài
  pandas rồi chạy", "chạy lệnh này nhưng đừng động vào máy tôi".
---

# Sandbox Runner

You drive the **Sandbox** app through the `sandbox-mcp` MCP server. Full tool
names look like `mcp__sandbox-mcp__sbx_*`.

## First decision: one shot or several steps

Most requests fall on the left.

| Situation | Use |
|---|---|
| Run one snippet, read the result, done | `sbx_run` — throwaway sandbox, auto-deleted |
| Several steps, keeping files or installed packages | `sbx_create`, then `sbx_run_in` / `sbx_exec` |

Do not `sbx_create` for a single calculation. Sandboxes made and never deleted
pile up.

## Normal sequence

1. **`sbx_run`** with `language` + `code`. That is usually the whole job — no
   setup call needed. If the machine cannot run it, the error says why and how
   to fix it.
2. If the error says a backend is unavailable → `sbx_capabilities` for the
   detail, then tell the user the one thing they need to do (usually: start
   Docker Desktop).
3. Multi-step work: `sbx_create` → `sbx_file_write` (feed data in) →
   `sbx_run_in` / `sbx_exec` → `sbx_delete` when finished.

## Tools

| Tool | For |
|---|---|
| `mcp__sandbox-mcp__sbx_capabilities` | What isolation this machine supports |
| `mcp__sandbox-mcp__sbx_run` | **Default** — run a snippet, then clean up |
| `mcp__sandbox-mcp__sbx_create` | Create a long-lived sandbox |
| `mcp__sandbox-mcp__sbx_list` | List existing sandboxes |
| `mcp__sandbox-mcp__sbx_exec` | Shell command in an existing sandbox |
| `mcp__sandbox-mcp__sbx_run_in` | Snippet in an existing sandbox |
| `mcp__sandbox-mcp__sbx_install` | Install pip / npm / apt packages |
| `mcp__sandbox-mcp__sbx_file_write` | Put data into the sandbox |
| `mcp__sandbox-mcp__sbx_file_read` | Read a result file |
| `mcp__sandbox-mcp__sbx_files` | List files |
| `mcp__sandbox-mcp__sbx_update` | Network on/off, CPU/RAM limits |
| `mcp__sandbox-mcp__sbx_delete` | Delete a sandbox |
| `mcp__sandbox-mcp__sbx_runs` | Run history |
| `mcp__sandbox-mcp__sbx_stats` | CPU/RAM in use + running processes |
| `mcp__sandbox-mcp__sbx_kill` | Stop one process, or all of them |
| `mcp__sandbox-mcp__sbx_mount` | Mount a real folder into the sandbox |
| `mcp__sandbox-mcp__sbx_unmount` | Unmount it again |
| `mcp__sandbox-mcp__sbx_fs_mode` | Change a sandbox's disk read isolation |
| `mcp__sandbox-mcp__sbx_settings` | Read/change the app defaults |
| `mcp__sandbox-mcp__sbx_trace` | Turn activity tracing on/off (testing) |
| `mcp__sandbox-mcp__sbx_events` | Read the file/process/network events |
| `mcp__sandbox-mcp__sbx_ports` | Open specific ports while the rest stays shut |

## Opening ports (running an app in a sandbox)

The network switch is all-or-nothing. `sbx_ports` is the middle ground: closed
except what you name.

- `listen: [8000]` — the sandbox may serve on 8000, and **you reach it at
  `http://127.0.0.1:8000`**. This is how you run someone's app in a sandbox and
  look at it in a browser.
- `connect: [443]` — the only remote port it may dial out to. HTTPS and nothing
  else.

Both lists **replace** the current ones; send them complete. Empty lists close
everything again. Listening ports must be 1024 or above.

You do not need `network: true` for this — the port rules are the whole
permission, which is the point: an app that serves on 8000 does not also get to
phone home.

**Enforcement differs by backend, and the reply tells you.** On macOS both
directions are exact. On docker and Linux, opening a listening port gives the
sandbox a network, so outbound is *not* limited to the `connect` list — the
`note` field in the reply says so, and you should pass that on rather than
implying the restriction held.

## Three levels of disk READ isolation

Writes are always confined to the sandbox directory. Reads have three levels,
set with `sbx_fs_mode` or at creation with `fsMode`:

| Level | The sandbox can read |
|---|---|
| `strict` (**default**) | Its own directory + mounted folders + system libraries |
| `allowlist` | As `strict`, plus folders declared in app settings |
| `open` | The whole disk, except `~/.ssh`, `~/.aws`, Keychain, SenClaw data |

**Do not reach for `open`.** The `strict` default means the code cannot read
the user's data, which is the point of the app. When a snippet needs one
particular folder, the right move is `sbx_mount` for that folder (read-only if
that is enough) — **not** opening the whole disk. Use `open` only when the user
explicitly asks for it.

The docker backend ignores this setting — a container is already fully isolated.

`sbx_settings` changes defaults for **new** sandboxes; existing ones keep
theirs. Note that its `allowlist` **replaces** the whole list rather than adding
to it — read the current list first, then send it back complete.

## Two backends

| | `direct` | `docker` |
|---|---|---|
| Needs | nothing | a running Docker daemon |
| Start-up | instant | seconds, plus a first-time image pull |
| Blocks writes outside the sandbox | yes | yes |
| Blocks reading the rest of the disk | yes, at `strict`/`allowlist` (default) | yes |
| Enforced RAM ceiling | only on Windows | yes |

**Leaving `backend` empty is right almost always** — the app picks what the
machine can actually do.

## Rules

- **The network is OFF by default.** Only turn it on (`network: true`) when the
  work needs it, and **say so when you do**. Untrusted code plus a network is
  how data leaves.
- **Installing packages needs the network.** `sbx_install` refuses while it is
  off; turn it on with `sbx_update` first and explain why.
- **Read `isolation` in the result.** It reports what actually confined that
  run: `seatbelt`, `bubblewrap`, `appcontainer`, `container` or `degraded`.
- **`degraded` means NO barrier at all.** The machine lacks an isolation tool.
  Tell the user before running anything else, not after.
- **Mount read-only by default**, and do not mount anything when the code is
  untrusted.
- **"File not found" when the file plainly exists** is usually `strict`
  blocking the read, not a wrong path. Mount that folder; do not switch to
  `open`.
- **`purge: true` deletes files irreversibly.** Only for sandboxes you created
  yourself, or when the user asks for a full delete.

## Reading a result

- `ok` — finished with exit code 0
- `exitCode` — `null` means killed (timed out), not success
- `timedOut` — over the deadline; **output printed earlier may be lost**, so do
  not conclude the code is wrong. Say it timed out and offer a larger
  `timeoutMs`
- `truncated` — output was cut; the tail may be missing
- `isolation` — what actually confined it
- `stdout` / `stderr`

Present it result first, isolation second; one line is enough. Do not paste the
user's own code back at them.

## Activity tracing, for testing

Off by default. Turn it on with `sbx_trace`, **run the code again**, then read
`sbx_events`.

It records file reads and writes, process launches with argv, network
connections with addresses, and hostname lookups. Filter with `kind` =
`file` | `proc` | `net`, or `runId` for a single run.

It works through in-process hooks (Python `sys.addaudithook`, Node `--require`)
plus a directory diff, so other languages still show their file writes. Hooks
are inherited by child processes.

**This is an observation tool for testing, NOT security evidence.** The hook
runs inside the sandbox and the log lives in the sandbox directory — code that
deliberately hides can evade it. Never tell the user "the log is clean, so this
code is safe." What actually stops hostile code is the sandbox itself, enforced
by the kernel.

Use it to answer "what did this code *actually* touch", and to point out
suspicious behaviour — an installer that reads `~/.aws`, or a script that calls
out to an unfamiliar domain.

## When Docker is not running

The most common situation. `sbx_capabilities` returns `docker.detail` with the
measured reason — usually "The Docker CLI is present but the daemon is not
answering".

Tell the user to start Docker Desktop, then call `sbx_capabilities` again with
`refresh: true`. **Do not** conclude the machine cannot sandbox — the `direct`
backend works on nearly every macOS and Linux machine.

## Language

The app's interface is English with a Vietnamese switch, and tool results are
English. Reply in whatever language the user is writing in.
