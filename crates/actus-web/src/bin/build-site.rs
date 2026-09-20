//! Site builder for the ACTUS Explorer: compiles the WebUI templates in
//! `assets/src` into `dist/` (protocol.bin + CSS) via the `microsoft-webui`
//! build API, then bundles the client TypeScript with esbuild (from the
//! `assets` npm project) into `dist/index.js`.
//!
//! Prerequisites (run once): `cd crates/actus-web/assets && npm install`.
//! The WebUI template build itself is pure Rust and needs no node.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let assets_dir = manifest_dir.join("assets");
    let app_dir = assets_dir.join("src");
    let dist_dir = manifest_dir.join("dist");

    println!("building WebUI protocol from {}", app_dir.display());
    let result = webui::build(webui::BuildOptions {
        app_dir,
        entry: "index.html".to_string(),
        css: webui::CssStrategy::Link,
        dom: webui::DomStrategy::Light,
        plugin: Some(webui::Plugin::WebUI),
        // Content-hashed CSS filenames so browsers never serve a stale
        // stylesheet after an upgrade.
        css_file_name_template: "[name].[hash].[ext]".to_string(),
        ..webui::BuildOptions::default()
    })
    .unwrap_or_else(|e| panic!("WebUI build failed: {e}"));

    std::fs::create_dir_all(&dist_dir).expect("create dist dir");
    std::fs::write(dist_dir.join("protocol.bin"), &result.protocol_bytes)
        .expect("write protocol.bin");
    for (name, content) in &result.css_files {
        std::fs::write(dist_dir.join(name), content).expect("write css file");
    }
    // Drop the unhashed legacy stylesheet and any stale hashed ones so a
    // cached copy can never be served next to the current build.
    let _ = std::fs::remove_file(dist_dir.join("app-shell.css"));
    if let Ok(entries) = std::fs::read_dir(&dist_dir) {
        let current: std::collections::HashSet<String> = result
            .css_files
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("app-shell") && name.ends_with(".css") && !current.contains(&name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    println!(
        "protocol.bin: {} bytes, {} components, {} css files, {:?}",
        result.stats.protocol_size_bytes,
        result.stats.component_count,
        result.stats.css_file_count,
        result.stats.duration
    );
    for warning in &result.warnings {
        println!("build warning: {warning}");
    }

    bundle_typescript(&assets_dir, &dist_dir);
    println!("site built into {}", dist_dir.display());
}

/// Bundles `assets/src/index.ts` into `dist/index.js` with esbuild. The
/// `/pkg/*` WASM glue imports are kept external: the server serves them
/// from `crates/actus-wasm/pkg` at runtime.
fn bundle_typescript(assets_dir: &Path, dist_dir: &Path) {
    let esbuild = assets_dir.join("node_modules/.bin/esbuild");
    if !esbuild.exists() {
        panic!(
            "esbuild not found at {}. Run `cd {} && npm install` first.",
            esbuild.display(),
            assets_dir.display()
        );
    }
    let entry = assets_dir.join("src/index.ts");
    let outfile = dist_dir.join("index.js");
    let status = Command::new(esbuild)
        .arg(entry)
        .arg("--bundle")
        .arg("--format=esm")
        .arg("--target=es2022")
        .arg("--minify")
        .arg(format!("--outfile={}", outfile.display()))
        .arg("--external:/pkg/*")
        .arg("--log-level=warning")
        .status()
        .expect("spawn esbuild");
    if !status.success() {
        panic!("esbuild failed with {status}");
    }
}
