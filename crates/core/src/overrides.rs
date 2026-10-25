//! Where the owner's choice differs from the advice: each choice the saved history shows
//! (`choices`), next to what the advice picks on the state it was made from.
//!
//! The live page shows the advice before the owner acts, so a choice that differs from it is a
//! deliberate override, and the payoff is mostly for the advisor: an override the judge can't
//! show was worse is a candidate replay fixture (CLAUDE.md workflow, step 1).
//!
//! - Plays and discards in a blind: the owner's move against Best play's pick (the quick pass,
//!   as the history check's picks are, `tests/history_picks.rs`), judged as the history check
//!   judges a moved pick (`advise::Options::judge`, D12): the oracle plays the rest of the round
//!   after each, and the verdict uses the search's tie margin. The judge shares the advice's
//!   valuation and engine, so a verdict is the advice judging itself: where the advice is wrong
//!   it rates the owner's correct move behind its own pick, and a verdict against the owner is
//!   not evidence the owner erred.
//! - Shop, pack and blind choices: by agreement only, never by a cost (nothing judges a shop
//!   choice yet, D12's limits): the shop ranking's top option or one shown tied with it
//!   (`advise::ShopOption::tied`), a lower one, or a choice the ranking doesn't list; skip or
//!   play against `advise::SkipCompare::verdict` ("close" is a tie).
//! - The jokers' order: by agreement with the order advice (`advise::OrderAdvice`, the quick
//!   pass's), on the jokers whose order can change a score.
//!
//! The advice is today's code on the saved state, not what the page showed then (the page
//! doesn't keep its analyses): an override of advice since changed reads as a match or the
//! other way round.

use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::advise::{self, Analysis, Options, ShopOption};
use crate::choices::{self, Choice, Unclear};
use crate::data::GameData;
use crate::save;

/// How a choice compares with the advice on its state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(tag = "grade", rename_all = "snake_case")]
pub enum Grade {
    /// The advice's own pick
    Same,
    /// A play or discard other than the pick, judged: the owner's move's worth over the pick's
    /// (share of the pick's, ± one standard error). `verdict` by the search's tie margin
    Judged { verdict: Verdict, gain: f64, se: f64 },
    /// A play or discard the judge can't play (not a move the game allows now)
    Unjudged,
    /// Shop and packs: the ranking's top option
    Top,
    /// Shown tied with the top (its place in the ranking, 1 = the top)
    TiedTop { place: usize },
    /// Ranked below the top's tie band
    Below { place: usize, of: usize },
    /// A choice the ranking doesn't list (a buy with no room, a swap for another joker…)
    NotAnOption,
    /// Skip or play: the comparison called it close
    Close,
    /// Skip or play: the other way from the comparison
    Other,
    /// The jokers put in another order than the order advice's (`gain`: what the advice's
    /// order adds over the one before the move, on a typical best hand; 0 when it said to keep
    /// it)
    OrderOther { gain: f64 },
}

/// The owner's move against the pick, by the search's tie margin (`advise::TIE_MARGIN`), as the
/// history check's judge reads its verdicts (`tests/history.rs`)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The owner's move clearly ahead, by more than the margin
    YoursAhead,
    /// The pick clearly ahead, by more than the margin
    PickAhead,
    /// Within the margin both ways
    Tie,
    /// Neither: more rounds could tell
    Unclear,
}

impl Verdict {
    pub fn of(gain: f64, se: f64) -> Verdict {
        let tie = advise::TIE_MARGIN;
        if gain - 2.0 * se > 0.0 && gain > tie {
            Verdict::YoursAhead
        } else if gain + 2.0 * se < 0.0 && gain < -tie {
            Verdict::PickAhead
        } else if gain - 2.0 * se >= -tie && gain + 2.0 * se <= tie {
            Verdict::Tie
        } else {
            Verdict::Unclear
        }
    }
}

impl Grade {
    /// The choice was the advice's (or one shown tied with it)
    pub fn agrees(&self) -> bool {
        matches!(self, Grade::Same | Grade::Top | Grade::TiedTop { .. } | Grade::Close)
    }

    /// An override worth a look as a replay fixture: the judge can't show the advice's pick was
    /// worth more (shop overrides are all candidates: nothing judges them)
    pub fn candidate(&self) -> bool {
        match self {
            Grade::Judged { verdict, .. } => *verdict != Verdict::PickAhead,
            Grade::Below { .. } | Grade::NotAnOption | Grade::Other | Grade::OrderOther { .. } => true,
            _ => false,
        }
    }

    pub fn describe(&self) -> String {
        match self {
            Grade::Same => "the advice's pick".into(),
            Grade::Judged { verdict, gain, se } => {
                let v = match verdict {
                    Verdict::YoursAhead => "yours ahead",
                    Verdict::PickAhead => "the advice's pick ahead",
                    Verdict::Tie => "a tie",
                    Verdict::Unclear => "unclear",
                };
                format!("{v}, by the advice's own measure: yours {:+.2}% ± {:.2}% of the pick's worth", 100.0 * gain, 100.0 * se)
            }
            Grade::Unjudged => "can't be judged (not a move the judge can play now)".into(),
            Grade::Top => "the ranking's top option".into(),
            Grade::TiedTop { place } => format!("shown tied with the top (#{place})"),
            Grade::Below { place, of } => format!("ranked #{place} of {of}, below the top's tie band"),
            Grade::NotAnOption => "not among the ranking's options".into(),
            Grade::Close => "skip or play called close".into(),
            Grade::Other => "the other way from skip or play's call".into(),
            Grade::OrderOther { gain } if *gain == 0.0 => "another order, where the advice kept the order as it was".into(),
            Grade::OrderOther { gain } => format!("another order than the advice's (its order +{:.1}% on a typical best hand over the one before)", 100.0 * gain),
        }
    }
}

/// One choice graded.
#[derive(Debug, Clone, Serialize)]
pub struct Graded {
    pub file: PathBuf,
    pub ante: i64,
    pub round: i64,
    pub screen: String,
    pub choice: Choice,
    /// What the advice picks on the state
    pub advice: String,
    /// The owner's choice as the shop ranking labels it, when it's one of its options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
    #[serde(flatten)]
    pub grade: Grade,
    /// The analyses' time for this choice (seconds), the judging included
    pub secs: f64,
}

/// Which choices to grade
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    All,
    /// Plays and discards only
    Blind,
    /// The rest: shop, pack and skip-or-play choices, the jokers' order
    Shop,
}

/// A run's choices against the advice.
#[derive(Debug, Clone, Serialize)]
pub struct RunReport {
    pub seed: String,
    /// The last saved state's ante, and whether the run was won
    pub reached_ante: i64,
    pub won: bool,
    pub graded: Vec<Graded>,
    /// Choices the saves don't show, by why
    pub unclear: Vec<(Unclear, usize)>,
    /// Choices left out by the scope or the limit
    pub not_graded: usize,
    pub secs: f64,
}

/// The choices in one run's history (`history/<seed>/`), graded, up to `limit` of them (0: all).
/// `judge_rounds` is the judge's rounds a move (`Options::judge_rounds`; the history check
/// uses 400). `progress` hears each choice as it's graded.
pub fn grade_run(dir: &Path, data: &GameData, scope: Scope, limit: usize, judge_rounds: usize, mut progress: impl FnMut(&Graded)) -> RunReport {
    let t0 = Instant::now();
    let (chosen, unclear) = choices::scan_run(dir, data);
    let mut by_why = std::collections::BTreeMap::new();
    for (_, u) in &unclear {
        *by_why.entry(*u).or_insert(0) += 1;
    }
    let states = choices::run_states(dir);
    let last = states.last().and_then(|p| save::load(p, data).ok());
    let in_scope = |c: &Choice| match scope {
        Scope::All => true,
        Scope::Blind => matches!(c, Choice::Play { .. } | Choice::Discard { .. }),
        Scope::Shop => !matches!(c, Choice::Play { .. } | Choice::Discard { .. }),
    };
    let mut graded = vec![];
    let mut not_graded = 0;
    for (file, choice) in chosen {
        if !in_scope(&choice) || (limit > 0 && graded.len() >= limit) {
            not_graded += 1;
            continue;
        }
        let Ok(run) = save::load(&file, data) else {
            not_graded += 1;
            continue;
        };
        let t = Instant::now();
        let (advice, option, grade) = grade(&run, &choice, data, judge_rounds);
        let g = Graded { file, ante: run.ante, round: run.round, screen: format!("{:?}", run.screen), choice, advice, option, grade, secs: t.elapsed().as_secs_f64() };
        progress(&g);
        graded.push(g);
    }
    RunReport {
        seed: dir.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        reached_ante: last.as_ref().map_or(0, |r| r.ante),
        won: last.as_ref().is_some_and(|r| r.won),
        graded,
        unclear: by_why.into_iter().collect(),
        not_graded,
        secs: t0.elapsed().as_secs_f64(),
    }
}

/// One choice against the advice on `run`, the state it was made from: what the advice picks,
/// the owner's choice as the shop ranking labels it, and the grade
pub fn grade(run: &save::RunState, choice: &Choice, data: &GameData, judge_rounds: usize) -> (String, Option<String>, Grade) {
    if let Some(mine) = choice.judge_name() {
        let quick = Options { quick: true, ..Default::default() };
        let Some(bp) = advise::analyze(run, data, None, &quick).best_play else { return ("no Best play".into(), None, Grade::Unjudged) };
        let shown = format!("{} {}", bp.action, bp.cards.join(" "));
        if by_positions(&bp) == mine {
            return (shown, None, Grade::Same);
        }
        let opts = Options { judge: vec![mine], judge_rounds, ..quick };
        let judged = advise::analyze(run, data, None, &opts).best_play.and_then(|b| b.judged.first().and_then(|j| j.1));
        return match judged {
            Some((gain, se)) => (shown, None, Grade::Judged { verdict: Verdict::of(gain, se), gain, se }),
            None => (shown, None, Grade::Unjudged),
        };
    }
    if let Choice::Order { order } = choice {
        return order_grade(run, order, data);
    }
    let a = advise::analyze(run, data, None, &Options::default());
    match choice {
        Choice::PlayBlind { .. } | Choice::SkipBlind { .. } => {
            let Some(skip) = a.blinds.iter().find_map(|b| b.skip.as_ref()) else { return ("no skip-or-play comparison".into(), None, Grade::NotAnOption) };
            let mine = if matches!(choice, Choice::SkipBlind { .. }) { "skip" } else { "play" };
            let grade = match skip.verdict.as_str() {
                "close" => Grade::Close,
                v if v == mine => Grade::Same,
                _ => Grade::Other,
            };
            (skip.verdict.clone(), None, grade)
        }
        _ => shop_grade(&a, choice),
    }
}

/// The jokers' new order against the order advice on the state before the move, compared on
/// the jokers whose order can change a score (the rest may sit anywhere)
fn order_grade(run: &save::RunState, order: &[String], data: &GameData) -> (String, Option<String>, Grade) {
    let a = advise::analyze(run, data, None, &Options { quick: true, ..Default::default() });
    let Some(adv) = a.order else { return ("no order advice".into(), None, Grade::NotAnOption) };
    let board = crate::engine::Board::from_run(run, data);
    let copiers = board.jokers.iter().any(|j| matches!(j.kind, crate::engine::Kind::Blueprint | crate::engine::Kind::Brainstorm));
    let movable: Vec<String> = board.jokers.iter().filter(|j| copiers || !advise::order_free(j)).map(|j| data.name(&j.key).to_string()).collect();
    let only_movable = |v: &[String]| v.iter().filter(|n| movable.contains(n)).cloned().collect::<Vec<_>>();
    let shown = if adv.is_current { "keep the order".to_string() } else { adv.order.join(", ") };
    let grade = if only_movable(order) == only_movable(&adv.order) { Grade::Same } else { Grade::OrderOther { gain: if adv.is_current { 0.0 } else { adv.gain } } };
    (shown, None, grade)
}

/// A pick by its cards' places in the hand, sorted, as `Choice::judge_name` names a move
fn by_positions(bp: &advise::PlayAdvice) -> String {
    let mut ix = bp.indices.clone();
    ix.sort();
    format!("{} {} [{}]", bp.action, ix.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","), bp.use_first.clone().unwrap_or_default())
}

fn shop_grade(a: &Analysis, choice: &Choice) -> (String, Option<String>, Grade) {
    let Some(top) = a.options.first() else { return ("no options".into(), None, Grade::NotAnOption) };
    let Some(i) = a.options.iter().position(|o| is_choice(o, choice)) else { return (top.label.clone(), None, Grade::NotAnOption) };
    // the top's tie band: the options after it shown tied with the one above
    let band = 1 + a.options[1..].iter().take_while(|o| o.tied).count();
    let grade = if i == 0 {
        Grade::Top
    } else if i < band {
        Grade::TiedTop { place: i + 1 }
    } else {
        Grade::Below { place: i + 1, of: a.options.len() }
    };
    (top.label.clone(), Some(a.options[i].label.clone()), grade)
}

/// The shop ranking's option for a choice
fn is_choice(o: &ShopOption, c: &Choice) -> bool {
    let picked = o.label.starts_with("pick ");
    let key = |k: &str| o.key.as_deref() == Some(k);
    match c {
        Choice::Leave => o.kind == "leave",
        Choice::SkipPack => o.kind == "skip",
        Choice::Reroll => o.kind == "reroll",
        Choice::Sell { key: k, .. } => o.kind == "sell" && o.sell_key.as_deref() == Some(k),
        Choice::Pick { key: k, .. } => picked && key(k),
        Choice::Buy { item, key: k, sold, .. } => o.kind == *item && key(k) && !picked && !o.label.ends_with("(you have it)") && (item != "joker" || o.sell_key == *sold),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_by_the_search_tie_margin() {
        let t = advise::TIE_MARGIN;
        assert_eq!(Verdict::of(3.0 * t, 0.1 * t), Verdict::YoursAhead);
        assert_eq!(Verdict::of(-3.0 * t, 0.1 * t), Verdict::PickAhead);
        assert_eq!(Verdict::of(0.0, 0.1 * t), Verdict::Tie);
        // clearly apart but within the margin, or too noisy to say
        assert_eq!(Verdict::of(0.0, t), Verdict::Unclear);
        // only an override the judge can't show was worse is a fixture candidate
        assert!(!Grade::Judged { verdict: Verdict::PickAhead, gain: -0.1, se: 0.01 }.candidate());
        assert!(Grade::Judged { verdict: Verdict::Tie, gain: 0.0, se: 0.0 }.candidate());
        assert!(Grade::Below { place: 3, of: 5 }.candidate() && !Grade::TiedTop { place: 2 }.candidate() && !Grade::Same.candidate());
    }
}
