//! API access-token auth for the daemon's own HTTP + WS surface.
//!
//! Threat model: the daemon normally binds `127.0.0.1` and trusts the local
//! machine. When the user opts into LAN exposure (`SENCLAW_UI_BIND_HOST=0.0.0.0`)
//! every `/api/*` route and the WebSocket gateway become reachable from the
//! network, so **non-loopback peers must present the API token**. Loopback
//! peers stay exempt — the bundled desktop app, Space Apps calling back into
//! the daemon, and same-machine tooling keep working with zero configuration.
//!
//! The token is resolved once at startup: `SENCLAW_API_TOKEN` env override,
//! else `~/.senclaw/api_token` (auto-generated on first use, chmod 0600).
//!
//! Accepted credential carriers, in order of preference:
//! - `Authorization: Bearer <token>`
//! - `X-SenClaw-Token: <token>`
//! - `?token=<token>` query parameter (WebSocket clients that cannot set headers)
//! - `senclaw_token` cookie — set by `POST /api/auth/login`. Required for the
//!   browser flows that cannot attach a header: Space-App proxy iframes and
//!   WS upgrades. `SameSite=Lax` keeps cross-site pages from riding it.

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use rand::RngCore;

/// Cookie set by `POST /api/auth/login`.
pub const AUTH_COOKIE: &str = "senclaw_token";

/// Paths under `/api/` that must stay reachable without a token: the login
/// handshake itself, and the probe the web/desktop gates use to decide
/// whether to even show a token prompt.
const OPEN_API_PATHS: &[&str] = &["/api/auth/login", "/api/auth/status"];

#[derive(Clone)]
pub struct ApiAuth {
    /// True when the daemon is bound to a non-loopback host — remote peers
    /// must then present [`ApiAuth::token`].
    pub required: bool,
    /// The accepted token. Always `Some` when [`ApiAuth::required`].
    pub token: Option<String>,
}

impl ApiAuth {
    /// Auth disabled — the default loopback-bind posture.
    pub fn disabled() -> Self {
        Self {
            required: false,
            token: None,
        }
    }
}

// ===== Host / origin classification =====

/// True for hosts that only ever resolve to the local machine:
/// `localhost`, `127.0.0.0/8`, `::1` (with or without brackets).
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim();
    let h = h.strip_prefix('[').unwrap_or(h);
    let h = h.strip_suffix(']').unwrap_or(h);
    if h.eq_ignore_ascii_case("localhost") {
        return true;
    }
    h.parse::<IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

/// True when an `Origin` header value (`scheme://host[:port]`) points at a
/// loopback host. Non-URL origins (`null`, garbage) are rejected.
pub fn origin_is_loopback(origin: &str) -> bool {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    let Some(rest) = rest else { return false };
    let host_port = rest.split('/').next().unwrap_or("");
    // `[::1]:5173` — the port separator is the last ':' *after* any ']'.
    let host = if let Some(end) = host_port.rfind(']') {
        &host_port[..=end]
    } else {
        host_port.split(':').next().unwrap_or("")
    };
    is_loopback_host(host)
}

// ===== Token resolution =====

/// Resolve the daemon API token: env override first, else the persisted
/// `api_token` file next to the global config (created on first use).
pub fn resolve_token(env_token: Option<&str>, senclaw_dir: &Path) -> String {
    if let Some(t) = env_token {
        let t = t.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    load_or_create_token_file(&senclaw_dir.join("api_token"))
}

fn load_or_create_token_file(path: &PathBuf) -> String {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let t = existing.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let token: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match std::fs::write(path, &token) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
        }
        Err(e) => {
            // In-memory token still protects this run; it just won't survive
            // a restart.
            tracing::warn!("[Auth] cannot persist API token at {path:?}: {e}");
        }
    }
    token
}

// ===== Credential extraction & checking =====

/// Constant-time string equality — a naive `==` short-circuits on the first
/// differing byte and leaks prefix length through timing.
fn ct_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) {
        if let Some(t) = v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")) {
            let t = t.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    if let Some(v) = headers.get("x-senclaw-token").and_then(|v| v.to_str().ok()) {
        let t = v.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    None
}

fn token_from_query(query: Option<&str>) -> Option<String> {
    for pair in query?.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some("token") {
            let raw = it.next().unwrap_or("");
            let decoded = urlencoding::decode(raw).map(|c| c.into_owned()).ok()?;
            if !decoded.is_empty() {
                return Some(decoded);
            }
        }
    }
    None
}

fn token_from_cookies(headers: &HeaderMap) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        let Ok(s) = value.to_str() else { continue };
        for part in s.split(';') {
            let mut it = part.trim().splitn(2, '=');
            if it.next() == Some(AUTH_COOKIE) {
                let v = it.next().unwrap_or("").trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Whether this request may pass. Loopback peers are always trusted; remote
/// peers must carry the token in a header, the query string, or the cookie.
/// `peer == None` (no `ConnectInfo`, e.g. unit tests without a real socket)
/// is treated as remote — fail closed.
pub fn authorize(auth: &ApiAuth, peer: Option<SocketAddr>, req: &Request) -> bool {
    if !auth.required {
        return true;
    }
    if let Some(p) = peer {
        if p.ip().is_loopback() {
            return true;
        }
    }
    let Some(expected) = auth.token.as_deref() else {
        return false;
    };
    let given = token_from_headers(req.headers())
        .or_else(|| token_from_query(req.uri().query()))
        .or_else(|| token_from_cookies(req.headers()));
    match given {
        Some(t) => ct_eq(&t, expected),
        None => false,
    }
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({
            "error": "unauthorized",
            "authRequired": true,
        })),
    )
        .into_response()
}

// ===== Middleware =====

/// UI server (18788): gate `/api/*` except the login/status handshake.
/// Static assets and the SPA shell stay open — the remote user must be able
/// to load the page that asks for the token.
pub async fn http_auth_mw(
    State(auth): State<Arc<ApiAuth>>,
    peer: Option<ConnectInfo<SocketAddr>>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path();
    let protected = path.starts_with("/api/") && !OPEN_API_PATHS.contains(&path);
    if protected && !authorize(&auth, peer.map(|c| c.0), &req) {
        return unauthorized();
    }
    next.run(req).await
}

/// WS gateway (18789): gate **every** path at upgrade time. The in-band
/// `connect` message is not a real gate — the dispatcher runs handlers for
/// unauthenticated sockets — so the HTTP upgrade is where remote peers are
/// stopped. Browsers authenticate via the `senclaw_token` cookie (cookies are
/// port-agnostic, so the login on 18788 covers 18789 on the same host);
/// native clients use `?token=` or headers.
pub async fn ws_auth_mw(
    State(auth): State<Arc<ApiAuth>>,
    peer: Option<ConnectInfo<SocketAddr>>,
    req: Request,
    next: Next,
) -> Response {
    if !authorize(&auth, peer.map(|c| c.0), &req) {
        return unauthorized();
    }
    next.run(req).await
}

// ===== /api/auth/* handlers =====

#[derive(serde::Deserialize)]
pub struct LoginBody {
    pub token: String,
}

/// `POST /api/auth/login {token}` — verify the token and mint the browser
/// session cookie. Open (unauthenticated) by design; it *is* the login.
pub async fn auth_login(
    State(auth): State<Arc<ApiAuth>>,
    Json(body): Json<LoginBody>,
) -> Response {
    if !auth.required {
        return Json(serde_json::json!({ "ok": true, "authRequired": false })).into_response();
    }
    let ok = auth
        .token
        .as_deref()
        .map(|t| ct_eq(body.token.trim(), t))
        .unwrap_or(false);
    if !ok {
        return unauthorized();
    }
    // No `Secure` attribute: the LAN deployment this protects is plain HTTP.
    // HttpOnly keeps page JS away from it; SameSite=Lax blocks cross-site use.
    let cookie = format!(
        "{AUTH_COOKIE}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age=2592000",
        body.token.trim()
    );
    (
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({ "ok": true, "authRequired": true })),
    )
        .into_response()
}

/// `GET /api/auth/status` — lets a client decide whether to prompt for a
/// token before touching any gated endpoint. Open by design; leaks only the
/// two booleans.
pub async fn auth_status(
    State(auth): State<Arc<ApiAuth>>,
    peer: Option<ConnectInfo<SocketAddr>>,
    req: Request,
) -> Json<serde_json::Value> {
    let authorized = authorize(&auth, peer.map(|c| c.0), &req);
    Json(serde_json::json!({
        "authRequired": auth.required,
        "authorized": authorized,
    }))
}

// ===== CORS =====

/// Replacement for the old `CorsLayer::permissive()` (ACAO `*`), which let
/// any web page the user visited read API responses off the loopback daemon —
/// including cleartext provider keys from `/api/llm-config`. Only loopback
/// origins (the Vite dev server, local tooling) may now read cross-origin
/// responses; the served UI itself is same-origin and needs no CORS at all.
pub fn restrictive_cors() -> tower_http::cors::CorsLayer {
    use axum::http::HeaderName;
    tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::AllowOrigin::predicate(|origin, _| {
            origin.to_str().map(origin_is_loopback).unwrap_or(false)
        }))
        .allow_methods(tower_http::cors::Any)
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-senclaw-token"),
        ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;

    fn req(uri: &str) -> Request {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    fn req_with_header(uri: &str, name: &str, value: &str) -> Request {
        Request::builder()
            .uri(uri)
            .header(name, value)
            .body(Body::empty())
            .unwrap()
    }

    fn auth_on(token: &str) -> ApiAuth {
        ApiAuth {
            required: true,
            token: Some(token.to_string()),
        }
    }

    fn remote_peer() -> Option<SocketAddr> {
        Some("192.168.1.50:44444".parse().unwrap())
    }

    fn local_peer() -> Option<SocketAddr> {
        Some("127.0.0.1:55555".parse().unwrap())
    }

    #[test]
    fn loopback_hosts() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.5.0.3"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("[::1]"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("example.com"));
        assert!(!is_loopback_host(""));
    }

    #[test]
    fn loopback_origins() {
        assert!(origin_is_loopback("http://127.0.0.1:5173"));
        assert!(origin_is_loopback("http://localhost:18788"));
        assert!(origin_is_loopback("http://localhost"));
        assert!(origin_is_loopback("https://[::1]:8443"));
        assert!(!origin_is_loopback("http://192.168.1.7:5173"));
        assert!(!origin_is_loopback("https://evil.example"));
        assert!(!origin_is_loopback("null"));
        assert!(!origin_is_loopback("file://x"));
        // Loopback host as a *subdomain* of a public domain must not pass.
        assert!(!origin_is_loopback("http://localhost.evil.example"));
    }

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq("abc", "abc"));
        assert!(!ct_eq("abc", "abd"));
        assert!(!ct_eq("abc", "ab"));
        assert!(!ct_eq("", "a"));
        assert!(ct_eq("", ""));
    }

    #[test]
    fn disabled_auth_allows_everything() {
        let auth = ApiAuth::disabled();
        assert!(authorize(&auth, remote_peer(), &req("/api/llm-config")));
        assert!(authorize(&auth, None, &req("/api/llm-config")));
    }

    #[test]
    fn loopback_peer_is_exempt() {
        let auth = auth_on("secret");
        assert!(authorize(&auth, local_peer(), &req("/api/llm-config")));
    }

    #[test]
    fn remote_peer_needs_token() {
        let auth = auth_on("secret");
        assert!(!authorize(&auth, remote_peer(), &req("/api/llm-config")));
        // Missing ConnectInfo fails closed.
        assert!(!authorize(&auth, None, &req("/api/llm-config")));
    }

    #[test]
    fn bearer_header_accepted() {
        let auth = auth_on("secret");
        let r = req_with_header("/api/x", "authorization", "Bearer secret");
        assert!(authorize(&auth, remote_peer(), &r));
        let r = req_with_header("/api/x", "authorization", "Bearer wrong");
        assert!(!authorize(&auth, remote_peer(), &r));
    }

    #[test]
    fn custom_header_accepted() {
        let auth = auth_on("secret");
        let r = req_with_header("/api/x", "x-senclaw-token", "secret");
        assert!(authorize(&auth, remote_peer(), &r));
    }

    #[test]
    fn query_token_accepted() {
        let auth = auth_on("se cret");
        let r = req("/api/ws/terminal?cwd=%2Ftmp&token=se%20cret");
        assert!(authorize(&auth, remote_peer(), &r));
        let r = req("/api/ws/terminal?token=wrong");
        assert!(!authorize(&auth, remote_peer(), &r));
    }

    #[test]
    fn cookie_accepted() {
        let auth = auth_on("secret");
        let r = req_with_header("/api/x", "cookie", "theme=dark; senclaw_token=secret");
        assert!(authorize(&auth, remote_peer(), &r));
        let r = req_with_header("/api/x", "cookie", "senclaw_token=wrong");
        assert!(!authorize(&auth, remote_peer(), &r));
    }

    #[test]
    fn token_file_roundtrip_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let t1 = resolve_token(None, dir.path());
        assert_eq!(t1.len(), 64, "32 random bytes hex-encoded");
        // Second resolve reuses the persisted token.
        let t2 = resolve_token(None, dir.path());
        assert_eq!(t1, t2);
        // Env override wins and does not touch the file.
        let t3 = resolve_token(Some("envtok"), dir.path());
        assert_eq!(t3, "envtok");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.path().join("api_token"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn http_middleware_gates_api_only() {
        use axum::{middleware::from_fn_with_state, routing::get, Router};
        use tower::ServiceExt;

        let auth = Arc::new(auth_on("secret"));
        let app = Router::new()
            .route("/api/data", get(|| async { "data" }))
            .route("/api/auth/status", get(|| async { "status" }))
            .route("/", get(|| async { "shell" }))
            .layer(from_fn_with_state(auth, http_auth_mw));

        // No ConnectInfo in oneshot ⇒ treated as remote.
        let res = app.clone().oneshot(req("/api/data")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let res = app
            .clone()
            .oneshot(req_with_header("/api/data", "x-senclaw-token", "secret"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);

        // Handshake endpoints and the SPA shell stay open.
        let res = app.clone().oneshot(req("/api/auth/status")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let res = app.clone().oneshot(req("/")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn ws_middleware_gates_every_path() {
        use axum::{middleware::from_fn_with_state, routing::get, Router};
        use tower::ServiceExt;

        let auth = Arc::new(auth_on("secret"));
        let app = Router::new()
            .route("/", get(|| async { "ws" }))
            .route("/browser", get(|| async { "ext" }))
            .layer(from_fn_with_state(auth, ws_auth_mw));

        let res = app.clone().oneshot(req("/")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = app.clone().oneshot(req("/browser")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let res = app
            .clone()
            .oneshot(req("/?token=secret"))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
    }

    /// Real listener + real client socket: proves the `ConnectInfo` plumbing
    /// (`into_make_service_with_connect_info`) actually delivers the peer
    /// address, so a loopback client is exempt even with auth required.
    #[tokio::test]
    async fn real_socket_loopback_exempt() {
        use axum::{middleware::from_fn_with_state, routing::get, Router};

        let auth = Arc::new(auth_on("secret"));
        let app = Router::new()
            .route("/api/data", get(|| async { "d" }))
            .layer(from_fn_with_state(auth, http_auth_mw));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let res = reqwest::get(format!("http://{addr}/api/data")).await.unwrap();
        assert_eq!(res.status(), reqwest::StatusCode::OK, "loopback peer must not need a token");
    }

    #[tokio::test]
    async fn login_mints_cookie() {
        use axum::{routing::post, Router};
        use tower::ServiceExt;

        let auth = Arc::new(auth_on("secret"));
        let app = Router::new()
            .route("/api/auth/login", post(auth_login))
            .with_state(auth);

        let body = |tok: &str| {
            Request::builder()
                .method("POST")
                .uri("/api/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(format!("{{\"token\":\"{tok}\"}}")))
                .unwrap()
        };

        let res = app.clone().oneshot(body("secret")).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let cookie = res.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap();
        assert!(cookie.starts_with("senclaw_token=secret"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Lax"));

        let res = app.clone().oneshot(body("wrong")).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert!(res.headers().get(header::SET_COOKIE).is_none());
    }
}
