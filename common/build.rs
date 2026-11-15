use std::{env, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"))
        .parent()
        .expect("workspace root")
        .to_path_buf();
    println!("cargo:rerun-if-changed=../scripts/bake_voxels.py");
    println!("cargo:rerun-if-changed=../assets/voxel");
    println!("cargo:rerun-if-changed=../assets/baked");
    println!("cargo:rerun-if-env-changed=PYTHON");
    let status = Command::new(env::var_os("PYTHON").unwrap_or_else(|| "python3".into()))
        .current_dir(root)
        .args(["scripts/bake_voxels.py", "--check"])
        .status()
        .expect("logical rig build requires Python 3.12+ (or set PYTHON)");
    assert!(
        status.success(),
        "invalid/stale authored rig; see baker diagnostic"
    );
}
