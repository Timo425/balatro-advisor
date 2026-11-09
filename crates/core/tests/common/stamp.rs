//! What a check runs on (CLAUDE.md step 4). `start` refuses a test program built from other
//! code than the tree holds now (one run straight from `target/*/deps/` after an edit checks the
//! old code; `cargo test` rebuilds it first) and logs the run, so `tools/checks.py` can tell
//! which runs were on the code you're committing, which on older code or another base, and
//! which never finished. `build.rs` compiles this file too, for `source_hash`: it stamps the
//! library's inputs into every program built from the crate (`BAV_SOURCE`). Uses only the
//! standard library and `option_env!`, so a test that includes it still builds when copied into
//! a worktree of a base without `build.rs` (no stamp there to check against: logged, not checked).
#![allow(dead_code)]

use std::io::Write;
use std::path::{Path, PathBuf};

/// What the library is compiled from, relative to the crate: its source and the bundled data
/// (`data.rs` `GameData::bundled`). `tools/checks.py` hashes the same.
pub const INPUTS: [&str; 2] = ["src", "../../data/game.json"];

/// The log every check run appends to, in the temp folder: per machine, read by `tools/checks.py`
pub const LOG: &str = "balatro-advisor-checks.tsv";

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

/// A source file of this test program changed after it was built: the first one, by cargo's
/// dep-info beside the program (`<name>-<hash>.d`, paths from the workspace root)
fn newer_test_source() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let built = std::fs::metadata(&exe).ok()?.modified().ok()?;
    let deps = std::fs::read_to_string(exe.with_extension("d")).ok()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // each dependency also stands alone as `path:`, after the targets' `target: deps` lines
    deps.lines().filter(|l| !l.starts_with('#') && !l.contains(": ")).filter_map(|l| l.strip_suffix(':')).map(|l| l.replace("\\ ", " ")).find(|dep| {
        let p = root.join(dep);
        std::fs::metadata(&p).and_then(|m| m.modified()).is_ok_and(|t| t > built)
    })
}

/// A check's run, logged as ended (done, or failed on a panic) when dropped; a run killed
/// before has no end in the log
pub struct Run {
    id: String,
}

impl Drop for Run {
    fn drop(&mut self) {
        let how = if std::thread::panicking() { "failed" } else { "done" };
        log(&format!("{}\tend\t{}\t{how}\n", now(), self.id));
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn log(line: &str) {
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join(LOG)) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Starts the check `check`, writing to `out` if it does: panics if this program is older than
/// the code (the library's inputs hash otherwise than when it was built, or one of this test's
/// own source files changed since), then logs the run (`LOG`): when, the check, its output, the
/// code (its hash, the crate's folder, the commit and whether the library's inputs differ from
/// it) and every `BAV_*` setting. Keep the returned `Run` until the check ends.
#[must_use = "the run is logged as ended when this is dropped"]
pub fn start(check: &str, out: Option<&Path>) -> Run {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let hash = source_hash(crate_dir);
    let rebuild = "run it through `cargo test`, which rebuilds it";
    if let Some(built) = option_env!("BAV_SOURCE") {
        assert!(built == hash, "{check}: this test program was built from other code than the tree holds now (crates/core/src or data/game.json changed since: built from {built}, now {hash}); {rebuild}");
    }
    if let Some(file) = newer_test_source() {
        panic!("{check}: {file} changed after this test program was built; {rebuild}");
    }
    let git = |args: &[&str]| {
        let o = std::process::Command::new("git").arg("-C").arg(crate_dir).args(args).output().ok().filter(|o| o.status.success())?;
        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    let commit = git(&["rev-parse", "--short=10", "HEAD"]).unwrap_or_else(|| "?".into());
    let state = match git(&["status", "--porcelain", "--", INPUTS[0], INPUTS[1]]) {
        Some(s) if s.is_empty() => "clean",
        Some(_) => "changed",
        None => "?",
    };
    let mut settings: Vec<String> = std::env::vars().filter(|(k, _)| k.starts_with("BAV_")).map(|(k, v)| format!("{k}={v}")).collect();
    settings.sort();
    let when = now();
    let id = format!("{}-{when}-{check}", std::process::id());
    let out = out.map(|o| std::path::absolute(o).unwrap_or_else(|_| o.to_path_buf()).display().to_string()).unwrap_or_default();
    log(&format!("{when}\tstart\t{id}\t{check}\t{out}\t{hash}\t{}\t{commit}\t{state}\t{}\n", crate_dir.display(), settings.join(" ")));
    eprintln!("{check}: on source {hash} ({commit}{}){}", if state == "changed" { " with changes" } else { "" }, if out.is_empty() { String::new() } else { format!(" -> {out}") });
    Run { id }
}

/// Moves a single-file output an earlier run left in `dir` aside (`<stem>.previous.<ext>`), so
/// a run that dies midway leaves no file that passes for its own
pub fn set_aside(dir: &Path, file: &str) {
    let p = dir.join(file);
    if p.exists() {
        let to = match file.rsplit_once('.') {
            Some((stem, ext)) => format!("{stem}.previous.{ext}"),
            None => format!("{file}.previous"),
        };
        let _ = std::fs::rename(&p, dir.join(to));
    }
}
