//! Makes sure the directory the web UI is embedded from exists, so the Rust build never
//! depends on Node. A real UI build (`make ui`) replaces the placeholder.

use std::path::Path;

const PLACEHOLDER: &str = r#"<!doctype html>
<html lang="en">
  <head><meta charset="utf-8"><title>ramzes</title></head>
  <body>
    <h1>ramzes</h1>
    <p>This binary was built without the web UI. Run <code>make ui</code> and rebuild.</p>
    <p>The API is available: see <a href="/api/openapi.json">/api/openapi.json</a>.</p>
  </body>
</html>
"#;

fn main() {
    let build = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/build");
    println!("cargo:rerun-if-changed={}", build.display());
    if !build.join("200.html").exists() {
        std::fs::create_dir_all(&build).expect("could not create ui/build");
        std::fs::write(build.join("200.html"), PLACEHOLDER)
            .expect("could not write the UI placeholder");
    }
}
