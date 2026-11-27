//! Static asset serving: content types, the `/pkg/` mapping onto the
//! wasm-pack output, 404s, and the path traversal guard. Uses real files in
//! a per-process temporary directory.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use webui::{BuildOptions, Plugin, Protocol};

struct Fixture {
    router: axum::Router,
    root: PathBuf,
}

static FIXTURE: OnceLock<Fixture> = OnceLock::new();

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, contents).expect("write fixture file");
}

fn fixture() -> &'static Fixture {
    FIXTURE.get_or_init(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let result = webui::build(BuildOptions {
            app_dir: manifest_dir.join("assets/src"),
            entry: "index.html".to_string(),
            css: webui::CssStrategy::Link,
            dom: webui::DomStrategy::Light,
            plugin: Some(Plugin::WebUI),
            ..BuildOptions::default()
        })
        .expect("WebUI protocol build");
        let protocol = Protocol::from_protobuf(&result.protocol_bytes).expect("protocol parse");

        let root = std::env::temp_dir().join(format!("actus-web-static-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dist = root.join("dist");
        let pkg = root.join("pkg");
        write(&dist.join("index.js"), "export const ok = 1;");
        write(&dist.join("app-shell.css"), "body { margin: 0 }");
        write(&dist.join("nested/data.json"), "{}");
        write(&dist.join("notes.unknownext"), "not served");
        write(&pkg.join("actus_wasm_bg.wasm"), "\0asm");
        write(&pkg.join("actus_wasm.js"), "export default 1;");
        // A file outside both asset roots that traversal must not reach.
        write(&root.join("secret.txt"), "top secret");

        let app = Arc::new(actus_web::App::from_parts(protocol, dist, pkg));
        Fixture {
            router: actus_web::router(app),
            root,
        }
    })
}

async fn get(path: &str) -> (StatusCode, Option<String>, String) {
    let response = fixture()
        .router
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .expect("response");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|value| value.to_str().unwrap().to_string());
    let body = BodyExt::collect(response.into_body())
        .await
        .expect("body")
        .to_bytes();
    (
        status,
        content_type,
        String::from_utf8_lossy(&body).to_string(),
    )
}

#[tokio::test]
async fn built_client_files_are_served_with_their_content_types() {
    for (path, mime, body) in [
        (
            "/index.js",
            "text/javascript; charset=utf-8",
            "export const ok = 1;",
        ),
        (
            "/app-shell.css",
            "text/css; charset=utf-8",
            "body { margin: 0 }",
        ),
        ("/nested/data.json", "application/json", "{}"),
    ] {
        let (status, content_type, served) = get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert_eq!(content_type.as_deref(), Some(mime), "{path}");
        assert_eq!(served, body, "{path}");
    }
}

#[tokio::test]
async fn pkg_prefix_maps_onto_the_wasm_pack_output() {
    let (status, content_type, _) = get("/pkg/actus_wasm_bg.wasm").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("application/wasm"));
    let (status, _, body) = get("/pkg/actus_wasm.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "export default 1;");
    // The pkg directory is also reachable without the prefix (wasm-bindgen
    // glue resolves its .wasm relative to the importing module).
    let (status, _, _) = get("/actus_wasm_bg.wasm").await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn missing_and_unknown_assets_are_404() {
    for path in ["/missing.js", "/pkg/missing.wasm", "/notes.unknownext"] {
        let (status, _, body) = get(path).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(body, "not found", "{path}");
    }
}

#[tokio::test]
async fn path_traversal_cannot_escape_the_asset_roots() {
    assert!(fixture().root.join("secret.txt").is_file());
    for path in [
        "/../secret.txt",
        "/pkg/../secret.txt",
        "/nested/../../secret.txt",
        "/%2e%2e/secret.txt",
        "//secret.txt",
    ] {
        let (status, _, body) = get(path).await;
        assert_ne!(status, StatusCode::OK, "{path} must not be served");
        assert!(!body.contains("top secret"), "{path} leaked the file");
    }
}

#[tokio::test]
async fn extensionless_paths_render_the_document_not_a_file() {
    let (status, content_type, body) = get("/nested").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type.as_deref(), Some("text/html; charset=utf-8"));
    assert!(body.contains("ACTUS Explorer"));
}

#[test]
fn load_reports_a_missing_protocol_with_a_build_hint() {
    // `App::load` reads `$ACTUS_WEB_DIST/protocol.bin`; point it at an
    // empty directory. This is the only test in this binary touching the
    // variable.
    let empty = std::env::temp_dir().join(format!("actus-web-empty-{}", std::process::id()));
    fs::create_dir_all(&empty).expect("mkdir");
    std::env::set_var("ACTUS_WEB_DIST", &empty);
    let err = actus_web::App::load().err().expect("load must fail");
    std::env::remove_var("ACTUS_WEB_DIST");
    assert!(err.contains("protocol.bin"), "{err}");
    assert!(err.contains("build-site"), "{err}");

    fs::write(empty.join("protocol.bin"), b"definitely not protobuf").expect("write");
    std::env::set_var("ACTUS_WEB_DIST", &empty);
    let err = actus_web::App::load().err().expect("load must fail");
    std::env::remove_var("ACTUS_WEB_DIST");
    assert!(err.contains("invalid WebUI protocol"), "{err}");
}
