//! Query/mutation helpers over the SQLite store, shared by the REST API and MCP.

use anyhow::{anyhow, Result};
use rusqlite::params;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::db::Db;
use crate::mailer::FetchedMsg;
use crate::models::{Account, AccountCreate, AccountSecret};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Correct well-known host typos so common muscle-memory values still work.
/// Gmail's servers are `imap.gmail.com` / `smtp.gmail.com`, NOT `*.google.com`,
/// which is a frequent reason accounts "won't log in".
pub fn normalize_host(host: &str) -> String {
    let h = host.trim().trim_end_matches('.').to_lowercase();
    match h.as_str() {
        "imap.google.com" | "imap.googlemail.com" => "imap.gmail.com".into(),
        "smtp.google.com" | "smtp.googlemail.com" => "smtp.gmail.com".into(),
        "pop.google.com" => "pop.gmail.com".into(),
        _ => h,
    }
}

/// Gmail app-passwords are 16 characters shown in four space-separated groups
/// (e.g. `abcd efgh ijkl mnop`); the real secret has no spaces. Users routinely
/// paste the spaced form, which fails auth — strip the spaces when the value
/// unambiguously matches that shape, and leave every other password untouched.
pub fn normalize_app_password(pw: &str) -> String {
    let has_ws = pw.chars().any(|c| c.is_whitespace());
    let compact: String = pw.chars().filter(|c| !c.is_whitespace()).collect();
    if has_ws && compact.len() == 16 && compact.chars().all(|c| c.is_ascii_alphanumeric()) {
        compact
    } else {
        pw.to_string()
    }
}

/// Clean up an incoming create payload in place: trim text fields, fix host
/// typos, and de-space Gmail app passwords.
pub fn normalize_account(b: &mut AccountCreate) {
    b.label = b.label.trim().to_string();
    b.email = b.email.trim().to_string();
    b.username = b.username.trim().to_string();
    b.imap_host = normalize_host(&b.imap_host);
    b.smtp_host = normalize_host(&b.smtp_host);
    b.password = normalize_app_password(&b.password);
}

/// Field/port validation shared by create and the connection-test endpoint.
pub fn validate_account(b: &AccountCreate) -> Result<()> {
    if b.label.trim().is_empty()
        || b.email.trim().is_empty()
        || b.imap_host.trim().is_empty()
        || b.smtp_host.trim().is_empty()
        || b.username.trim().is_empty()
        || b.password.is_empty()
    {
        return Err(anyhow!("Missing required email account fields"));
    }
    if !(1..=65_535).contains(&b.imap_port) || !(1..=65_535).contains(&b.smtp_port) {
        return Err(anyhow!("Invalid email port"));
    }
    Ok(())
}

/// Build an in-memory secret (not persisted) so a payload can be verified
/// against the live IMAP server before it is saved.
pub fn secret_from_create(b: &AccountCreate) -> AccountSecret {
    AccountSecret {
        id: String::new(),
        email: b.email.clone(),
        imap_host: b.imap_host.clone(),
        imap_port: b.imap_port,
        smtp_host: b.smtp_host.clone(),
        smtp_port: b.smtp_port,
        username: b.username.clone(),
        password: format!("plaintext:{}", b.password),
        use_tls: b.use_tls,
    }
}

pub fn list_accounts(db: &Db) -> Result<Vec<Account>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, label, email, imap_host, imap_port, smtp_host, smtp_port, use_tls, created_at
             FROM space_email_accounts ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Account {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    email: row.get(2)?,
                    imap_host: row.get(3)?,
                    imap_port: row.get(4)?,
                    smtp_host: row.get(5)?,
                    smtp_port: row.get(6)?,
                    use_tls: row.get::<_, i32>(7)? != 0,
                    created_at: row.get(8)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    })
}

/// Resolve an account (with secret) by id, or the first configured account.
pub fn account_secret(db: &Db, account_id: Option<&str>) -> Result<AccountSecret> {
    db.with_conn(|conn| {
        let map = |row: &rusqlite::Row| {
            Ok(AccountSecret {
                id: row.get(0)?,
                email: row.get(1)?,
                imap_host: row.get(2)?,
                imap_port: row.get(3)?,
                smtp_host: row.get(4)?,
                smtp_port: row.get(5)?,
                username: row.get(6)?,
                password: row.get(7)?,
                use_tls: row.get::<_, i32>(8)? != 0,
            })
        };
        let cols = "id, email, imap_host, imap_port, smtp_host, smtp_port, username, password, use_tls";
        let mut acct = match account_id {
            Some(id) => conn.query_row(
                &format!("SELECT {cols} FROM space_email_accounts WHERE id=?1"),
                params![id],
                map,
            ),
            None => conn.query_row(
                &format!("SELECT {cols} FROM space_email_accounts ORDER BY created_at DESC LIMIT 1"),
                [],
                map,
            ),
        }
        .map_err(|e| anyhow!("No email account configured: {e}"))?;
        // Auto-heal legacy rows saved with a typo'd host (e.g. imap.google.com)
        // or a Gmail app-password that was stored with its display spaces.
        acct.imap_host = normalize_host(&acct.imap_host);
        acct.smtp_host = normalize_host(&acct.smtp_host);
        if let Some(inner) = acct.password.strip_prefix("plaintext:") {
            acct.password = format!("plaintext:{}", normalize_app_password(inner));
        }
        Ok(acct)
    })
}

pub fn create_account(db: &Db, b: &AccountCreate) -> Result<Account> {
    validate_account(b)?;

    let id = Uuid::new_v4().to_string();
    let now = now_ms();
    // Stored with a `plaintext:` prefix for parity with senclaw core.
    // TODO: AES-GCM encryption at rest.
    let password_stored = format!("plaintext:{}", b.password);

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO space_email_accounts
             (id, label, email, imap_host, imap_port, smtp_host, smtp_port, username, password, use_tls, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
            params![
                id, b.label, b.email, b.imap_host, b.imap_port,
                b.smtp_host, b.smtp_port, b.username, password_stored,
                b.use_tls as i32, now
            ],
        )?;
        Ok(())
    })?;

    Ok(Account {
        id,
        label: b.label.clone(),
        email: b.email.clone(),
        imap_host: b.imap_host.clone(),
        imap_port: b.imap_port,
        smtp_host: b.smtp_host.clone(),
        smtp_port: b.smtp_port,
        use_tls: b.use_tls,
        created_at: now,
    })
}

pub fn delete_account(db: &Db, id: &str) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute("DELETE FROM space_email_accounts WHERE id=?1", params![id])?;
        conn.execute("DELETE FROM space_email_cache WHERE account_id=?1", params![id])?;
        Ok(())
    })
}

/// Body text collapsed to a single-line preview for the message list.
fn snippet(body: Option<&str>) -> Option<String> {
    let body = body?;
    let mut out = String::new();
    let mut last_space = false;
    for c in body.chars() {
        if c.is_whitespace() {
            if !out.is_empty() && !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
            if out.chars().count() >= 140 {
                break;
            }
        }
    }
    let trimmed = out.trim_end().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// List cached messages in a folder, newest first.
///
/// `folder` is matched case-insensitively so callers can pass `inbox` or
/// `INBOX`; an unknown folder simply yields no rows.
pub fn inbox(
    db: &Db,
    account_id: Option<&str>,
    folder: Option<&str>,
    limit: u32,
) -> Result<Vec<Value>> {
    let folder = folder.unwrap_or("INBOX").to_string();
    db.with_conn(|conn| {
        let cols = "id, account_id, subject, from_addr, to_addrs, date, flags, folder, body_text";
        let map = |row: &rusqlite::Row| {
            Ok(json!({
                "id": row.get::<_,String>(0)?,
                "account_id": row.get::<_,String>(1)?,
                "subject": row.get::<_,Option<String>>(2)?,
                "from": row.get::<_,Option<String>>(3)?,
                "to": row.get::<_,Option<String>>(4)?,
                "date": row.get::<_,Option<i64>>(5)?,
                "flags": row.get::<_,String>(6)?,
                "folder": row.get::<_,String>(7)?,
                "snippet": snippet(row.get::<_,Option<String>>(8)?.as_deref()),
            }))
        };
        let rows: Vec<Value> = match account_id {
            Some(aid) => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {cols} FROM space_email_cache
                     WHERE account_id=?1 AND folder=?2 COLLATE NOCASE
                     ORDER BY date DESC LIMIT ?3"
                ))?;
                stmt.query_map(params![aid, folder, limit], map)?
                    .filter_map(|r| r.ok())
                    .collect()
            }
            None => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {cols} FROM space_email_cache
                     WHERE folder=?1 COLLATE NOCASE
                     ORDER BY date DESC LIMIT ?2"
                ))?;
                stmt.query_map(params![folder, limit], map)?
                    .filter_map(|r| r.ok())
                    .collect()
            }
        };
        Ok(rows)
    })
}

/// Per-folder totals plus the inbox unread count, for the sidebar badges.
///
/// Unread is derived the same way the UI does it: a message is unread when its
/// flags array has no `\Seen` entry.
pub fn folder_counts(db: &Db, account_id: Option<&str>) -> Result<Value> {
    db.with_conn(|conn| {
        let map = |row: &rusqlite::Row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?));
        let rows: Vec<(String, String)> = match account_id {
            Some(aid) => {
                let mut stmt = conn.prepare(
                    "SELECT folder, flags FROM space_email_cache WHERE account_id=?1",
                )?;
                stmt.query_map(params![aid], map)?
                    .filter_map(|r| r.ok())
                    .collect()
            }
            None => {
                let mut stmt = conn.prepare("SELECT folder, flags FROM space_email_cache")?;
                stmt.query_map([], map)?.filter_map(|r| r.ok()).collect()
            }
        };

        let (mut inbox, mut unread, mut sent) = (0_u32, 0_u32, 0_u32);
        for (folder, flags) in rows {
            if folder.eq_ignore_ascii_case("INBOX") {
                inbox += 1;
                if !flags_seen(&flags) {
                    unread += 1;
                }
            } else if folder.eq_ignore_ascii_case("Sent") {
                sent += 1;
            }
        }
        Ok(json!({ "inbox": inbox, "unread": unread, "sent": sent }))
    })
}

/// True when a stored flags array contains the IMAP `\Seen` flag.
fn flags_seen(flags: &str) -> bool {
    serde_json::from_str::<Vec<String>>(flags)
        .map(|f| f.iter().any(|x| x == "\\Seen"))
        .unwrap_or(false)
}

/// Add or remove `\Seen` on a cached message. Local-only: this does not push
/// the flag back to the IMAP server, so a re-sync from the server wins.
pub fn mark_read(db: &Db, id: &str, seen: bool) -> Result<Value> {
    db.with_conn(|conn| {
        let current: String = conn
            .query_row(
                "SELECT flags FROM space_email_cache WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .map_err(|e| anyhow!("Email not found: {e}"))?;

        let mut flags: Vec<String> = serde_json::from_str(&current).unwrap_or_default();
        let has = flags.iter().any(|f| f == "\\Seen");
        if seen && !has {
            flags.push("\\Seen".to_string());
        } else if !seen && has {
            flags.retain(|f| f != "\\Seen");
        }

        let encoded = serde_json::to_string(&flags)?;
        conn.execute(
            "UPDATE space_email_cache SET flags=?2 WHERE id=?1",
            params![id, encoded],
        )?;
        Ok(json!({ "success": true, "id": id, "flags": encoded }))
    })
}

pub fn read_msg(db: &Db, id: &str) -> Result<Value> {
    db.with_conn(|conn| {
        conn.query_row(
            "SELECT id, account_id, subject, from_addr, to_addrs, date, body_text, body_html, flags
             FROM space_email_cache WHERE id=?1",
            params![id],
            |row| {
                Ok(json!({
                    "id": row.get::<_,String>(0)?,
                    "account_id": row.get::<_,String>(1)?,
                    "subject": row.get::<_,Option<String>>(2)?,
                    "from": row.get::<_,Option<String>>(3)?,
                    "to": row.get::<_,Option<String>>(4)?,
                    "date": row.get::<_,Option<i64>>(5)?,
                    "body_text": row.get::<_,Option<String>>(6)?,
                    "body_html": row.get::<_,Option<String>>(7)?,
                    "flags": row.get::<_,String>(8)?,
                }))
            },
        )
        .map_err(|e| anyhow!("Email not found: {e}"))
    })
}

/// Keyword search over subject and body. Returns the same row shape as
/// [`inbox`] so the UI can render either list with one component.
///
/// `%` and `_` in the query are escaped so a literal underscore does not act as
/// a wildcard.
pub fn search(db: &Db, query: &str, account_id: Option<&str>, limit: u32) -> Result<Vec<Value>> {
    db.with_conn(|conn| {
        let escaped = query
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        let cols = "id, account_id, subject, from_addr, to_addrs, date, flags, folder, body_text";
        let map = |row: &rusqlite::Row| {
            Ok(json!({
                "id": row.get::<_,String>(0)?,
                "account_id": row.get::<_,String>(1)?,
                "subject": row.get::<_,Option<String>>(2)?,
                "from": row.get::<_,Option<String>>(3)?,
                "to": row.get::<_,Option<String>>(4)?,
                "date": row.get::<_,Option<i64>>(5)?,
                "flags": row.get::<_,String>(6)?,
                "folder": row.get::<_,String>(7)?,
                "snippet": snippet(row.get::<_,Option<String>>(8)?.as_deref()),
            }))
        };
        let rows: Vec<Value> = match account_id {
            Some(aid) => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {cols} FROM space_email_cache
                     WHERE account_id=?1
                       AND (subject LIKE ?3 ESCAPE '\\' OR body_text LIKE ?3 ESCAPE '\\'
                            OR from_addr LIKE ?3 ESCAPE '\\')
                     ORDER BY date DESC LIMIT ?2"
                ))?;
                stmt.query_map(params![aid, limit, pattern], map)?
                    .filter_map(|r| r.ok())
                    .collect()
            }
            None => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {cols} FROM space_email_cache
                     WHERE (subject LIKE ?2 ESCAPE '\\' OR body_text LIKE ?2 ESCAPE '\\'
                            OR from_addr LIKE ?2 ESCAPE '\\')
                     ORDER BY date DESC LIMIT ?1"
                ))?;
                stmt.query_map(params![limit, pattern], map)?
                    .filter_map(|r| r.ok())
                    .collect()
            }
        };
        Ok(rows)
    })
}

/// Upsert a batch of fetched messages into the INBOX cache for an account.
pub fn upsert_inbox(db: &Db, account_id: &str, msgs: &[FetchedMsg]) -> Result<usize> {
    let now = now_ms();
    db.with_conn(|conn| {
        let mut n = 0;
        for m in msgs {
            let flags = serde_json::to_string(&m.flags).unwrap_or_else(|_| "[]".into());
            conn.execute(
                "INSERT OR REPLACE INTO space_email_cache
                 (id, account_id, folder, subject, from_addr, to_addrs, date, body_text, body_html, flags, synced_at)
                 VALUES (?1, ?2, 'INBOX', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    m.id, account_id, m.subject, m.from, m.to, m.date,
                    m.body_text, m.body_html, flags, now
                ],
            )?;
            n += 1;
        }
        Ok(n)
    })
}

/// Record an outgoing message in the Sent folder after a successful SMTP send.
pub fn record_sent(
    db: &Db,
    account_id: &str,
    from: &str,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<String> {
    let msg_id = format!("out-{}", Uuid::new_v4());
    let now = now_ms();
    db.with_conn(|conn| {
        conn.execute(
            "INSERT OR IGNORE INTO space_email_cache
             (id, account_id, folder, subject, from_addr, to_addrs, date, body_text, flags, synced_at)
             VALUES (?1, ?2, 'Sent', ?3, ?4, ?5, ?6, ?7, '[\"\\\\Seen\"]', ?6)",
            params![msg_id, account_id, subject, from, to, now, body],
        )?;
        Ok(())
    })?;
    Ok(msg_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_gmail_host_typos() {
        assert_eq!(normalize_host("imap.google.com"), "imap.gmail.com");
        assert_eq!(normalize_host("SMTP.Google.com "), "smtp.gmail.com");
        assert_eq!(normalize_host("imap.googlemail.com"), "imap.gmail.com");
        assert_eq!(normalize_host("smtp.gmail.com."), "smtp.gmail.com");
        // Unknown hosts pass through (lower-cased + trimmed) unchanged.
        assert_eq!(normalize_host(" mail.example.com "), "mail.example.com");
    }

    #[test]
    fn strips_spaces_from_gmail_app_password() {
        assert_eq!(normalize_app_password("abcd efgh ijkl mnop"), "abcdefghijklmnop");
        // Not the 16-char app-password shape → left exactly as typed.
        assert_eq!(normalize_app_password("my real pass"), "my real pass");
        assert_eq!(normalize_app_password("secret-with-no-spaces"), "secret-with-no-spaces");
        assert_eq!(normalize_app_password("abcdefghijklmnop"), "abcdefghijklmnop");
    }

    #[test]
    fn flags_seen_detects_the_imap_seen_flag() {
        assert!(flags_seen(r#"["\\Seen"]"#));
        assert!(flags_seen(r#"["\\Flagged","\\Seen"]"#));
        assert!(!flags_seen("[]"));
        assert!(!flags_seen(r#"["\\Flagged"]"#));
        // A substring match would wrongly call this seen.
        assert!(!flags_seen(r#"["\\SeenLater"]"#));
        // Malformed flags must not panic; treat as unread.
        assert!(!flags_seen("not json"));
    }

    #[test]
    fn snippet_collapses_whitespace_and_caps_length() {
        assert_eq!(snippet(Some("  hello \n\n  world  ")).unwrap(), "hello world");
        assert_eq!(snippet(Some("   ")), None);
        assert_eq!(snippet(None), None);

        // Multibyte text must be capped by chars, not bytes, and never panic.
        let long = "á".repeat(500);
        let s = snippet(Some(&long)).unwrap();
        assert_eq!(s.chars().count(), 140);
    }

    #[test]
    fn normalize_account_cleans_all_fields() {
        let mut b = AccountCreate {
            label: "  Work  ".into(),
            email: " me@gmail.com ".into(),
            imap_host: "imap.google.com".into(),
            imap_port: 993,
            smtp_host: "smtp.google.com".into(),
            smtp_port: 587,
            username: " me@gmail.com ".into(),
            password: "abcd efgh ijkl mnop".into(),
            use_tls: true,
        };
        normalize_account(&mut b);
        assert_eq!(b.label, "Work");
        assert_eq!(b.email, "me@gmail.com");
        assert_eq!(b.imap_host, "imap.gmail.com");
        assert_eq!(b.smtp_host, "smtp.gmail.com");
        assert_eq!(b.username, "me@gmail.com");
        assert_eq!(b.password, "abcdefghijklmnop");
        assert!(validate_account(&b).is_ok());
    }
}
