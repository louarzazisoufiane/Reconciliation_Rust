//! Ensures `web/dist/` exists before `rust-embed` scans it at compile time,
//! so a fresh clone (before `npm run build` has ever run) still compiles — it
//! serves a placeholder page until the real SPA build lands.

use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    if !dist.join("index.html").exists() {
        std::fs::create_dir_all(&dist).expect("create web/dist placeholder dir");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><title>Recon</title><body style=\"font-family:sans-serif\">\
             Web UI not built yet — run <code>npm install &amp;&amp; npm run build</code> \
             in <code>web/</code>, then rebuild <code>recon-web</code>.</body>",
        )
        .expect("write placeholder index.html");
    }
}
