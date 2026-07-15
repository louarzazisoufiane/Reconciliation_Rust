//! Embeds the built React SPA (`web/dist/`, produced by `npm run build`) into
//! the binary via `rust-embed` — the single-binary deploy goal from the
//! original server-rendered build carries over: one `recon-web` binary serves
//! both the JSON API and the UI. Unknown paths fall back to `index.html` so
//! client-side routing (react-router) works on a hard refresh / deep link.

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Assets;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    serve(path).unwrap_or_else(|| {
        serve("index.html").unwrap_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                "web UI not built — run `npm install && npm run build` in web/",
            )
                .into_response()
        })
    })
}

fn serve(path: &str) -> Option<Response> {
    let path = if path.is_empty() { "index.html" } else { path };
    let file = Assets::get(path)?;
    let mime = file.metadata.mimetype();
    Some(([(header::CONTENT_TYPE, mime)], file.data).into_response())
}
