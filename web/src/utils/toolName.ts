/**
 * Canonicalize an MCP tool name to the stripped "bridge" form used everywhere
 * in the UI for matching. Mirrors `normalize_mcp_tool_name` in the Rust daemon
 * (src/tools/tool_search.rs).
 *
 * Tools register under their full server prefix
 * (`mcp__senclaw-browser__browser_search`), but the model — and the skill docs —
 * call them by the stripped form (`mcp__browser__search`). Events carry whichever
 * form the model emitted, so the UI must fold both onto the same key before
 * matching, or labels/icons silently fall through to the generic fallback.
 *
 *   mcp__senclaw-browser__browser_search → mcp__browser__search
 *   mcp__senclaw-space__space_event_create → mcp__space__event_create
 *   mcp__senclaw-memory__memory_search → mcp__memory__search
 *   Bash / mcp__browser__search (already canonical) → unchanged
 */
export function normalizeMcpName(name: string): string {
  const PREFIX = 'mcp__senclaw-';
  if (!name.startsWith(PREFIX)) return name;
  const rest = name.slice(PREFIX.length);
  const sep = rest.indexOf('__');
  if (sep === -1) return name;
  const server = rest.slice(0, sep);
  const tool = rest.slice(sep + 2);
  const toolPrefix = `${server}_`;
  const cleanTool = tool.startsWith(toolPrefix) ? tool.slice(toolPrefix.length) : tool;
  return `mcp__${server}__${cleanTool}`;
}
