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
        let acct = match account_id {
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
        Ok(acct)
    })
}

pub fn create_account(db: &Db, b: &AccountCreate) -> Result<Account> {
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

pub fn inbox(db: &Db, account_id: Option<&str>, limit: u32) -> Result<Vec<Value>> {
    db.with_conn(|conn| {
        let (sql, has_acct) = match account_id {
            Some(_) => ("SELECT id, account_id, subject, from_addr, date, flags FROM space_email_cache WHERE account_id=?1 AND folder='INBOX' ORDER BY date DESC LIMIT ?2", true),
            None => ("SELECT id, account_id, subject, from_addr, date, flags FROM space_email_cache WHERE folder='INBOX' ORDER BY date DESC LIMIT ?1", false),
        };
        let mut stmt = conn.prepare(sql)?;
        let map = |row: &rusqlite::Row| {
            Ok(json!({
                "id": row.get::<_,String>(0)?,
                "account_id": row.get::<_,String>(1)?,
                "subject": row.get::<_,Option<String>>(2)?,
                "from": row.get::<_,Option<String>>(3)?,
                "date": row.get::<_,Option<i64>>(4)?,
                "flags": row.get::<_,String>(5)?,
            }))
        };
        let rows: Vec<Value> = if has_acct {
            stmt.query_map(params![account_id.unwrap(), limit], map)?.filter_map(|r| r.ok()).collect()
        } else {
            stmt.query_map(params![limit], map)?.filter_map(|r| r.ok()).collect()
        };
        Ok(rows)
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

pub fn search(db: &Db, query: &str, account_id: Option<&str>, limit: u32) -> Result<Vec<Value>> {
    db.with_conn(|conn| {
        let pattern = format!("%{query}%");
        let map = |row: &rusqlite::Row| {
            Ok(json!({
                "id": row.get::<_,String>(0)?,
                "account_id": row.get::<_,String>(1)?,
                "subject": row.get::<_,Option<String>>(2)?,
                "from": row.get::<_,Option<String>>(3)?,
                "date": row.get::<_,Option<i64>>(4)?,
            }))
        };
        let rows: Vec<Value> = match account_id {
            Some(aid) => {
                let mut stmt = conn.prepare("SELECT id, account_id, subject, from_addr, date FROM space_email_cache WHERE account_id=?1 AND (subject LIKE ?3 OR body_text LIKE ?3) ORDER BY date DESC LIMIT ?2")?;
                stmt.query_map(params![aid, limit, pattern], map)?.filter_map(|r| r.ok()).collect()
            }
            None => {
                let mut stmt = conn.prepare("SELECT id, account_id, subject, from_addr, date FROM space_email_cache WHERE (subject LIKE ?2 OR body_text LIKE ?2) ORDER BY date DESC LIMIT ?1")?;
                stmt.query_map(params![limit, pattern], map)?.filter_map(|r| r.ok()).collect()
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
