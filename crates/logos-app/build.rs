fn main() {
    // The front end is built by `npm run build` in app/ (see app/README.md) and embedded from
    // app/dist. When it hasn't been built, a page saying so is put there, so that
    // `cargo build --workspace` and `cargo test --workspace` still work without Node.
    let dist = std::path::Path::new("../../app/dist");
    println!("cargo:rerun-if-changed=../../app/dist");
    if !dist.join("index.html").is_file() {
        println!(
            "cargo:warning=app/dist is missing; the app will show a page asking to build the front end (cd app; npm ci; npm run build)"
        );
        std::fs::create_dir_all(dist).expect("create app/dist");
        std::fs::write(
            dist.join("index.html"),
            "<!doctype html><meta charset=utf-8><title>Logos</title>\
             <p style=\"font:14px sans-serif;margin:40px\">The Logos front end hasn't been built. \
             In a terminal: <code>cd app; npm ci; npm run build</code>, then build the app again.</p>",
        )
        .expect("write app/dist/index.html");
    }
    tauri_build::build()
}
