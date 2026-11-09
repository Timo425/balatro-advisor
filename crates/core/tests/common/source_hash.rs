//! A hash of what the library is compiled from, shared by `build.rs` (which stamps it into every
//! program built from the crate as `BAV_SOURCE`) and `stamp.rs` (which recomputes it from the
//! tree when a check starts). Its own file so that editing `stamp.rs` doesn't rerun the build
//! script and rebuild the library.

use std::path::{Path, PathBuf};

/// What the library is compiled from, relative to the crate: its source and the bundled data
/// (`data.rs` `GameData::bundled`). `tools/checks.py` hashes the same.
pub const INPUTS: [&str; 2] = ["src", "../../data/game.json"];

fn fnv(h: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(h, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// A hash of `INPUTS` under the crate at `crate_dir`: FNV-1a over each file's path from the
/// crate ('/'-separated) and its text with CRLF read as LF, in path order (`tools/checks.py`
/// `source_hash` is the same)
pub fn source_hash(crate_dir: &Path) -> String {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = format!("{rel}/{}", e.file_name().to_string_lossy());
            if e.path().is_dir() {
                walk(&e.path(), &name, out);
            } else {
                out.push((name, e.path()));
            }
        }
    }
    let mut files = vec![];
    for input in INPUTS {
        let p = crate_dir.join(input);
        if p.is_dir() {
            walk(&p, input, &mut files);
        } else {
            files.push((input.to_string(), p));
        }
    }
    files.sort();
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for (name, path) in files {
        let text = std::fs::read(&path).unwrap_or_default();
        let lf: Vec<u8> = text.iter().enumerate().filter(|&(i, &b)| !(b == b'\r' && text.get(i + 1) == Some(&b'\n'))).map(|(_, &b)| b).collect();
        h = fnv(fnv(fnv(fnv(h, name.as_bytes()), &[0]), &lf), &[0]);
    }
    format!("{h:016x}")
}
