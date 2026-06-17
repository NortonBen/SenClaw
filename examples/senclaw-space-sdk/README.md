# SenclawSpace SDK

TypeScript SDK for Space Apps.

## Init

Browser apps should wait for the host runtime:

```ts
import { SenclawSpace } from '@senclaw/space-sdk';

const space = await SenclawSpace.init();
```

The host iframe sends `senclaw:init` with:

- `appId`
- `apiBase`
- `coreBase`
- config KV endpoint
- app SQLite endpoint
- MCP registration endpoint

Node-side app processes can pass env directly:

```ts
const space = new SenclawSpace({
  appId: process.env.SENCLAW_SPACE_APP_ID,
  apiBase: process.env.SENCLAW_SPACE_API_BASE,
});
```

## Config KV

```ts
await space.setConfig('settings', { days: 30 });
const settings = await space.getConfig('settings');
```

## App SQLite

Each Space App gets a private SQLite database under its install directory.

```ts
await space.sqlite('CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY, created_at INTEGER)');
await space.sqlite('INSERT INTO runs (created_at) VALUES (?1)', [Date.now()]);
const rows = await space.sqlite('SELECT * FROM runs ORDER BY id DESC LIMIT 10');
```

## Register MCP Back To SenClaw

If the app starts its own MCP server, register it back to SenClaw:

```ts
await space.registerMcp({
  name: 'google-workspace-mcp',
  transport: 'http',
  url: 'http://127.0.0.1:4107/mcp',
  description: 'Google Workspace tools exposed by the Space App',
});
```

SenClaw persists the MCP server in project scope and connects it through the normal MCP manager.

## Run An MCP Server From The SDK (Node-only)

The `@senclaw/space-sdk/mcp` subpath turns a Space App into a local MCP server in
a few lines. It bundles the Streamable HTTP transport, a settings tool pair
(read/write over the config KV), Origin protection, and — importantly — an
`Accept`-header compatibility shim so the **current SenClaw Rust MCP client**
(which sends no `Accept` header) interoperates with the strict MCP TypeScript SDK
transport (which would otherwise reply HTTP 406).

```ts
import { serveSpaceMcp } from '@senclaw/space-sdk/mcp';

await serveSpaceMcp({
  appId: 'google-workspace',
  toolPrefix: 'gworkspace',           // → gworkspace_get_settings / gworkspace_set_settings
  settings: {
    key: 'google-workspace-settings', // shared with the app UI
    defaults: { days: 7, services: ['gmail'], mcpPort: 4107, mcpName: 'google-workspace-mcp' },
    normalize,                         // coerce stored value → typed settings
    patchSchema,                       // optional Zod shape for typed set_settings
  },
  registerTools: (ctx) => {
    // ctx.server (McpServer), ctx.space (SenclawSpace), ctx.getSettings/setSettings
    ctx.server.registerTool('gworkspace_sync', { /* ... */ }, async (args) => {
      const r = await ctx.space.core('space/sync/google-workspace', { method: 'POST', /* ... */ });
      return { content: [{ type: 'text', text: JSON.stringify(r) }], structuredContent: r };
    });
  },
  autoRegister: true,                  // optional: self-register with SenClaw on startup
});
```

> The root export (`@senclaw/space-sdk`) is browser+node safe with no runtime
> deps. The `/mcp` subpath is **Node-only** — it pulls in `express` and the MCP
> SDK, so import it from server processes only, never from browser app code.

A complete example lives in
[`google-workspace-space-app/mcp-server`](../google-workspace-space-app/mcp-server).
