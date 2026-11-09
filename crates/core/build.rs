//! Stamps what the library is compiled from into every program built from this crate
//! (`BAV_SOURCE`, a hash of `stamp::INPUTS`), so a check can refuse to run on older code than
//! the tree holds (`tests/common/stamp.rs` `start`).

#[path = "tests/common/stamp.rs"]
mod stamp;

fn main() {
    for input in stamp::INPUTS {
        println!("cargo:rerun-if-changed={input}");
    }
    println!("cargo:rerun-if-changed=tests/common/stamp.rs");
    let dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    println!("cargo:rustc-env=BAV_SOURCE={}", stamp::source_hash(&dir));
}
