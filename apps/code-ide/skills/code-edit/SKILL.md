---
name: code-edit
description: Make a precise, applyable edit to a file in the open SenClaw Code workspace. Use when the user asks to change, refactor, or write code into a specific file.
---

# code-edit

Apply a focused change to a file in the currently open **SenClaw Code** workspace,
using the `code-ide-mcp` MCP server.

## Steps

1. **Locate the target.** If the user named a file, use it. Otherwise use
   `mcp__code-ide-mcp__ide_search` to find the relevant symbol/text, or
   `mcp__code-ide-mcp__ide_list_dir` to browse.
2. **Read before writing.** Call `mcp__code-ide-mcp__ide_read_file` to get the exact
   current contents. Never overwrite blind.
3. **Make the change** with `mcp__code-ide-mcp__ide_write_file` (full new file
   content). Keep the diff minimal and match the file's existing style.
4. **Verify the blast radius.** If you changed a function signature or exported name,
   `ide_search` for its callers and update or flag them.
5. **Summarize** what changed as `path:line` references.

## Notes

- All paths are **workspace-relative** (the folder opened in the editor).
- Writes are reflected live in the editor and picked up by its file watcher.
- If no workspace is open, tell the user to open a folder first (or call
  `mcp__code-ide-mcp__ide_open` with an absolute path).
