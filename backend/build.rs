//! Embeds the web client (`web/dist`, built by `just web`) into the `janus` binary, so the service
//! answers its pages with nothing beside it (docs/architecture/20261126-web.md#serving). Without a
//! build there is nothing to embed: the service starts all the same and its pages say how to build
//! the client, so `cargo test` needs no Node.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let web = manifest.join("../web");
    let dist = web.join("dist");
    let mut files = vec![];
    if dist.join("index.html").is_file() {
        collect(&dist, &dist, &mut files);
        println!("cargo:rerun-if-changed={}", dist.display());
    } else {
        // A build appearing later lands under web/: watched until there is one (a path that does
        // not exist would run this script, and compile the crate, every time).
        println!("cargo:rerun-if-changed={}", web.display());
    }
    files.sort();
    let mut out = String::from(
        "/// The web client's files, by their path under `web/dist`.\npub static FILES: &[(&str, &[u8])] = &[\n",
    );
    for (name, path) in &files {
        writeln!(out, "    ({name:?}, include_bytes!({:?})),", path.display().to_string()).unwrap();
    }
    out.push_str("];\n");
    let dest = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("web_assets.rs");
    // Written only when it changed, so an unchanged build does not compile the crate again.
    if std::fs::read_to_string(&dest).ok().as_deref() != Some(out.as_str()) {
        std::fs::write(&dest, out).unwrap();
    }
}

fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let name = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            files.push((name, path.canonicalize().unwrap()));
        }
    }
}
