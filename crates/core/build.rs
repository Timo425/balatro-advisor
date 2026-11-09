//! Stamps what the library is compiled from into every program built from this crate
//! (`BAV_SOURCE`, a hash of `hash::INPUTS`), so a check can refuse to run on older code than
//! the tree holds (`tests/common/stamp.rs` `start`).

#[path = "tests/common/source_hash.rs"]
mod hash;

fn main() {
    for input in hash::INPUTS {
        println!("cargo:rerun-if-changed={input}");
    }
    println!("cargo:rerun-if-changed=tests/common/source_hash.rs");
    let dir = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets it"));
    println!("cargo:rustc-env=BAV_SOURCE={}", hash::source_hash(&dir));
}
