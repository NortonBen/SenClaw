---
name: mindmap-generate
description: >-
  Create a NEW mind map on a topic and populate it with a structured hierarchy of
  sub-topics, in the SenClaw Mindmap app. Use when the user wants to START a map from
  scratch — e.g. "tạo/vẽ sơ đồ tư duy về X", "make a mind map for X", "brainstorm X as
  a mind map", "map out X", a SWOT/empathy/project map, or "use a mindmap template".
  Do NOT use this for adding to a map that already exists — use mindmap-expand instead.
---

# mindmap-generate

Build a brand-new mind map in the **SenClaw Mindmap** app via the `mindmap-mcp` MCP
server, then flesh it out.

## When to use this skill

- The user asks to **create / draw / make** a mind map, sơ đồ tư duy, or brainstorm
  diagram on a topic.
- The user names a **known structure** — SWOT, empathy map, project plan, study/exam
  map, meeting agenda — that maps cleanly onto a template.
- The user wants to pick a **layout style** (mind map / org chart / list / horizontal
  tree) for a new map.

If a map already exists and the user just wants MORE detail on part of it, use
**mindmap-expand** instead.

## Steps

1. **Prefer a template when one fits.** Call `mcp__mindmap-mcp__mindmap_templates` to
   list starter templates (SWOT, empathy map, project plan, exam revision, macro-
   economics, team sync, campaign brainstorm). If one matches the request, instantiate
   it with `mcp__mindmap-mcp__mindmap_from_template` (`template_id`, optional `title`)
   — this sets a good layout and a styled tree in one call. Then jump to step 4.
2. **Otherwise create the map.** Call `mcp__mindmap-mcp__mindmap_create` with the topic
   as `title` and an optional `layout` (`mindmap` default, `org` for hierarchies,
   `outline` for agendas/lists, `right` for a horizontal tree). Keep the returned `id`
   and `rootId`.
3. **Generate the structure.** Call `mcp__mindmap-mcp__mindmap_generate` with
   `parent_id` = `rootId`, `topic` = the subject, and any constraints (branch count,
   focus, audience) as `instruction`. This inserts a balanced multi-level hierarchy.
4. **Refine (optional).** Add precise nodes with `mindmap_add_node`; style them with
   `mindmap_update_node` (`color`, `shape`, `fill`, `icon`); switch layout with
   `mindmap_set_layout`.
5. **Show the result.** Call `mcp__mindmap-mcp__mindmap_get` and give the user a short
   outline, noting it's open in the Mindmap app.

## Notes

- `mindmap_generate` is best for breadth; `mindmap_add_node` for exact, user-dictated
  nodes. Labels stay short (2–6 words); longer detail goes in a node's `note`.
- Generated labels follow the language of the `topic`/`instruction` you pass.
- Reply in the user's language (Vietnamese or English).
