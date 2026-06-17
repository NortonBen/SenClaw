# QQ Channel Binding Guide

## Prerequisite: Create a QQ Bot

> 🦞 **Lobster-specific entry**: [https://q.qq.com/qqbot/openclaw/login.html](https://q.qq.com/qqbot/openclaw/login.html)
>
> Log in by scanning QR code, create a bot, and record the **App ID** and **App Secret** (Client Secret).

---

## Method 1: Primary App (`.env` configuration)

Use this for the first / primary QQ bot by writing directly to `.env`.

```bash
# .env
QQ_APP_ID=1234567890
QQ_APP_SECRET=xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

# Optional: sandbox mode (for development/testing, default false)
# QQ_SANDBOX=true
```

Restart `senclaw` after changes. After the bot receives its first message, it auto-binds to a real JID (DM or group) with no manual setup.

---

## Method 2: Additional Apps (Web UI or CLI)

Use this to bind second/third QQ bots to different agent folders. Config is saved to `~/.senclaw/config.json` and takes effect immediately without restart.

### Web UI Configuration

Open Settings -> Agents -> **Add Agent**, then choose `QQ` as Channel.

| Field | Required | Description |
|------|------|------|
| Display Name | ✓ | Agent name shown in UI |
| Agent ID | ✓ | Target agent folder (lowercase letters/numbers/hyphens) |
| App ID | ✓ | QQ bot App ID |
| App Secret | ✓ | QQ bot App Secret |
| Sandbox Mode | | Enable for development/testing (off by default) |

> QQ channel **does not support manually entering JID**. Binding is completed automatically after the first incoming message (pending -> real JID).

### CLI Configuration

```bash
# Add (JID auto-binding)
senclaw channel qq add \
  --app-id 1234567890 \
  --app-secret xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx \
  --group mybot \
  --name "My QQ Bot"

# Sandbox mode
senclaw channel qq add \
  --app-id 1234567890 \
  --app-secret xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx \
  --group mybot-sandbox \
  --name "QQ Bot (Sandbox)" \
  --sandbox

# List
senclaw channel qq list
senclaw channel list            # all channel types

# Remove (also removes related group binding)
senclaw channel qq remove --app-id 1234567890
```

| Parameter | Required | Description |
|------|------|------|
| `--app-id` | ✓ | QQ bot App ID |
| `--app-secret` | ✓ | QQ bot App Secret |
| `--group` | ✓ | Target agent folder (lowercase letters/numbers/hyphens) |
| `--name` | | Optional display name (default `<folder>(qq)`) |
| `--sandbox` | | Enable sandbox mode (optional) |

> CLI add takes effect after restart; Web UI add is instant. Remove operations sync `config.json` in both cases.

> **Note**: `remove` only applies to Method 2. For the primary app in Method 1 (`.env`), manually clear the related fields in `.env` and restart.

---

## Pending Auto-Binding

QQ `openid` is assigned per App and cannot be known in advance. The binding flow is:

1. After adding a bot (CLI/Web UI), the system stores `qq:pending:{appId}`.
2. When the bot receives the **first message**, pending is migrated to a real JID (e.g., `qq:user:XXXX` or `qq:group:XXXX`).
3. After migration, the agent processes that same message immediately (no resend needed).

> Each pending binding can migrate to **only one** JID (wherever the first message comes from). To support both DM and group chat, add two bindings (same App ID, different folders).

---

## Trigger Behavior

| Scenario | Default behavior |
|------|---------|
| Bot in DM | Every message triggers the agent |
| Group (`requiresTrigger = true`) | Must @mention the bot |
| Group (`requiresTrigger = false`) | Every message triggers the agent |

`requiresTrigger` can be changed in Agent settings in Web UI.

---

## FAQ

**Q: The bot connects but receives no messages**  
-> Make sure WebSocket long connection is enabled on QQ Open Platform and bot permission scopes are configured correctly.

**Q: Permission/approval UI shows numbered menu instead of buttons**  
-> This is expected. QQ bot inline buttons (Markdown Keyboard) need extra platform approval. If not approved, it falls back to numbered text menu; reply with the number to operate.

**Q: What is the difference between sandbox and production mode?**  
-> Sandbox mode connects to QQ test environment. Messages only circulate among test accounts and do not affect production users. Recommended for development/debugging.

**Q: Can one QQ bot bind to multiple groups?**  
-> Currently no. Each App ID can bind to **one** pending binding only (first message determines JID). Use multiple bot apps to serve multiple groups.

**Q: First message arrived after adding, but the agent did not start**  
-> Check backend logs for `QQ pending binding completed`. If missing, confirm bot has been restarted (CLI method requires restart) and `addApp` connection succeeded.
