use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
        .parent()
        .expect("workspace root")
        .to_path_buf();
    println!("cargo:rerun-if-changed=../assets/baked");
    // The common build checks this directory against the complete source set. New
    // assets are discovered as data, without adding a renderer match arm.
    let mut paths: Vec<_> = fs::read_dir(root.join("assets/baked"))
        .expect("baked assets directory")
        .map(|entry| entry.expect("baked asset entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    let mut registry = String::from("const BAKED_ASSETS: &[(&str, &str)] = &[\n");
    for path in paths {
        registry.push_str(&format!(
            "({:?}, include_str!({:?})),\n",
            path.file_stem()
                .expect("asset ID")
                .to_str()
                .expect("UTF-8 ID"),
            path,
        ));
    }
    registry.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").expect("Cargo output directory"))
            .join("voxel_registry.rs"),
        registry,
    )
    .expect("write voxel registry");
}
