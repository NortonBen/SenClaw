---
name: mindmap-expand
description: >-
  Expand, detail, restyle, or re-layout an EXISTING mind map in the SenClaw Mindmap app.
  Use when a map already exists and the user wants to grow or change it — e.g. "mở
  rộng / phát triển / đào sâu nhánh X", "thêm ý cho sơ đồ", "add subtopics", "flesh out
  this node", "đổi màu/kiểu nút", or "đổi bố cục sơ đồ / change the layout". Do NOT use
  this to create a brand-new map from scratch — use mindmap-generate for that.
---

# mindmap-expand

Grow and refine an existing mind map in the **SenClaw Mindmap** app via the
`mindmap-mcp` MCP server.

## When to use this skill

- The user wants **more ideas under an existing node/branch** ("mở rộng nhánh…",
  "add subtopics", "đào sâu phần…").
- The user wants to **restyle** nodes — colors, shapes, fill, icons.
- The user wants to **change the layout** of an existing map (mind map ↔ org chart ↔
  list ↔ horizontal tree).

If there is no map yet, or the user names a fresh topic, use **mindmap-generate**.

## Steps

1. **Locate the map and node.** `mcp__mindmap-mcp__mindmap_list` to find the map, then
   `mcp__mindmap-mcp__mindmap_get` to read its tree and find the target node by its
   `text` (keep its `id`).
2. **Expand a branch.** `mcp__mindmap-mcp__mindmap_generate` with `parent_id` = that
   node's `id`; pass guidance as `instruction` (e.g. "focus on risks", "3 branches").
   Set `replace: true` ONLY if the user wants to overwrite that branch's children.
3. **Precise edits / styling.** `mcp__mindmap-mcp__mindmap_add_node` for one exact idea;
   `mcp__mindmap-mcp__mindmap_update_node` to change `text`, `note`, `color`, `shape`
   (`rounded|rect|pill|ellipse|line`), `fill`, or `icon` (a single emoji).
4. **Re-layout.** `mcp__mindmap-mcp__mindmap_set_layout` with `mindmap` | `org` |
   `outline` | `right`.
5. **Confirm.** Re-read with `mindmap_get` and summarize what changed.

## Notes

- Default to **appending** (leave `replace` false) so existing work is preserved.
- Reply in the user's language (Vietnamese or English).
