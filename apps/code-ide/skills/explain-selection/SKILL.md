---
name: explain-selection
description: Explain what a piece of code does, grounded in the real source in the open SenClaw Code workspace. Use when the user asks what some code/file/function does or how it works.
---

# explain-selection

Explain code from the open **SenClaw Code** workspace, grounded in the actual source
via the `code-ide-mcp` MCP server.

## Steps

1. **Get the code.** If the user pinned a selection or named a file/symbol, read it
   with `mcp__code-ide-mcp__ide_read_file`. To find a symbol by name, use
   `mcp__code-ide-mcp__ide_search`.
2. **Follow the threads.** If the code calls into other files, `ide_read_file` those
   too so the explanation is concrete, not hand-wavy.
3. **Explain** the purpose, the flow (inputs → steps → outputs), and any notable edge
   cases or gotchas. Cite concrete `path:line` locations.
4. Keep it skimmable: a one-line summary first, then the details.

## Notes

- Reply in the same language as the user's question (Vietnamese or English).
- Do not invent behavior — if something is unclear from the source, say so and point
  to where the answer would live.
