# @senclaw/space-sdk

Build a **Space App** for [SenClaw](https://github.com/NortonBen/SenClaw) in TypeScript.

A Space App is a small HTTP server the SenClaw daemon installs, launches, and
embeds: its UI shows up in an iframe, its MCP tools show up in every agent's
roster, and it reaches AI, memory, storage and settings through the daemon
rather than holding provider keys of its own. This SDK is that contract, typed.

```bash
npm install @senclaw/space-sdk
```

Node ≥ 18. TypeScript types ship with the package.

> Other languages: [Python](../senclaw-app-sdk-python) ·
> [Go](../senclaw-app-sdk-go) · [Rust](../../app-space-sdk).

## What you get

| Import | What's in it | Runs where |
|---|---|---|
| `@senclaw/space-sdk` | `SenclawSpace` — AI bridge, knowledge, config KV, per-app SQLite, model list | browser + Node |
| `@senclaw/space-sdk/lifecycle` | bind host, port, graceful shutdown, manifest definition + validation | Node |
| `@senclaw/space-sdk/mcp` | `serveSpaceMcp` — a working MCP server in a few lines | Node |
| `@senclaw/space-sdk/dispatch` | be driven by the daemon's autonomous work dispatcher | Node |
| `npx senclaw-manifest` | validate `senclaw-manifest.json` in CI | CLI |

The root export touches nothing but `fetch`, so it is safe in browser app code.
The three subpaths are Node-only — `/mcp` and `/dispatch` reach for `express`,
and `/lifecycle` reads `process.env` and installs signal handlers.

## Quick start

```ts
import { SenclawSpace } from '@senclaw/space-sdk';
import { appPort, bindHost, onShutdown } from '@senclaw/space-sdk/lifecycle';
import express from 'express';

const space = SenclawSpace.forDaemon(process.env.SENCLAW_SPACE_APP_ID!);
const app = express();

app.get('/api/status', (_req, res) => res.json({ ok: true }));

// Loopback unless the operator explicitly opted out. A Space App authenticates
// nothing of its own, so binding 0.0.0.0 publishes its whole REST + MCP surface
// to anyone on the network.
const server = app.listen(appPort(4810), bindHost());

// A session app is stopped when it goes idle: SIGTERM, then SIGKILL ~2s later.
onShutdown(async () => { server.close(); await db.close(); });
```

Alongside it, a `senclaw-manifest.json` telling the daemon how to run the app:

```jsonc
{
  "id": "my-app",
  "name": "My App",
  "runtime": {
    "kind": "server",
    "mode": "session",          // "background" to stay resident — see the trap below
    "runner": "node",
    "start": "node server.mjs",
    "healthPath": "/api/status",
    "port": 4810
  }
}
```

Install it into a running daemon without publishing anything:

```bash
curl -X POST http://127.0.0.1:18788/api/space/apps/register-local \
  -H 'Content-Type: application/json' -d "{\"path\": \"$(pwd)\"}"
```

A complete runnable app — two MCP tools, a UI page, health, SIGTERM — is in
[`examples/space-app-node-demo`](https://github.com/NortonBen/SenClaw/tree/main/senclaw-sdk/senclaw-app-sdk/examples/space-app-node-demo).
It hand-rolls the JSON-RPC on purpose, so it doubles as the reference for what
the wire protocol actually is.

## Daemon services

The app never holds a provider API key. Everything below runs on whatever
provider the *user* configured, through the daemon.

```ts
const space = SenclawSpace.forDaemon('my-app');     // server process
const space = await SenclawSpace.init();            // browser (waits for the host)

await space.capabilities();                          // what this daemon supports

const text = await space.llm({ prompt, system, maxTokens, profile });
const full = await space.llmDetailed({ prompt });    // + model, finish, usage
const done = await space.agent('Find every TODO and file them');  // full agent turn

await space.knowledgeSave('remember this', { space: 'proj', tags: ['x'] });
await space.knowledgeSearch('query', { space: 'proj' });   // raw hits
await space.knowledgeRecall('query', { space: 'proj' });   // synthesized answer

const { activeId, models } = await space.listModels();
await space.usageReport({ model, provider, inputTokens, outputTokens });
```

Four things that bite:

- **`llm()` throws on a truncated reply.** `finish === 'length'` means the model
  hit `maxTokens` mid-sentence, and a fragment is indistinguishable from a short
  answer. Use `llmDetailed()` when you want to handle that yourself.
- **A failed bridge action arrives as HTTP 200**, carrying
  `{status: "error", message}`. The SDK throws on it. If you ever call the
  bridge by hand, check that envelope — otherwise a dead provider reads as an
  empty string.
- **`profile` beats `setActiveModel()`.** The active model is *global*: the
  agent and every other app share it. Pin your app's model with `profile`.
- **There is no temperature knob.** Only `system`, `prompt`, `maxTokens` and
  `profile` are read; anything else is ignored rather than honoured.

Each knowledge *space* is an independent partition. Omit `space` and you get the
app's own private one, named after the app id — so an app that never passes one
can neither read nor pollute anybody else's memory.

## Config KV and per-app SQLite

```ts
await space.setConfig('settings', { days: 30 });
const settings = await space.getConfig('settings');   // null when unset

await space.sqlite('CREATE TABLE IF NOT EXISTS runs (id INTEGER PRIMARY KEY, at INTEGER)');
await space.sqlite('INSERT INTO runs (at) VALUES (?1)', [Date.now()]);
const { rows } = await space.sqlite('SELECT * FROM runs ORDER BY id DESC LIMIT 10');
```

Config KV is the same store the app's own UI reads and writes — use it rather
than a file in the app directory, which an update overwrites. SQLite is a
private database per app; always pass values as parameters, never format them
into the SQL.

## MCP server

`/mcp` turns the app into an MCP server in a few lines: Streamable HTTP
transport, a settings tool pair over the config KV, Origin protection, and an
`Accept`-header shim so SenClaw's Rust MCP client (which sends no `Accept`)
interoperates with the strict MCP TypeScript transport that would otherwise
answer HTTP 406.

```ts
import { serveSpaceMcp } from '@senclaw/space-sdk/mcp';

await serveSpaceMcp({
  appId: 'my-app',
  toolPrefix: 'myapp',                 // → myapp_get_settings / myapp_set_settings
  settings: {
    key: 'my-app-settings',            // shared with the app UI
    defaults: { days: 7, mcpPort: 4810 },
    normalize,                         // coerce stored value → typed settings
    patchSchema,                       // optional Zod shape for typed set_settings
  },
  registerTools: (ctx) => {
    ctx.server.registerTool('myapp_sync', { /* ... */ }, async (args) => {
      const r = await ctx.space.core('space/sync/my-app', { method: 'POST' });
      return { content: [{ type: 'text', text: JSON.stringify(r) }], structuredContent: r };
    });
  },
  autoRegister: true,                  // self-register with SenClaw on startup
});
```

Already running your own MCP server? Register it instead:

```ts
await space.registerMcp({
  name: 'my-app-mcp',
  transport: 'http',
  url: 'http://127.0.0.1:4810/mcp',
  description: 'Tools exposed by My App',
});
```

SenClaw persists it in project scope and connects it through the normal MCP
manager. Tools then resolve as `mcp__my-app-mcp__myapp_sync`.

## Dispatch

Make the app drivable by the daemon's autonomous work dispatcher — it claims
work from you, keeps leases alive, recovers items whose worker died, and reports
terminal outcomes back:

```ts
import { dispatchRouter, outcome } from '@senclaw/space-sdk/dispatch';

app.use('/api/dispatch', await dispatchRouter({
  claimReady: (cap) => store.claim(cap.total),    // must be atomic
  finalize: (id, o) => store.close(id, o),
}));
```

`heartbeat` and `reclaim` are optional — a source with no lease model shouldn't
have to write two empty functions. Not on Express? `handleDispatch(provider,
action, body)` returns `{status, body}` for any server.

Field names are snake_case (`depends_on`, `timeout_secs`, `item_id`) because the
engine parses them with serde: camelCase is dropped silently, which surfaces as
a dependency that never held rather than as an error.

## Manifest validation

```bash
npx senclaw-manifest senclaw-manifest.json
```

Non-zero exit on the mistakes that otherwise fail *silently*: a misspelled
`runtime.mode` (which quietly falls back to `session`, so an always-on app stops
after a minute of idle), `network: "hosts"` with an empty allowlist (which
leaves the app with no network at all), `mcp.autoRegister` with neither `path`
nor `url`. The same checks are available in code:

```ts
import { defineManifest, validateManifest } from '@senclaw/space-sdk/lifecycle';

export default defineManifest({ id: 'my-app', runtime: { /* ... */ } });  // throws
const problems: string[] = validateManifest(json);                        // or inspect
```

## Environment the daemon injects

| Variable | Meaning |
|---|---|
| `PORT` | The port assigned to this launch — always prefer it over the manifest's |
| `SENCLAW_SPACE_APP_ID` | This app's id |
| `SENCLAW_BASE_URL` | Daemon base URL, default `http://127.0.0.1:18788` |
| `SENCLAW_BIND_HOST` | Bind host; absent means loopback, and loopback is the right default |

## Related

- [SenClaw](https://github.com/NortonBen/SenClaw) — the daemon this plugs into
- [Space App lifecycle](https://github.com/NortonBen/SenClaw/blob/main/docs/space-app-lifecycle.md) — `background` vs `session`, `requires`, `sandbox`, `runner`
- [Publishing guide](https://github.com/NortonBen/SenClaw/blob/main/docs/space-app-sdk-publish-guide.md) — building an app in its own repo and shipping it to the hub
- Same SDK in other languages: [Python](https://github.com/NortonBen/SenClaw/tree/main/senclaw-sdk/senclaw-app-sdk-python) · [Rust](https://github.com/NortonBen/SenClaw/tree/main/app-space-sdk)

MIT
