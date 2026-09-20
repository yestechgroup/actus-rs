//! Integration test: builds the WebUI protocol from the crate's own assets
//! (pure Rust, no node required) and asserts the SSR document of the axum
//! router carries the explorer shell, the PAM seed schedule and the
//! applicability matrix.

use std::path::PathBuf;
use std::sync::OnceLock;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;
use webui::{BuildOptions, Plugin, Protocol};

struct TestApp {
    router: axum::Router,
}

static APP: OnceLock<TestApp> = OnceLock::new();

fn app() -> &'static TestApp {
    APP.get_or_init(|| {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let app_dir = manifest_dir.join("assets/src");
        let result = webui::build(BuildOptions {
            app_dir,
            entry: "index.html".to_string(),
            css: webui::CssStrategy::Link,
            dom: webui::DomStrategy::Light,
            plugin: Some(Plugin::WebUI),
            ..BuildOptions::default()
        })
        .expect("WebUI protocol build");
        let protocol = Protocol::from_protobuf(&result.protocol_bytes).expect("protocol parse");
        let app = std::sync::Arc::new(actus_web::App::from_parts(
            protocol,
            PathBuf::from("nonexistent-dist"),
            PathBuf::from("nonexistent-pkg"),
        ));
        TestApp {
            router: actus_web::router(app),
        }
    })
}

async fn get(path: &str) -> (StatusCode, String) {
    let response = app()
        .router
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .expect("response");
    let status = response.status();
    let body = BodyExt::collect(response.into_body())
        .await
        .expect("body")
        .to_bytes();
    (status, String::from_utf8_lossy(&body).to_string())
}

#[tokio::test]
async fn document_renders_explorer_markers() {
    let (status, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("ACTUS Explorer"), "title missing");
    assert!(body.contains("app-shell"), "root component missing");
    assert!(body.contains("PAM"), "selected contract missing");
    assert!(
        body.contains("Algorithmic Contract Types"),
        "tagline missing"
    );
    // The hydration plugin must emit the client bootstrap (state, template
    // payloads and inventory) or nothing hydrates in the browser.
    assert!(body.contains("webui-data"), "hydration bootstrap missing");
    assert!(body.contains("\"selected\""), "seed state missing");
}

#[tokio::test]
async fn document_contains_seed_schedule() {
    let (status, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    // The PAM default schedule renders the swim-lane timeline (lane tracks
    // with precomputed left percentages) and the event table with IED/IP/MD
    // badges.
    assert!(
        body.contains("event-pin"),
        "timeline markers missing: {body}"
    );
    assert!(body.contains("lane-track"), "timeline lanes missing");
    assert!(body.contains("lane-label"), "lane labels missing");
    assert!(body.contains("data-kind=\"ied\""), "IED markers missing");
    assert!(body.contains("data-kind=\"md\""), "MD markers missing");
    assert!(body.contains("Event Timeline"), "timeline panel missing");
    assert!(
        body.contains("Generate Schedule"),
        "generate button missing"
    );
}

#[tokio::test]
async fn document_contains_applicability_matrix() {
    let (status, body) = get("/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Applicability Matrix"), "matrix missing");
    assert!(body.contains("matrix-cell"), "matrix cells missing");
    assert!(body.contains("notionalPrincipal"), "dictionary ids missing");
}

#[tokio::test]
async fn every_path_renders_the_document() {
    for path in ["/explorer", "/contracts/PAM"] {
        let (status, body) = get(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(body.contains("ACTUS Explorer"), "{path}");
    }
}
