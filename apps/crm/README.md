# SenClaw CRM

A personal CRM Space App for SenClaw.

- **Customers** — name, avatar (URL or inline base64 upload), email, phone,
  company, title, address, birthday, source, notes, free-form tags, pipeline
  status (`lead|prospect|customer|vip|paused|lost`).
- **Interactions** — chronological log of `call` / `email` / `meeting` / `note`
  / `task` touchpoints per customer.
- **Search & filter** — free-text over name/email/phone/company/notes + tag
  chips + status chips.
- **AI briefing** — one-click summary + next-step suggestion, grounded in the
  stored profile + recent interactions via the daemon's active LLM.
- **MCP** — `crm-mcp` (11 tools) so any SenClaw agent can look a customer up,
  log an interaction, or create/update records from real context.
- **Skills** — `crm-quick-lookup` (reads), `crm-log-interaction` (writes).
- **Persona** — `crm-assistant`.

## Data

SQLite at `~/.senclaw/space-apps/crm/crm.db` (WAL). Two tables:

```
customers(id, name, email, phone, company, title, avatar_url, notes,
          tags_json, status, source, address, birthday,
          created_at, updated_at)
interactions(id, customer_id, kind, summary, details,
             occurred_at, created_at)
```

Avatars are stored as a URL — either an external `https://…` or an inline
`data:image/…;base64,…` uploaded from the UI. No blob column, no external
CDN, no orphan files.

## Ports

`PORT=4390` by default. Manifest: `runtime.port = 4390`.

## Dev

```
cargo run -p crm                       # backend on :4390
(cd apps/crm/web && npm install && npm run dev)   # web on :5173 → proxy /api
```

Then point SenClaw daemon at this Space App via `apps/crm/senclaw-manifest.json`.

## Pack

```
bash apps/crm/scripts/pack.sh
```

Produces `apps/crm/crm-app.zip` — the installer you upload in
**SenClaw → Apps → Install**.
