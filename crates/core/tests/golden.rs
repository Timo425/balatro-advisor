//! Every golden case with a real in-game score must match the engine exactly, except a case
//! marked `pending` (a known engine bug), which must still disagree.

use std::path::Path;

#[test]
fn golden_cases_match_the_game() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden");
    let mut failures = Vec::new();
    for (path, case) in balatro_advisor::golden::load_all(&dir) {
        let case = case.unwrap_or_else(|e| panic!("{e}"));
        let (Some(expected), Some(agrees)) = (case.expected, case.agrees()) else { continue };
        let got = case.rescore();
        match (&case.pending, agrees) {
            (None, false) => failures.push(format!(
                "{}: engine {got}, game {expected} (run `balatro-advisor golden show {}` for the trace)",
                path.display(),
                case.name
            )),
            (Some(why), true) => failures.push(format!("{}: pending ({why}) but the engine now agrees: remove `pending`", path.display())),
            _ => {}
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
