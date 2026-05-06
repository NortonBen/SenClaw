---
name: code
description: Agent-driven code editing — read, write, search, and run shell commands inside a sandboxed workspace. Supports single-agent tasks and DAG multi-agent teams for complex refactors.
version: 1.0.0
mcp_servers:
  - senclaw-code
  - senclaw-code-graph
personas:
  single_agent: code-agent
  dag_team:
    - code-planner
    - code-implementer
    - code-reviewer
    - code-tester
    - code-integrator
---

# Code — Agent-Driven Code Editing

The Code feature lets agents read, write, and refactor code inside a **sandboxed git-backed workspace**. Every file write is git-committed for rollback. Two modes are available:

---

## Mode 1 — Single Agent (`code-agent`)

Use for tasks touching 1–3 files. The agent uses the `senclaw-code` MCP server directly.

**Tools available:**

| Tool | Description |
|------|-------------|
| `read_file` | Read file with line numbers; supports start_line/end_line range |
| `write_file` | Create or overwrite a file; returns unified diff |
| `edit_file` | Exact string replacement; `old_str` must be unique; returns diff |
| `bash` | Run shell command in workspace (timeout 30s default, max 300s) |
| `search_code` | AST pattern search via ast-grep, falls back to grep |
| `glob` | Find files by glob pattern |
| `get_skeleton` | Token-efficient function signatures + struct declarations |
| `list_files` | Directory tree view |

**Workflow the agent follows:**
1. `get_skeleton` or `list_files` to orient
2. `read_file` the relevant file(s)
3. `edit_file` or `write_file` to apply change
4. `bash` to verify (build / lint / test)
5. Report changed files with line references

---

## Mode 2 — DAG Team (`code-planner` + team)

Use when a task affects ≥ 4 files or needs independent review/test passes.

**DAG structure:**
```
code-planner
  ├── code-implementer (parallel, per module)
  ├── code-implementer (parallel, per module)
  └── ...
       ├── code-reviewer  (after all implementers)
       ├── code-tester    (after all implementers)
       └── code-integrator (after reviewer + tester)
```

The Planner writes `.senclaw-code/context.json` — a shared skeleton + assignments file — before dispatching. Each Implementer reads only its assigned files.

---

## Knowledge Graph (`senclaw-code-graph`)

Available alongside the editing tools:

| Tool | Description |
|------|-------------|
| `graph_reindex` | Index/update the code symbol graph for the workspace |
| `graph_find_callers` | Who calls symbol X? |
| `graph_find_callees` | What does X call? |
| `graph_impact` | Blast-radius BFS: what breaks if X changes? |
| `graph_symbol_context` | Full context: signature + callers + callees |
| `graph_trace_flow` | DFS call tree from an entry point |
| `graph_search` | Full-text search on symbol names + signatures |
| `graph_skeleton` | Token-efficient skeleton from the indexed graph |
| `graph_file_deps` | File-level import/importer graph |

**Typical pre-refactor workflow:**
```
graph_impact { name: "process_payment", depth: 3 }
graph_symbol_context { name: "process_payment" }
read_file { path: "src/payment/processor.rs", start_line: 120, end_line: 200 }
edit_file { ... }
graph_reindex { incremental: true }
```

---

## Security

All file tools enforce workspace sandbox boundaries. Paths containing `../` that would escape the workspace are rejected with `Path traversal denied`.
