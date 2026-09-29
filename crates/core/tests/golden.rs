//! Every golden case with a real in-game score must match the engine exactly.

use std::path::Path;

#[test]
fn golden_cases_match_the_game() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden");
    let mut failures = Vec::new();
    for (path, case) in balatro_advisor::golden::load_all(&dir) {
        let case = case.unwrap_or_else(|e| panic!("{e}"));
        let Some(expected) = case.expected else { continue };
        let got = case.rescore();
        if got != expected {
            failures.push(format!(
                "{}: engine {got}, game {expected} (run `balatro-advisor golden show {}` for the trace)",
                path.display(),
                case.name
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
