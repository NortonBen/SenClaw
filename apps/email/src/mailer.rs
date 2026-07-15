//! Real IMAP fetch + SMTP send transport.
//!
//! Both the `imap` and `lettre` SMTP clients are blocking, so these functions
//! are synchronous and are expected to be called from `tokio::task::spawn_blocking`.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use anyhow::{anyhow, Result};
use imap::types::Flag;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};

use crate::models::AccountSecret;

/// Fail fast on a wrong/filtered host instead of blocking on the OS default
/// (~75s) TCP connect timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(12);
/// Cap on individual socket reads/writes once connected.
const IO_TIMEOUT: Duration = Duration::from_secs(45);

type ImapSession = imap::Session<native_tls::TlsStream<TcpStream>>;

/// Open an IMAP+TLS session and authenticate.
///
/// Mirrors `imap::connect` but uses `TcpStream::connect_timeout` so a misconfigured
/// host (e.g. `imap.google.com` instead of `imap.gmail.com`) surfaces a clear error
/// quickly rather than hanging the request. Shared by fetch and the verify path.
fn imap_login(acct: &AccountSecret) -> Result<ImapSession> {
    let host = acct.imap_host.trim();
    let port = acct.imap_port as u16;

    let addr = (host, port)
        .to_socket_addrs()
        .map_err(|e| anyhow!("Không phân giải được IMAP host \"{host}\": {e}"))?
        .next()
        .ok_or_else(|| anyhow!("Không tìm thấy địa chỉ cho IMAP host \"{host}\""))?;

    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)
        .map_err(|e| anyhow!("Không kết nối được tới {host}:{port}: {e}"))?;
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));

    let tls = native_tls::TlsConnector::builder().build()?;
    let tls_stream = tls
        .connect(host, stream)
        .map_err(|e| anyhow!("Bắt tay TLS với {host} thất bại: {e}"))?;

    let mut client = imap::Client::new(tls_stream);
    client
        .read_greeting()
        .map_err(|e| anyhow!("Không nhận được greeting IMAP từ {host}: {e}"))?;

    client
        .login(&acct.username, &acct.plain_password())
        .map_err(|(e, _)| anyhow!("Đăng nhập IMAP thất bại: {e}"))
}

/// Verify IMAP credentials by logging in and immediately logging out.
///
/// Used to reject broken accounts at save time and to power the "test connection"
/// button, so users never persist an account that can't actually authenticate.
pub fn verify_imap(acct: &AccountSecret) -> Result<()> {
    let mut session = imap_login(acct)?;
    let _ = session.logout();
    Ok(())
}

/// A message fetched from IMAP and parsed into Space's cache shape.
pub struct FetchedMsg {
    pub id: String,
    pub subject: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub date: Option<i64>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub flags: Vec<String>,
}

/// Fetch the most recent `limit` messages from the account's INBOX over IMAP+TLS.
pub fn fetch_imap(acct: &AccountSecret, limit: u32) -> Result<Vec<FetchedMsg>> {
    let mut session = imap_login(acct)?;

    let mailbox = session.select("INBOX")?;
    let total = mailbox.exists;
    if total == 0 {
        let _ = session.logout();
        return Ok(vec![]);
    }
    let start = if total > limit { total - limit + 1 } else { 1 };
    let seq = format!("{}:{}", start, total);

    let messages = session.fetch(seq, "(RFC822 FLAGS)")?;
    let mut out = Vec::new();
    for msg in messages.iter() {
        let body = match msg.body() {
            Some(b) => b,
            None => continue,
        };
        let parsed = match mailparse::parse_mail(body) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let header = |name: &str| {
            parsed
                .headers
                .iter()
                .find(|h| h.get_key_ref().eq_ignore_ascii_case(name))
                .map(|h| h.get_value())
        };
        let date = header("Date")
            .and_then(|d| mailparse::dateparse(&d).ok())
            .map(|secs| secs * 1000);
        let (body_text, body_html) = extract_bodies(&parsed);
        let flags: Vec<String> = msg.flags().iter().map(flag_token).collect();
        let id = header("Message-ID")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("imap-{}", uuid::Uuid::new_v4()));

        out.push(FetchedMsg {
            id,
            subject: header("Subject"),
            from: header("From"),
            to: header("To"),
            date,
            body_text,
            body_html,
            flags,
        });
    }
    let _ = session.logout();
    Ok(out)
}

/// Send a message over SMTP (STARTTLS submission when `use_tls`, plaintext otherwise).
pub fn send_smtp(
    acct: &AccountSecret,
    from_email: &str,
    to: &str,
    subject: &str,
    body: &str,
) -> Result<()> {
    let email = Message::builder()
        .from(from_email.parse().map_err(|e| anyhow!("Invalid from address: {e}"))?)
        .to(to.parse().map_err(|e| anyhow!("Invalid to address: {e}"))?)
        .subject(subject)
        .body(body.to_string())?;

    let creds = Credentials::new(acct.username.clone(), acct.plain_password());
    let builder = if acct.use_tls {
        SmtpTransport::relay(&acct.smtp_host)?
    } else {
        SmtpTransport::builder_dangerous(&acct.smtp_host)
    };
    let mailer = builder
        .port(acct.smtp_port as u16)
        .credentials(creds)
        .build();

    mailer.send(&email).map_err(|e| anyhow!("SMTP send failed: {e}"))?;
    Ok(())
}

fn flag_token(flag: &Flag) -> String {
    match flag {
        Flag::Seen => "\\Seen".to_string(),
        Flag::Answered => "\\Answered".to_string(),
        Flag::Flagged => "\\Flagged".to_string(),
        Flag::Deleted => "\\Deleted".to_string(),
        Flag::Draft => "\\Draft".to_string(),
        Flag::Recent => "\\Recent".to_string(),
        Flag::MayCreate => "\\*".to_string(),
        Flag::Custom(s) => s.to_string(),
    }
}

/// Walk the MIME tree collecting the first text/plain and text/html parts.
fn extract_bodies(part: &mailparse::ParsedMail) -> (Option<String>, Option<String>) {
    let mut text = None;
    let mut html = None;
    collect(part, &mut text, &mut html);
    (text, html)
}

fn collect(
    part: &mailparse::ParsedMail,
    text: &mut Option<String>,
    html: &mut Option<String>,
) {
    let mime = part.ctype.mimetype.to_lowercase();
    if part.subparts.is_empty() {
        if mime == "text/plain" && text.is_none() {
            *text = part.get_body().ok();
        } else if mime == "text/html" && html.is_none() {
            *html = part.get_body().ok();
        }
    } else {
        for sub in &part.subparts {
            collect(sub, text, html);
        }
    }
}
