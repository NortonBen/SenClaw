use std::fs;
use std::path::PathBuf;
use axum::{
    body::Body,
    http::{header, HeaderMap, StatusCode},
    response::Response,
};

use super::types::path_to_mime;

pub(crate) async fn spa_fallback(dist_dir: PathBuf, headers: HeaderMap) -> Response {
    let path = headers
        .get("x-original-uri")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("/");

    let is_wiki = path == "/wiki" || path.starts_with("/wiki/");
    let is_plugins = path == "/plugins" || path.starts_with("/plugins/");

    let fallback = if is_wiki {
        "wiki.html"
    } else if is_plugins {
        "plugins.html"
    } else {
        "index.html"
    };

    let file = dist_dir.join(fallback);
    match fs::read(&file) {
        Ok(contents) => {
            let mime = path_to_mime(fallback);
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .body(Body::from(contents))
                .unwrap()
        }
        Err(_) => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Body::from("Web UI not built. Run: npm run build:web"))
            .unwrap(),
    }
}
