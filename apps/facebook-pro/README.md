# Facebook Pro — SenClaw Space App

Manage your Facebook **Pages** through the **official Facebook Graph API**, driven
by your own **Facebook Developer App**. No scraping, no session/cookie stealing,
no anti-bot evasion, no bulk posting. Every post / comment / reply is **draft-first**:
composed into an approval queue and only published when you press **Approve** (or
after you explicitly switch autonomy to `live`).

Port `4590`. MCP server `facebook-mcp` (tools `fb_*`). Local state (App Secret,
user token, per-page tokens) lives only in this app's SQLite and is sent only to
`graph.facebook.com`.

## What it does

- **Post** to a Page: text, link, or photo (by image URL); **edit** and **delete** posts.
- **Comments**: read comments on a post, **reply** to a comment, add a comment, **like** an object.
- **Messages (Inbox)**: list the Page's Messenger conversations, open a thread, and
  **reply** to a user (Send API, draft-first). Needs `pages_messaging`.
- **Overview**: a dashboard aggregating recent-post interactions (reactions / comments
  / shares totals, top posts, pending drafts) for a quick stats summary.
- **Analyze** a post with AI (engagement read + suggestions).
- **Insights**: Page-level and post-level statistics from the official Insights API.
- **Ads (Marketing API)**: read CTR / CPC / CPM / spend / results / ROAS per
  campaign / adset / ad; an AI verdict on whether each is effective or *burning
  money* (đốt tiền) and should be paused; pause/resume an ad entity.
- **Auto-reply**: a heartbeat polls new comments and, per your **rule triggers**
  (keyword / question / all), drafts a reply or logs a notification.
- **Draft-first autonomy**: `observe` (read-only) · `draft` (queue drafts, default) · `live` (auto-publish).

## Setup — create a Facebook Developer App

1. Go to <https://developers.facebook.com/apps> → **Create App** → type **Business**.
2. Add the **Facebook Login** product. Under *Facebook Login → Settings*, add a
   Valid OAuth Redirect URI matching this app's callback
   (e.g. `http://127.0.0.1:4590/api/oauth/callback`).
3. From *App Settings → Basic* copy **App ID** and **App Secret** → paste them in
   the app's **Connect** tab.
4. Click **Connect (OAuth)** and grant the scopes below, OR paste a User Access
   Token from the [Graph API Explorer](https://developers.facebook.com/tools/explorer/)
   into **Paste token**. The app exchanges it for a long-lived token (~60 days)
   and fetches your Pages (each with a permanent Page Access Token).
5. Pick the Page to act on in the **Pages** tab.

### Running inside the SenClaw desktop app

Server Space Apps load directly in an embedded webview. Facebook **blocks OAuth
inside embedded webviews** (`disallowed_useragent`), so the app opens the authorize
link (and the Developers Console / Graph API Explorer links) in the user's **system
browser** via the host bridge (`flutter_inappwebview.callHandler('senclawOpenExternal', url)`
— handled in `desktop_app/lib/widgets/embedded_web_stub.dart`). After granting access
in the browser, Facebook redirects to `http://127.0.0.1:4590/api/oauth/callback` — this
app's own local server — which stores the token; return to the app and the status
updates within seconds. In a plain browser the links just open a new tab.

### Scopes

`pages_show_list`, `pages_manage_posts`, `pages_read_engagement`,
`pages_manage_engagement`, `pages_read_user_content`, `pages_messaging` (inbox),
`read_insights`, `ads_read` (ad insights), `ads_management` (pause/resume ads).

The three-tier token model (short-lived user token → long-lived user token →
permanent page token) is exactly the official Graph API flow; page tokens are
what actually post/read on a Page.

## Boundary

- Official Graph API only. You can only act on Pages **you administer** with your
  own Developer App. There is no scraping of other pages and no bulk/broadcast path.
- Writes that create public content (post, photo, comment, reply, edit) go through
  the draft-approve gate. Default autonomy is `draft`.
