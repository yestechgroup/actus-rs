//! ACTUS Explorer server: an axum app rendering the WebUI protocol with
//! server-side state (contract catalogue, applicability matrix, the
//! selected contract's evaluated schedule) and serving the built client
//! assets plus the `actus-wasm` browser bindings.
//!
//! Run with `cargo run -p actus-web` after `cargo run -p actus-web --bin
//! build-site` and `wasm-pack build crates/actus-wasm --target web
//! --out-dir pkg` (see the crate README).

pub mod state;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::response::Response;
use axum::routing::get;
use axum::Router;
use serde_json::Value;
use webui::{Protocol, RenderOptions, ResponseWriter, WebUIHandler};

/// Default bind address of the explorer server (override with
/// `$ACTUS_WEB_ADDR`).
pub const BIND_ADDR: &str = "127.0.0.1:8080";

/// The compiled protocol plus the static asset directories.
pub struct App {
    protocol: Protocol,
    dist_dir: PathBuf,
    wasm_pkg_dir: PathBuf,
}

impl App {
    /// Assembles the app from a parsed protocol and asset directories
    /// (test seam and loader backend).
    #[must_use]
    pub fn from_parts(protocol: Protocol, dist_dir: PathBuf, wasm_pkg_dir: PathBuf) -> Self {
        Self {
            protocol,
            dist_dir,
            wasm_pkg_dir,
        }
    }

    /// Loads `dist/protocol.bin` (or `$ACTUS_WEB_DIST/protocol.bin`) and
    /// points the static handlers at `dist/` and `crates/actus-wasm/pkg/`.
    ///
    /// # Errors
    /// The protocol file is missing or not a valid WebUI protocol.
    pub fn load() -> Result<Self, String> {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let workspace_root = manifest_dir
            .parent()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(|| manifest_dir.clone());
        let dist_dir = std::env::var("ACTUS_WEB_DIST")
            .map(PathBuf::from)
            .unwrap_or_else(|_| manifest_dir.join("dist"));
        let wasm_pkg_dir = std::env::var("ACTUS_WASM_PKG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| workspace_root.join("crates/actus-wasm/pkg"));
        let protocol_path = dist_dir.join("protocol.bin");
        let protocol_bytes = std::fs::read(&protocol_path).map_err(|e| {
            format!(
                "cannot read {}: {e}. Build the site first: `cargo run -p actus-web --bin build-site`",
                protocol_path.display()
            )
        })?;
        let protocol = Protocol::from_protobuf(&protocol_bytes)
            .map_err(|e| format!("invalid WebUI protocol in {}: {e}", protocol_path.display()))?;
        Ok(Self::from_parts(protocol, dist_dir, wasm_pkg_dir))
    }
}

/// String collector implementing the WebUI [`ResponseWriter`] contract.
#[derive(Default)]
pub struct StringWriter(String);

impl ResponseWriter for StringWriter {
    fn write(&mut self, content: &str) -> webui::HandlerResult<()> {
        self.0.push_str(content);
        Ok(())
    }

    fn end(&mut self) -> webui::HandlerResult<()> {
        Ok(())
    }
}

/// Renders the explorer document for one request path.
pub fn render_document(app: &App, request_path: &str) -> Result<String, String> {
    let state = state::build_state("PAM");
    render_document_with_state(app, request_path, &state)
}

/// Renders the explorer document with an explicit SSR state (test seam).
pub fn render_document_with_state(
    app: &App,
    request_path: &str,
    state: &Value,
) -> Result<String, String> {
    let options = RenderOptions::new("index.html", request_path);
    let mut writer = StringWriter::default();
    // The hydration plugin emits the `#webui-data` bootstrap block (state,
    // template payloads, inventory) the client framework needs; without it
    // SSR renders markup only and nothing hydrates.
    let handler = WebUIHandler::with_plugin(|| {
        Box::new(webui_handler::plugin::webui::WebUIHydrationPlugin::new())
    });
    handler
        .render(&app.protocol, state, &options, &mut writer)
        .map_err(|e| format!("WebUI render failed: {e}"))?;
    Ok(writer.0)
}

/// Resolves a request path (relative, without URL prefix) to a static file
/// inside `base`, rejecting path traversal.
fn static_file(base: &Path, request_path: &str) -> Option<(PathBuf, &'static str)> {
    let relative = request_path.trim_start_matches('/');
    if relative.is_empty() || relative.contains("..") {
        return None;
    }
    let path = base.join(relative);
    let mime = match path.extension()?.to_str()? {
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "html" => "text/html; charset=utf-8",
        "json" => "application/json",
        "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        "md" => "text/markdown; charset=utf-8",
        "ts" => "text/plain; charset=utf-8",
        "d.ts" => "text/plain; charset=utf-8",
        "bin" => "application/octet-stream",
        _ => return None,
    };
    std::fs::metadata(&path)
        .ok()?
        .is_file()
        .then_some((path, mime))
}

fn file_response(path: &Path, mime: &str) -> Response {
    match std::fs::read(path) {
        Ok(bytes) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .header(header::CACHE_CONTROL, "no-cache")
            .body(axum::body::Body::from(bytes))
            .unwrap_or_else(|_| internal_error()),
        Err(_) => internal_error(),
    }
}

fn internal_error() -> Response {
    Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(axum::body::Body::from("internal error"))
        .expect("static error response")
}

async fn handle(State(app): State<Arc<App>>, request: Request) -> Response {
    let path = request.uri().path().to_string();
    let is_root = path == "/";
    let has_asset_extension = Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some();

    // Static assets: built client files and the WASM bindings package
    // (URL prefix /pkg/ maps into the wasm-pack output directory).
    if !is_root && has_asset_extension {
        if let Some((file, mime)) = static_file(&app.dist_dir, &path) {
            return file_response(&file, mime);
        }
        if let Some(stripped) = path.strip_prefix("/pkg/") {
            if let Some((file, mime)) = static_file(&app.wasm_pkg_dir, stripped) {
                return file_response(&file, mime);
            }
        }
        if let Some((file, mime)) = static_file(&app.wasm_pkg_dir, &path) {
            return file_response(&file, mime);
        }
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(axum::body::Body::from("not found"))
            .expect("static 404 response");
    }

    // Everything else renders the explorer document (SSR state included).
    match render_document(&app, &path) {
        Ok(html) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(axum::body::Body::from(html))
            .expect("html response"),
        Err(e) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(axum::body::Body::from(format!("render error: {e}")))
            .expect("render error response"),
    }
}

/// The axum router of the explorer.
pub fn router(app: Arc<App>) -> Router {
    Router::new()
        .route("/", get(handle))
        .fallback(handle)
        .with_state(app)
}

/// Loads the app and serves it on [`BIND_ADDR`] (or `$ACTUS_WEB_ADDR`).
///
/// # Errors
/// Binding the listener fails.
pub async fn serve() -> std::io::Result<()> {
    let app = Arc::new(App::load().unwrap_or_else(|e| panic!("{e}")));
    let addr = std::env::var("ACTUS_WEB_ADDR").unwrap_or_else(|_| BIND_ADDR.to_string());
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("ACTUS Explorer serving on http://{addr}");
    axum::serve(listener, router(app)).await
}
