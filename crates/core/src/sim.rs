//! Monte Carlo on top of the scoring engine: best play for a hand, and whole-round
//! simulations for "chance to beat this blind".
//!
//! The discard policy is a heuristic (labelled as such in every output): the game has
//! no "right" discard, and a perfect one would need a search far beyond a 1 s budget.

use crate::engine::hand::{self, HandType};
use crate::engine::score::{self, Board};
use crate::engine::{Joker, Kind, Rng, Rolls, Unlucky};
use crate::engine::consumable::CardEffect;
use crate::model::{Card, Edition, Enhancement, Suit};

/// Best play found for one hand.
#[derive(Debug, Clone)]
pub struct Play {
    /// Indices into the hand, in the order to play them.
    pub cards: Vec<usize>,
    pub hand: HandType,
    /// Score with every random roll failing, and a pick of cards (The Hook's) averaged over what
    /// it could take (gap 15): what a play is chosen by.
    pub floor: f64,
    /// Score with every roll failing and the pick that costs most (`Outcome::sure`): whether the
    /// play surely wins. The floor when nothing is picked.
    pub sure: f64,
    /// The most any play the search tried surely scores (`sure`): whether the round can be won
    /// surely now, by this play or another (`surely_winning`). The floor when nothing is picked.
    pub surest: f64,
    /// Average score over the random rolls (Misprint, Lucky cards, …): what the play is
    /// expected to add, the measure for pace and for comparing it with a chase. The floor
    /// when nothing in it is random.
    pub mean: f64,
}

/// Rolls averaged for `Play::mean`
const MEAN_ROLLS: usize = 8;

/// `Play::mean` of `play` (cards of `hand`) whose floor is `floor`.
fn mean_score(b: &Board, hand: &[Card], play: &[usize], floor: f64) -> f64 {
    let played: Vec<Card> = play.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !play.contains(i)).map(|i| hand[i]).collect();
    // the hand detected once for every roll
    let info = hand::detect(&played, b.rule_flags());
    if score::score_detected(b, &played, &held, info.clone(), &mut crate::engine::Lucky, false).score <= floor {
        return floor;
    }
    let mut rolls = Rng::new(0x6d65616e);
    (0..MEAN_ROLLS).map(|_| score::score_detected(b, &played, &held, info.clone(), &mut rolls, false).score).sum::<f64>() / MEAN_ROLLS as f64
}

/// Whether the board has jokers that care about unscored kickers (so kicker choices matter).
fn kickers_matter(b: &Board) -> bool {
    b.blind.key == "bl_psychic"
        || b.jokers.iter().any(|j| {
            matches!(j.kind, Kind::Half | Kind::Square | Kind::Blackboard | Kind::RaisedFist | Kind::Splash | Kind::Hiker)
        })
}

/// Order a play the way a player would: +Chips/+Mult cards first, ×Mult cards (Glass,
/// Polychrome) last, so multipliers apply to everything before them; a face-down card by what
/// you can see of it, nothing (gap 25). Heuristic, on the retire list: the engine arranges the
/// cards itself (`engine::score`) and keeps this order only when its arrangement doesn't score
/// more as played.
fn arrange(hand: &[Card], idx: &mut [usize]) {
    let xmult = |c: &Card| !c.face_down && (c.enhancement == Some(Enhancement::Glass) || c.edition == Some(Edition::Polychrome));
    idx.sort_by_key(|&i| (xmult(&hand[i]), i));
}

/// Tries every 1–5 card subset of `hand` and returns the highest-scoring play.
/// Subsets with unscored kickers are skipped unless a joker or the boss cares about them.
pub fn best_play(b: &Board, hand: &[Card]) -> Option<Play> {
    best_play_with(b, hand, true)
}

/// `best_play`; `prefilter`: skip the plays `could_all_score` rules out before detecting them
fn best_play_with(b: &Board, hand: &[Card], prefilter: bool) -> Option<Play> {
    let n = hand.len().min(SUBSETS_MAX);
    best_play_of(b, hand, (1 << n) - 1, prefilter, None).map(|(mut p, rolled)| {
        // a floor that rolled nothing is every roll's score (with the same cards held: the
        // whole hand, unless it's more than `SUBSETS_MAX` cards)
        p.mean = if rolled || n < hand.len() { mean_score(b, hand, &p.cards, p.floor) } else { p.floor };
        p
    })
}

/// The most cards the simulated player's best play tries every subset of (`best_play_of`): a
/// bigger hand's best play is among its first 16 (`Card::order_key`, highest ranks first). A
/// cut, not a finding (gap 20): the plays a hand's structure gives (`scoring_plays`) cover
/// every card but cost ~10x on a 32-card hand, where it's chosen hundreds of thousands of
/// times an analysis.
const SUBSETS_MAX: usize = 16;

/// The most cards Best play tries every play and every discard of (`all_plays`,
/// `all_discards`); a bigger hand's moves come from its structure (`big_hand_moves`)
pub const ALL_MOVES_MAX: usize = 12;

/// The plays of a hand of more than `ALL_MOVES_MAX` cards that Best play races: the best by
/// their score now (`big_hand_moves`). A budget, not a finding: an 8-card hand has 218 plays.
const BIG_HAND_PLAYS: usize = 256;

/// Every play of `hand` (bit masks, ascending) whose cards could all score: Stone cards with
/// any play (they always score: state_events.lua `G.FUNCS.evaluate_play`), the rest only as
/// poker hands make them, where every part of a play that could all score could too: one rank
/// or two, one suit as flushes count it (`hand::is_suit`: Wild cards, Smeared Joker), distinct
/// ranks within one straight's run (`straight_runs`, Shortcut's gaps, the Ace low or high);
/// with Four Fingers one card may fall outside the suit or repeat a rank in the run. Built card
/// by card: a set that can't be part of such a play is dropped with every set containing it, so
/// a 32-card hand gives thousands, not every 5-card subset. A superset (`hand::detect`
/// decides), except with Splash, where every card scores: then only the plays a poker hand
/// makes, and the extra cards are `with_kickers`' (the highest few: a heuristic). Every card is
/// read as it is: give it a draw of the face-down cards, never their stand-ins
/// (`big_hand_moves`); the first 64 cards, the first 10 Stone cards.
pub fn scoring_plays(hand: &[Card], f: hand::RuleFlags) -> Vec<u64> {
    let n = hand.len().min(64);
    let stone = |i: usize| hand[i].enhancement == Some(Enhancement::Stone);
    let stones: Vec<usize> = (0..n).filter(|&i| stone(i)).take(10).collect();
    let rest: Vec<usize> = (0..n).filter(|&i| !stone(i)).collect();
    let runs = straight_runs(5, f.shortcut);
    let slack = usize::from(f.four_fingers);
    let could = |q: &[usize]| {
        if q.len() <= 1 {
            return true;
        }
        let mut ids: Vec<i32> = q.iter().map(|&i| i32::from(hand[i].rank.0)).collect();
        ids.sort_unstable();
        let mut distinct = ids.clone();
        distinct.dedup();
        if distinct.len() <= 2 {
            return true;
        }
        let off_suit = |s: Suit| q.iter().filter(|&&i| !hand::is_suit(&hand[i], s, false, true, f.smeared)).count();
        if Suit::ALL.into_iter().any(|s| off_suit(s) <= slack) {
            return true;
        }
        ids.len() - distinct.len() <= slack && runs.iter().any(|run| distinct.iter().all(|&r| run.contains(&r) || (r == 14 && run.contains(&1))))
    };
    fn grow(rest: &[usize], from: usize, q: &mut Vec<usize>, could: &dyn Fn(&[usize]) -> bool, out: &mut Vec<Vec<usize>>) {
        out.push(q.clone());
        if q.len() == 5 {
            return;
        }
        for k in from..rest.len() {
            q.push(rest[k]);
            if could(q) {
                grow(rest, k + 1, q, could, out);
            }
            q.pop();
        }
    }
    let mut cores = vec![];
    grow(&rest, 0, &mut vec![], &could, &mut cores);
    // each with every set of Stone cards that fits
    let mut out = vec![];
    for core in cores {
        let m: u64 = core.iter().fold(0, |m, &i| m | 1 << i);
        let room = 5 - core.len();
        for s in 0u64..(1 << stones.len()) {
            if s.count_ones() as usize <= room && (m != 0 || s != 0) {
                out.push((0..stones.len()).filter(|k| s & (1 << k) != 0).fold(m, |m, k| m | 1 << stones[k]));
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// The cards of a play's mask
fn mask_cards(m: u64) -> Vec<usize> {
    (0..64).filter(|i| m & (1 << i) != 0).collect()
}

/// A play's cards topped up from the cards outside it (up to 5 of the lowest, `junk_order`;
/// with Splash, where every card scores, the 5 highest): every set of them that fits. Unscored cards count only through what they leave
/// in hand (Raised Fist, Blackboard, Baron…), how many cards are played (Square, Half Joker,
/// The Psychic) and the cards a play cycles out (a dig): the lowest are what a player plays
/// away. A heuristic, for hands too big for every subset.
fn with_kickers(hand: &[Card], m: u64, splash: bool) -> Vec<u64> {
    let mut low: Vec<usize> = (0..hand.len().min(64)).filter(|&i| m & (1 << i) == 0).collect();
    junk_order(hand, &mut low);
    if splash {
        low.reverse();
    }
    low.truncate(5);
    let room = 5usize.saturating_sub(m.count_ones() as usize);
    (1u32..(1 << low.len())).filter(|s| s.count_ones() as usize <= room).map(|s| (0..low.len()).filter(|k| s & (1 << k) != 0).fold(m, |m, k| m | 1 << low[k])).collect()
}

/// Cards least worth keeping first: plain or debuffed ones (no enhancement, edition or seal
/// that counts) before the rest, then by chips, then position
fn junk_order(hand: &[Card], idx: &mut [usize]) {
    let plain = |c: &Card| c.debuff || (c.enhancement.is_none() && c.edition.is_none() && c.seal.is_none());
    idx.sort_by(|&a, &c| plain(&hand[c]).cmp(&plain(&hand[a])).then(hand[a].rank.chips().total_cmp(&hand[c].rank.chips())).then(a.cmp(&c)));
}

/// Of each kind of play (its hand and how many cards it plays), the best this many by score
/// now go into a big hand's race first (`big_hand_moves`), so the budget isn't all one hand
const BIG_HAND_PER_KIND: usize = 8;

/// The draws of a big hand's face-down cards its structure is read from (`big_hand_moves`): the
/// first of `BIG_HAND_RANK_DRAWS`. A budget, not a finding (the register).
pub const BIG_HAND_DRAWS: usize = 4;

/// The most draws of a big hand's face-down cards its plays are ranked on (`big_hand_moves`), on
/// seeds of their own the race never plays (`play.rs` `moves_of`): a play worth its occasional
/// Flush or Straight ranks by its mean only over many (gap 25 (c)). A budget, not a finding (the
/// register).
pub const BIG_HAND_RANK_DRAWS: usize = 128;

/// The scores a big hand's ranking may take (`big_hand_moves`: its plays times the draws they're
/// ranked on, at least `BIG_HAND_DRAWS` of them), on every core: 128 draws up to 28 cards all
/// face down, about 1.6–1.8 s on 16 threads at the full budget. A budget, not a finding (the
/// register).
const BIG_HAND_RANK_SCORES: usize = 16_000_000;

/// The fewest draws every play holding a face-down card is listed for (`big_hand_moves`): such a
/// play is worth what it may make, which fewer draws rank only by luck (the suited 16-card hand's
/// best passed on 0–1 of 8 seeds at 4 draws, 5 at 16, every one at 128: gap 25 (c)); past it
/// (over 42 cards all face down) the structure alone. A budget, not a finding (the register).
const BIG_HAND_LIST_DRAWS: usize = 16;

/// How many plays of 1 to 5 of `n` cards there are
fn plays_of(n: usize) -> usize {
    (1..=5.min(n)).map(|k| (0..k).fold(1usize, |a, i| a * (n - i) / (i + 1))).sum()
}

/// Best play's moves for a hand of more than `ALL_MOVES_MAX` cards, where every play and
/// discard would be far more than a race can take (32 cards: 242,824 plays): the plays the
/// hand's structure gives over every card (`scoring_plays`), each also topped up with the
/// lowest cards outside it (`with_kickers`: as kickers, or junk played to dig), the best
/// `BIG_HAND_PLAYS` by their score now (with every roll failing); and the discards the
/// hand's structure gives (`structure_discards`). Each play in the order to play it.
/// `draws`: the hand with its face-down cards drawn `BIG_HAND_RANK_DRAWS` times (`hidden_hands`,
/// on seeds the race doesn't play, so the
/// screen doesn't pick the plays its first rounds favour), or just the hand when it holds none.
/// The moves are the union of the first `BIG_HAND_DRAWS` draws' structure and every play holding
/// a face-down card (while they can be ranked on `BIG_HAND_LIST_DRAWS`), a play's score now its
/// mean over as many draws as `BIG_HAND_RANK_SCORES` allows (128 up to 28 cards all face down, 65
/// at 32) and its kind the hand most of them make, so no move reads a face-down card's stand-in,
/// a play isn't listed for what a few draws happened to make it, and a hand of only them is one
/// more case (gap 25 (c)). The cards' order is the first draw's (`arrange`; the engine orders
/// them anyway, by what each round dealt: gap 25).
pub fn big_hand_moves(b: &Board, draws: &[Vec<Card>], deck: &[Card], discards: i64) -> Vec<Move> {
    // a play scored on the draws it's ranked on: its mean score, mask, the hand most of them make
    // (the first draw's on a tie), its score on each of the structure's draws
    type Scored = (f64, u64, HandType, [f64; BIG_HAND_DRAWS]);
    let Some(hand) = draws.first() else { return vec![] };
    debug_assert!(draws.iter().all(|h| h.len() == hand.len()));
    let flags = b.rule_flags();
    let structure = &draws[..draws.len().min(BIG_HAND_DRAWS)];
    // each draw's structure, its face-down cards as drawn, and its lowest cards as kickers
    let mut masks: Vec<u64> = vec![];
    for h in structure {
        let plays = scoring_plays(h, flags);
        masks.extend(plays.iter().flat_map(|&m| with_kickers(h, m, flags.splash)));
        masks.extend(plays);
    }
    // and every play holding a face-down card: what it's worth lies in what it may make, which a
    // few draws' structure finds only when one of them happens to make it
    let n = hand.len().min(64);
    let down: u64 = (0..n).filter(|&i| hand[i].face_down).fold(0, |m, i| m | 1 << i);
    // (while everything ranked can be on `BIG_HAND_LIST_DRAWS`: the structure's plays as well)
    masks.sort_unstable();
    masks.dedup();
    let listed = plays_of(n) - plays_of(n - down.count_ones() as usize);
    let unlisted = masks.iter().filter(|&&m| m & down == 0).count();
    if down != 0 && (listed + unlisted) * BIG_HAND_LIST_DRAWS <= BIG_HAND_RANK_SCORES {
        fn grow(n: usize, from: usize, m: u64, down: u64, out: &mut Vec<u64>) {
            if m & down != 0 {
                out.push(m);
            }
            if m.count_ones() < 5 {
                for i in from..n {
                    grow(n, i + 1, m | 1 << i, down, out);
                }
            }
        }
        grow(n, 0, 0, down, &mut masks);
    }
    masks.sort_unstable();
    masks.dedup();
    // each play in the order to play it
    let ordered = |m: u64| {
        let mut idx = mask_cards(m);
        arrange(hand, &mut idx);
        idx
    };
    // ranked on as many draws as the budget allows, on every core
    let rank = &draws[..(BIG_HAND_RANK_SCORES / masks.len().max(1)).clamp(structure.len(), draws.len())];
    let mut scored: Vec<Scored> = crate::progress::par_map(&masks, |&m| {
        let idx = ordered(m);
        let mut sum = 0.0;
        let mut first = [0.0; BIG_HAND_DRAWS];
        let mut kinds: Vec<(HandType, usize)> = vec![];
        for (d, h) in rank.iter().enumerate() {
            let played: Vec<Card> = idx.iter().map(|&i| h[i]).collect();
            let held: Vec<Card> = (0..h.len()).filter(|i| !idx.contains(i)).map(|i| h[i]).collect();
            let o = score::score(b, &played, &held, &mut Unlucky, false);
            sum += o.score;
            if d < BIG_HAND_DRAWS {
                first[d] = o.score;
            }
            match kinds.iter_mut().find(|k| k.0 == o.hand) {
                Some(k) => k.1 += 1,
                None => kinds.push((o.hand, 1)),
            }
        }
        let kind = kinds.iter().fold(kinds[0], |a, &k| if k.1 > a.1 { k } else { a }).0;
        (sum / rank.len() as f64, m, kind, first)
    });
    // the best first; on a tie, fewer cards (a play topped up scores the same), then the mask
    scored.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.count_ones().cmp(&y.1.count_ones())).then(x.1.cmp(&y.1)));
    // the best few of each kind (hand, cards played: a dig with junk is a kind of its own),
    // then the best of the rest, up to the budget
    let mut per_kind: std::collections::HashMap<(HandType, u32), usize> = std::collections::HashMap::new();
    let (mut first, mut rest): (Vec<usize>, Vec<usize>) = (vec![], vec![]);
    for (k, x) in scored.iter().enumerate() {
        let n = per_kind.entry((x.2, x.1.count_ones())).or_insert(0);
        *n += 1;
        if *n <= BIG_HAND_PER_KIND { first.push(k) } else { rest.push(k) }
    }
    first.truncate(BIG_HAND_PLAYS);
    let room = BIG_HAND_PLAYS - first.len();
    first.extend(rest.into_iter().take(room));
    first.sort_unstable();
    let mut out: Vec<Move> = first.iter().map(|&k| Move::Play(ordered(scored[k].1))).collect();
    // each draw's best play over every card (the first of the best on it): what the structure's
    // discards of that draw keep first (draw 0's all, as a hand without face-down cards always
    // had them); a later draw adds only discards not given yet
    let plays = out.len();
    let same = |x: &Move, y: &Move| match (x, y) {
        (Move::Discard(x), Move::Discard(y)) => x.len() == y.len() && x.iter().all(|i| y.contains(i)),
        _ => false,
    };
    for (d, h) in structure.iter().enumerate() {
        let Some(best) = scored.iter().fold(None::<&Scored>, |a, x| match a {
            Some(a) if a.3[d] >= x.3[d] => Some(a),
            _ => Some(x),
        }) else {
            break;
        };
        for m in structure_discards(b, h, deck, discards, ordered(best.1)) {
            if d == 0 || !out[plays..].iter().any(|x| same(x, &m)) {
                out.push(m);
            }
        }
    }
    out
}

/// A source of rolls that notes whether it was asked for one: a score that asked for none is
/// the same whatever the rolls (the engine's randomness is all `Rolls`)
struct Watched<R> {
    rolls: R,
    asked: bool,
}

impl<R: crate::engine::Rolls> crate::engine::Rolls for Watched<R> {
    fn chance(&mut self, p: f64) -> bool {
        self.asked = true;
        self.rolls.chance(p)
    }
    fn range(&mut self, min: i64, max: i64) -> i64 {
        self.asked = true;
        self.rolls.range(min, max)
    }
    fn unit(&mut self) -> f64 {
        self.asked = true;
        self.rolls.unit()
    }
    /// an average is exact, not a roll drawn (gap 15)
    fn pick(&mut self, n: usize, k: usize) -> crate::engine::Pick {
        let p = self.rolls.pick(n, k);
        self.asked |= p != crate::engine::Pick::Average;
        p
    }
}

/// `best_play_with` of the cards `part` (a mask over `hand`'s first 16), without its mean: the
/// play's cards are indices into `hand`, and whether its floor asked for a roll (`Watched`).
/// `seen`: what's known of the plays of `hand` (by mask), filled in as it goes.
fn best_play_of(b: &Board, hand: &[Card], part: u32, prefilter: bool, seen: Option<&mut [Option<SeenPlay>]>) -> Option<(Play, bool)> {
    best_play_among(b, hand, part, prefilter, seen, None, None)
}

/// `best_play_of`, searching only the plays `only` (masks, ascending) when given, and noting
/// every play it scores in `scored` (its mask, hand, and chips and Mult with every roll failing,
/// before the deck balances them: `Outcome::unbalanced`; and its floor)
fn best_play_among(
    b: &Board,
    hand: &[Card],
    part: u32,
    prefilter: bool,
    mut seen: Option<&mut [Option<SeenPlay>]>,
    only: Option<&[u32]>,
    mut scored: Option<&mut Vec<Scored>>,
) -> Option<(Play, bool)> {
    let n = hand.len().min(16);
    let flags = b.rule_flags();
    let keep_kickers = kickers_matter(b);
    let mut best: Option<(Play, bool)> = None;
    let mut surest = f64::NEG_INFINITY;
    let mut played: Vec<Card> = Vec::with_capacity(5);
    let mut held: Vec<Card> = Vec::with_capacity(n);
    let mut idx: Vec<usize> = Vec::with_capacity(5);
    // every play within the part (or of `only`), in increasing order of its mask
    let mut mask = 0u32;
    let mut only = only.map(|o| o.iter());
    loop {
        mask = match only.as_mut() {
            Some(o) => match o.next() {
                Some(&m) => m,
                None => break,
            },
            None => mask.wrapping_sub(part) & part,
        };
        if mask == 0 {
            break;
        }
        let k = mask.count_ones();
        if k > 5 {
            continue;
        }
        if prefilter && !keep_kickers && !could_all_score(hand, mask, n, flags) {
            continue;
        }
        idx.clear();
        idx.extend((0..n).filter(|i| mask & (1 << i) != 0));
        arrange(hand, &mut idx);
        played.clear();
        played.extend(idx.iter().map(|&i| hand[i]));
        // the hand depends only on the cards played (and the rules), not on the cards held
        let mut known = seen.as_deref_mut().map(|d| d[mask as usize].get_or_insert_with(|| SeenPlay { info: hand::detect(&played, flags), floor: None }));
        let info = match &known {
            Some(k) => &k.info,
            None => &hand::detect(&played, flags),
        };
        if !keep_kickers && info.scoring.len() != played.len() {
            continue;
        }
        let held_mask = part & !mask;
        // a floor scored with these cards held or more, none of them taking part, is this one
        let (floor, sure, hand_type, asked, parts) = match known.as_ref().and_then(|k| k.floor).filter(|f| held_mask & !f.held == 0) {
            // no held card took part: no pick changed it, `sure` is the floor
            Some(f) => (f.score, f.score, f.hand, f.asked, f.parts),
            None => {
                held.clear();
                held.extend((0..n).filter(|i| held_mask & (1 << i) != 0).map(|i| hand[i]));
                let mut rolls = Watched { rolls: Unlucky, asked: false };
                let o = score::score_detected(b, &played, &held, info.clone(), &mut rolls, false);
                if let Some(k) = known.as_mut().filter(|k| k.floor.is_none() && !o.held_used) {
                    k.floor = Some(SeenFloor { held: held_mask, score: o.score, hand: o.hand, asked: rolls.asked, parts: o.unbalanced });
                }
                (o.score, o.sure, o.hand, rolls.asked, o.unbalanced)
            }
        };
        if let Some(v) = scored.as_deref_mut() {
            v.push((mask, hand_type, parts.0, parts.1, floor, sure));
        }
        surest = surest.max(sure);
        if best.as_ref().is_none_or(|(p, _)| floor > p.floor) {
            best = Some((Play { cards: idx.clone(), hand: hand_type, floor, sure, surest: 0.0, mean: floor }, asked));
        }
    }
    best.map(|(p, asked)| (Play { surest, ..p }, asked))
}

/// A play a search scored (`best_play_among`): its mask, hand, and chips and Mult with every
/// roll failing, before a deck balances them, its floor and what it surely scores (`Play::sure`)
type Scored = (u32, HandType, f64, f64, f64, f64);

/// What's known of a play of a hand (`best_play_of`): its hand, and its floor when no held card
/// took part in it (`Outcome::held_used`)
#[derive(Clone)]
struct SeenPlay {
    info: hand::HandInfo,
    floor: Option<SeenFloor>,
}

/// A play's floor with the cards `held` (a mask) held, none of them taking part: the same with
/// any of them held, and whether it asked for a roll (`Watched`)
#[derive(Clone, Copy)]
struct SeenFloor {
    held: u32,
    score: f64,
    hand: HandType,
    asked: bool,
    /// its chips and Mult before a deck balances them (`Outcome::unbalanced`)
    parts: (f64, f64),
}

/// The best plays of parts of one hand (a decision's: the hand each plan's dig leaves, the
/// cards a junk hand is made of), each part worked out once, each play's hand detected once
/// for every part it's in, and its floor scored once for every part it's in when no held card
/// takes part. The same results as `best_play` of the part's cards.
struct HandParts<'a> {
    b: &'a Board,
    hand: &'a [Card],
    /// what's known of the plays, by mask (nothing for a hand of more than `SHARE_MAX` cards)
    detected: std::cell::RefCell<Vec<Option<SeenPlay>>>,
    done: std::cell::RefCell<Vec<(u32, Option<Play>)>>,
    /// in a hand of more than `SHARE_MAX` cards, each part's best play (`best_play` of its
    /// cards, searched once), by the places asked for
    big: std::cell::RefCell<Vec<(Vec<usize>, Option<Play>)>>,
    /// the whole hand's plays as its search scored them (`best_play_among`; a hand of up to
    /// `SHARE_MAX` cards): their front (`front_of`) is where `playing_on`'s later hands start
    /// (up to `PARTS_MAX` cards)
    scored: std::cell::RefCell<Option<Vec<Scored>>>,
    /// the board each card's discard alone leaves, by card, when they're not all alike
    /// (`card_discards`)
    discards: std::cell::OnceCell<Option<Vec<Board>>>,
}

/// What a hand's searches found (`HandParts`), handed on to a later search of the same hand on
/// the same board (`HandParts::with`): `decide_unseen` values each draw's decision on the hand
/// and board it was decided on, whose parts that decision already searched
#[derive(Default)]
struct Searched {
    /// the hand searched
    hand: Vec<Card>,
    detected: Vec<Option<SeenPlay>>,
    done: Vec<(u32, Option<Play>)>,
    big: Vec<(Vec<usize>, Option<Play>)>,
    scored: Option<Vec<Scored>>,
    discards: std::cell::OnceCell<Option<Vec<Board>>>,
}

/// The largest hand whose decision's own search seeds playing on's shortlist
/// (`HandParts::shortlist`); a bigger one's first later hand searches in full, as before the
/// shared table reached past it (`SHARE_MAX`): seeding it there would move playing on's totals
/// in the cases the shortlist misses (the register), unmeasured on hands that big
const PARTS_MAX: usize = 12;

/// The largest hand `HandParts` shares detections for (2^n of them): every hand whose best play
/// tries every subset (`SUBSETS_MAX`); a bigger one's parts are searched each on its own, once
const SHARE_MAX: usize = SUBSETS_MAX;

impl<'a> HandParts<'a> {
    fn new(b: &'a Board, hand: &'a [Card]) -> Self {
        Self::with(b, hand, Searched::default())
    }

    /// `new`, knowing what an earlier search of the same hand on the same board found (`found`:
    /// its `searched`); the same results, as each is what that search would work out again. Of
    /// another hand nothing is kept (the board isn't checked: the caller's to keep the same)
    fn with(b: &'a Board, hand: &'a [Card], found: Searched) -> Self {
        let size = if hand.len() <= SHARE_MAX { 1 << hand.len() } else { 0 };
        let found = if found.hand.as_slice() == hand && found.detected.len() == size { found } else { Searched::default() };
        let detected = if found.detected.len() == size { found.detected } else { vec![None; size] };
        HandParts { b, hand, detected: std::cell::RefCell::new(detected), done: std::cell::RefCell::new(found.done), big: std::cell::RefCell::new(found.big), scored: std::cell::RefCell::new(found.scored), discards: found.discards }
    }

    /// What its searches found, to hand on (`with`)
    fn searched(self) -> Searched {
        Searched { hand: self.hand.to_vec(), detected: self.detected.into_inner(), done: self.done.into_inner(), big: self.big.into_inner(), scored: self.scored.into_inner(), discards: self.discards }
    }

    /// The board each card's discard alone leaves (the engine's step, `Board::discard`), by
    /// card; `None` when every card's leaves the same jokers, levels, deck size and money (no
    /// joker reads which card was thrown: Green Joker, Ramen, Yorick count any card alike)
    fn card_discards(&self) -> Option<&Vec<Board>> {
        self.discards
            .get_or_init(|| {
                let boards: Vec<Board> = self
                    .hand
                    .iter()
                    .map(|c| {
                        // the count of discards put back: the same for every card, it orders
                        // none of them
                        let mut a = self.b.clone();
                        let _ = a.discard(&[*c]);
                        a.discards_left = self.b.discards_left;
                        a.discards_used = self.b.discards_used;
                        a
                    })
                    .collect();
                let alike = |x: &Board, y: &Board| x.jokers == y.jokers && x.levels == y.levels && x.playing_cards == y.playing_cards && x.dollars == y.dollars;
                boards.iter().any(|x| !alike(x, &boards[0])).then_some(boards)
            })
            .as_ref()
    }

    /// The whole hand's unbeaten plays (`front_of`), once its best play was worked out: the
    /// shortlist `playing_on` starts from (none for a hand of more than `PARTS_MAX` cards)
    fn shortlist(&self) -> Option<Shortlist> {
        self.scored.borrow().clone().filter(|_| self.hand.len() <= PARTS_MAX && front_holds(self.b, self.hand)).map(|v| Shortlist { plays: front_of(v), stale: false })
    }

    /// The whole hand's plays as `best_play` tries them, with their floors (`plays_tried`): its
    /// search's, once the whole hand's best play was worked out, else searched now
    fn plays(&self) -> Vec<Scored> {
        self.scored.borrow().clone().unwrap_or_else(|| plays_tried(self.b, self.hand))
    }

    /// `best_play` of the cards at `idx` (indices into the hand, in order): the play's cards
    /// as indices into the hand
    fn best(&self, idx: &[usize]) -> Option<Play> {
        // (in order: a part searched by mask, as up to `SHARE_MAX` cards are, comes out in the
        // hand's order, which is the order asked only then)
        debug_assert!(idx.windows(2).all(|w| w[0] < w[1]), "parts are asked in the hand's order: {idx:?}");
        let (b, hand) = (self.b, self.hand);
        if hand.len() > SHARE_MAX {
            if let Some((_, p)) = self.big.borrow().iter().find(|e| e.0 == idx) {
                return p.clone();
            }
            let rest: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
            let p = best_play(b, &rest).map(|mut p| {
                p.cards = p.cards.iter().map(|&k| idx[k]).collect();
                p
            });
            self.big.borrow_mut().push((idx.to_vec(), p.clone()));
            return p;
        }
        let rest: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
        let part: u32 = idx.iter().fold(0, |m, &i| m | 1 << i);
        if let Some((_, p)) = self.done.borrow().iter().find(|e| e.0 == part) {
            return p.clone();
        }
        // the whole hand's search notes its plays' chips and Mult for its front
        let whole = part.count_ones() as usize == hand.len();
        let mut scored = Vec::new();
        let found = best_play_among(b, hand, part, true, Some(&mut self.detected.borrow_mut()), None, whole.then_some(&mut scored));
        if whole {
            *self.scored.borrow_mut() = Some(scored);
        }
        let p = found.map(|(mut p, rolled)| {
            // as `best_play_with`: a floor that rolled nothing (the part's other cards held) is
            // its average
            if rolled {
                let rel: Vec<usize> = p.cards.iter().map(|c| idx.iter().position(|i| i == c).unwrap()).collect();
                p.mean = mean_score(b, &rest, &rel, p.floor);
            }
            p
        });
        self.done.borrow_mut().push((part, p.clone()));
        p
    }
}

/// Whether every card of the play `mask` (of `hand`'s first `n`) could score, before running
/// hand detection: Stone cards always do (state_events.lua `G.FUNCS.evaluate_play`); of the
/// rest, 2 or 3 can only all score as one rank (a Pair, Three of a Kind: no flush or straight
/// is that short), and 4 without Four Fingers only as Two Pair or Four of a Kind; 5 without
/// Four Fingers only as a flush (hand.rs `flush`: one suit, a Wild card or Smeared Joker's
/// colours aside), a straight (5 ranks) or two ranks at most (Full House, Five of a Kind).
/// Splash isn't checked here (`best_play` doesn't call this with it). A shortcut:
/// `hand::detect` decides.
fn could_all_score(hand: &[Card], mask: u32, n: usize, flags: hand::RuleFlags) -> bool {
    if flags.splash {
        return true;
    }
    let mut cards = [0usize; 5];
    let mut k = 0;
    for (i, c) in hand.iter().enumerate().take(n) {
        if mask & (1 << i) != 0 && c.enhancement != Some(Enhancement::Stone) {
            if k == 5 {
                return true;
            }
            cards[k] = i;
            k += 1;
        }
    }
    let mut ranks = [0u8; 5];
    for j in 0..k {
        ranks[j] = hand[cards[j]].rank.0;
    }
    match k {
        2 | 3 => ranks[1..k].iter().all(|&r| r == ranks[0]),
        4 if !flags.four_fingers => {
            let mut r = [ranks[0], ranks[1], ranks[2], ranks[3]];
            r.sort_unstable();
            r[0] == r[1] && r[2] == r[3]
        }
        5 if !flags.four_fingers => {
            let mut r = ranks;
            r.sort_unstable();
            let distinct = 1 + r.windows(2).filter(|w| w[0] != w[1]).count();
            let c = |j: usize| &hand[cards[j]];
            let wild = (0..5).any(|j| c(j).enhancement == Some(Enhancement::Wild));
            let one_suit = (1..5).all(|j| if flags.smeared { c(j).suit.is_red() == c(0).suit.is_red() } else { c(j).suit == c(0).suit });
            distinct <= 2 || distinct == 5 || wild || one_suit
        }
        _ => true,
    }
}

/// A play of fewer than 5 cards topped up with junk from the hand, as a free discard (the
/// extra cards don't score, so they cycle out for new draws). Only when kickers can't
/// matter (no Half Joker, Square, Psychic, held-card jokers…), never cards worth holding
/// (Steel, Gold, seals, editions, the suit you hold most of, ranks that pair with another
/// held card), lowest first, and only fillers that leave the score unchanged.
pub fn with_fillers(b: &Board, hand: &[Card], play: &[usize]) -> Vec<usize> {
    let mut out = play.to_vec();
    if out.len() >= 5 || kickers_matter(b) {
        return out;
    }
    let held: Vec<usize> = (0..hand.len()).filter(|i| !out.contains(i)).collect();
    let suit_count = |s: Suit| held.iter().filter(|&&i| hand[i].suit == s).count();
    let flush_suit = Suit::ALL.into_iter().max_by_key(|&s| suit_count(s)).filter(|&s| suit_count(s) >= 3);
    let pairs = |i: usize| held.iter().any(|&k| k != i && hand[k].rank == hand[i].rank);
    let mut junk: Vec<usize> = held
        .iter()
        .copied()
        .filter(|&i| {
            let c = &hand[i];
            c.enhancement.is_none() && c.seal.is_none() && c.edition.is_none() && !c.debuff && Some(c.suit) != flush_suit && !pairs(i)
        })
        .collect();
    junk.sort_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips()));
    let score_of = |idx: &[usize]| {
        let played: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
        let held: Vec<Card> = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
        score::score(b, &played, &held, &mut Unlucky, false).score
    };
    let base = score_of(&out);
    for i in junk {
        if out.len() >= 5 {
            break;
        }
        let mut with = out.clone();
        with.push(i);
        if (score_of(&with) - base).abs() < 1e-6 {
            out = with;
        }
    }
    out
}

/// A first move in a round, for the look-ahead in "Best play".
#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    Play(Vec<usize>),
    Discard(Vec<usize>),
}

/// Every play of 1 to 5 cards, each in the order to play it, face-down cards included: a play
/// holding one is valued over what the card could be, drawn anew in every simulated round
/// (`deal_hidden`), so a play that rests on what the sorts pin (the suit for a Flush) comes out
/// sure and one that hopes for a rank is averaged. Of the first `ALL_MOVES_MAX` cards: a bigger
/// hand's moves are `big_hand_moves`.
pub fn all_plays(hand: &[Card]) -> Vec<Move> {
    let n = hand.len().min(ALL_MOVES_MAX);
    (1u32..(1 << n))
        .filter(|m| m.count_ones() <= 5)
        .map(|m| {
            let mut idx: Vec<usize> = (0..n).filter(|i| m & (1 << i) != 0).collect();
            arrange(hand, &mut idx);
            Move::Play(idx)
        })
        .collect()
}

/// Whether every card in `hand` is face down (and there is one): The House's first hand, The
/// Mark with Pareidolia, or by chance The Wheel or The Fish (blind.lua `Blind:stay_flipped`). The game still allows a play (and
/// a discard) of them, blind.
pub fn all_face_down(hand: &[Card]) -> bool {
    !hand.is_empty() && hand.iter().all(|c| c.face_down)
}

/// Every discard of 1 to 5 cards from the hand (none without a discard left), for screening
/// all of them instead of guessing which are worth simulating. Of the first `ALL_MOVES_MAX`
/// cards, as `all_plays`.
pub fn all_discards(hand: &[Card], discards: i64) -> Vec<Move> {
    if discards <= 0 {
        return vec![];
    }
    let n = hand.len().min(ALL_MOVES_MAX);
    (1u32..(1 << n)).filter(|m| m.count_ones() <= 5).map(|m| Move::Discard((0..n).filter(|i| m & (1 << i) != 0).collect())).collect()
}

/// One simulated round after a first move. `won` is 0 or 1; hands left over and planets count
/// only in a won round.
#[derive(Debug, Clone, Copy, Default)]
pub struct Outcome {
    pub won: f64,
    pub total: f64,
    pub spare: f64,
    pub cash: f64,
    pub planets: f64,
    /// The first hand played for points after a discard (not a junk hand played to dig)
    pub next: Option<(HandType, f64)>,
    /// The most a card drawn this round adds (`RoundGoals::seen`)
    pub seen: f64,
    /// The hand played last in a won round: a Blue Seal's planet is that hand's
    /// (card.lua Card:get_end_of_round_effect, G.GAME.last_hand_played)
    pub last: Option<HandType>,
    /// What the round's hands and discards changed on the jokers, in the round's value
    /// (`RoundGoals::grown`)
    pub grown: Grown,
}

/// What a round's hands and discards changed on the jokers its goals count, in two parts that
/// add up over a round's steps (`RoundGoals::grown`) and combine in its value
/// (`RoundGoals::value`): `long`, the log of the factor they leave the run's value by Ante 8 at
/// (each joker's own factor, multiplied: `JokerWorth::of`), and `ahead`, the shift they give the
/// log-odds of getting through this ante's blinds still ahead (each joker's, added: odds
/// multiply, so the chance stays between none and a sure pass)
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Grown {
    pub long: f64,
    pub ahead: f64,
}

impl std::ops::Add for Grown {
    type Output = Grown;
    fn add(self, o: Grown) -> Grown {
        Grown { long: self.long + o.long, ahead: self.ahead + o.ahead }
    }
}

impl std::ops::Sub for Grown {
    type Output = Grown;
    fn sub(self, o: Grown) -> Grown {
        Grown { long: self.long - o.long, ahead: self.ahead - o.ahead }
    }
}

impl std::ops::AddAssign for Grown {
    fn add_assign(&mut self, o: Grown) {
        *self = *self + o;
    }
}

/// The log-odds of chance `p` (`ln(p / (1 − p))`), for a chance measured on `n` runs or rounds:
/// none and all are taken half a run in, so a shift stays finite and a measured chance of none
/// or all isn't taken as certain
pub fn log_odds(p: f64, n: usize) -> f64 {
    let e = 0.5 / n.max(1) as f64;
    let p = p.clamp(e, 1.0 - e);
    (p / (1.0 - p)).ln()
}

/// The chance at log-odds `x` (`log_odds`' inverse)
pub fn chance_at(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// The chance at log-odds `base` shifted by `by` against shifted by `from` (between none and
/// what a sure pass gives): what a goal's shifts in the blinds still ahead multiply a round by
/// (`RoundGoals::value`, `value::Ahead::share`)
pub fn chance_ratio(base: f64, by: f64, from: f64) -> f64 {
    if by == from {
        return 1.0;
    }
    chance_at(base + by) / chance_at(base + from)
}

/// The rounds numbered `range` after `first` (round i always draws the same cards, whatever
/// the move, so moves compare on the same draws and more rounds can be added later).
pub fn outcomes_after(b: &Board, start: &RoundStart, first: &Move, range: std::ops::Range<usize>, seed: u64, uses: &[Use]) -> Vec<Outcome> {
    range
        .map(|i| {
            let mut rng = Rng::new(seed.wrapping_add(i as u64 * 7919));
            // the face-down cards drawn anew for this round (the same for every move on it)
            let dealt = deal_hidden(b, start, &rng);
            let start = dealt.as_ref().unwrap_or(start);
            // the consumables held, on the cards this round dealt
            let uses = &uses_on(b, uses, &start.hand);
            // a fresh round's blind set per round (its random part drawn from the round's own)
            let set;
            let (b, start) = if start.fresh {
                set = b.at_blind_set_drawn(start, set_seed(&rng));
                (&set.0, &set.1)
            } else {
                (b, start)
            };
            let size = start.hand_size.max(1) as usize;
            let (hands, discards) = (start.hands, start.discards);
            let mut deck = start.deck.clone();
            shuffle(&mut deck, &mut rng);
            let mut hand = start.hand.clone();
            let mut bb = b.clone();
            // the draw owed before the first move (`RoundStart::first_fill`): a hand short since
            // its last draw plays as it is, and the draw after the move fills it
            let fill = start.first_fill(&hand, size);
            draw(&mut bb, &mut hand, &mut deck, fill, &mut flip_stream(&rng));
            bb.hands_left = hands;
            bb.discards_left = discards;
            bb.deck_remaining = deck.len() as i64;
            // valued against the analysis start: the jokers' numbers as the round so far left them
            let mut o = Outcome { grown: bb.goals.as_ref().map_or(Grown::default(), |g| g.at(&bb)), ..Default::default() };
            // the sale the player would make first (Verdant Leaf), so every first move is played
            // on the board the rest of its round is (selling isn't a move of its own yet, gap 15)
            if bb.sale_lifts_blind() {
                let was = jokers_now(&bb);
                o.cash += leaf_sale(&mut bb, &mut hand, &mut deck, size, hands);
                o.grown += grown_since(&bb, &was);
            }
            let next = match first {
                Move::Play(idx) => {
                    let played: Vec<Card> = idx.iter().filter_map(|&k| hand.get(k).copied()).collect();
                    let mut held: Vec<Card> = (0..hand.len()).filter(|k| !idx.contains(k)).map(|k| hand[k]).collect();
                    let was = jokers_now(&bb);
                    o.cash += boss_takes_held(&mut bb, &mut held, &rng);
                    let s = score::score_pressed(&bb, &played, &held, &mut rng, false);
                    o.cash += s.dollars;
                    let total = start.scored + s.score;
                    let _ = bb.after_hand(&s);
                    o.grown += grown_since(&bb, &was);
                    if total >= start.target || hands <= 1 {
                        if total >= start.target {
                            o.won = 1.0;
                            o.spare = (hands - 1) as f64;
                            o.last = Some(s.hand);
                            o.seen = bb.goals.as_ref().map_or(0.0, |g| g.seen_gain(&hand).max(g.carried.iter().map(|e| e.1).fold(0.0, f64::max)));
                            o.planets = seal_planets(&bb, &held);
                            o.cash += won_money(&bb, &held);
                        }
                        o.total = total;
                        return o;
                    }
                    RoundStart { hand: held, deck, hands: hands - 1, discards, scored: total, fresh: false, hidden: None, changes: vec![], short: 0, ..start.clone() }
                }
                Move::Discard(idx) => {
                    let was = jokers_now(&bb);
                    o.cash += bb.discard(&picked(&hand, idx));
                    o.grown += grown_since(&bb, &was);
                    let kept: Vec<Card> = (0..hand.len()).filter(|k| !idx.contains(k)).map(|k| hand[k]).collect();
                    RoundStart { hand: kept, deck, hands, discards: (discards - 1).max(0), fresh: false, hidden: None, changes: vec![], short: 0, ..start.clone() }
                }
            };
            // the rest of the round plays on the board the first move left
            let r = sim_round_uses(&bb, &next, &mut rng, uses);
            o.won = r.won as u8 as f64;
            if r.won {
                o.spare = r.hands_left as f64;
            }
            o.cash += r.money;
            o.total = r.total;
            o.planets = r.planets;
            o.next = r.plays.iter().find(|p| !p.2).map(|p| (p.0, p.1));
            o.last = r.plays.last().map(|p| p.0);
            o.seen = r.seen;
            o.grown += r.grown;
            o
        })
        .collect()
}

/// What the boss does to the cards held once the played ones have left your hand, before the
/// hand scores: The Hook discards 2 of them at random (blind.lua `Blind:press_play`: one
/// when only one is held), through the board's discard step, using no discard
/// (`Board::discard_by`). The money that discard pays.
///
/// Each held card gets a random key from a seed of its own (the round's stream, not drawn from,
/// and the hands left, so each play picks anew) and the card itself; the 2 lowest go. Any two of
/// n are as likely as the game's two picks, the round's draws are left as a discard leaves them,
/// and moves compared on the same round that hold the same cards keep or lose them alike, however
/// the cards sit.
/// The seed a round's setup draws its random picks from (`Board::at_blind_set_drawn`: Madness's
/// victim, Riff-raff's jokers): the round's stream, not drawn from, so its deals stay the same
fn set_seed(rng: &Rng) -> u64 {
    rng.clone().next_u64() ^ 0x626c_696e_6400
}

fn boss_takes_held(b: &mut Board, held: &mut Vec<Card>, rng: &Rng) -> f64 {
    if !b.blind.active("bl_hook") {
        return 0.0;
    }
    let seed = rng.clone().next_u64() ^ 0x484f_4f4b ^ (b.hands_left as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    // a card's key: the seed, the card (`order_key`: every field) and which copy of it this is
    let mut keyed: Vec<(u64, usize)> = held
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let copy = held[..i].iter().filter(|x| *x == c).count();
            let (r, s, rest) = c.order_key();
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for byte in [r.0, s].into_iter().chain(rest.bytes()).chain((copy as u64).to_le_bytes()) {
                h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
            (Rng::new(seed ^ h).next_u64(), i)
        })
        .collect();
    keyed.sort_unstable();
    let mut gone: Vec<usize> = keyed.iter().take(2).map(|k| k.1).collect();
    if gone.is_empty() {
        return 0.0;
    }
    gone.sort_unstable();
    let out: Vec<Card> = gone.iter().rev().map(|&i| held.remove(i)).collect();
    b.discard_by(&out, true)
}

/// The suit worth keeping: the one a suit joker rewards (Wrathful, Greedy, Lusty,
/// Gluttonous, Arrowhead, Onyx Agate, Bloodstone, Rough Gem), else your deck's commonest.
pub fn keep_suit(b: &Board, hand: &[Card], deck: &[Card]) -> Option<Suit> {
    for j in &b.jokers {
        let s = match j.key.as_str() {
            "j_wrathful_joker" | "j_arrowhead" => Some(Suit::Spades),
            "j_greedy_joker" | "j_rough_gem" => Some(Suit::Diamonds),
            "j_lusty_joker" | "j_bloodstone" => Some(Suit::Hearts),
            "j_gluttenous_joker" | "j_onyx_agate" => Some(Suit::Clubs),
            _ => None,
        };
        if s.is_some() {
            return s;
        }
    }
    Suit::ALL.into_iter().max_by_key(|&s| hand.iter().chain(deck).filter(|c| c.suit == s).count())
}

/// Planets the Blue Seal cards still in hand at the end of a won round make: one each while
/// a consumable slot is free (card.lua Card:get_end_of_round_effect: the consumable limit check).
pub fn seal_planets(b: &Board, held: &[Card]) -> f64 {
    let n = held.iter().filter(|c| c.seal == Some(crate::model::Seal::Blue) && !c.debuff).count() as i64;
    n.min(b.planet_slots.max(0)) as f64
}

/// Cards that pay at the end of a won round while still in hand (card.lua
/// Card:get_end_of_round_effect): a Blue Seal's planet while a consumable slot is free, and a
/// Gold card's $3 (`h_dollars`). Blue Seals first, as many as there are free slots.
pub fn pays_at_end(b: &Board, hand: &[Card]) -> Vec<usize> {
    let mut v: Vec<usize> = (0..hand.len())
        .filter(|&i| hand[i].seal == Some(crate::model::Seal::Blue) && !hand[i].debuff)
        .take(b.planet_slots.max(0) as usize)
        .collect();
    let gold: Vec<usize> = (0..hand.len()).filter(|&i| hand[i].enhancement == Some(Enhancement::Gold) && !hand[i].debuff && !v.contains(&i)).collect();
    v.extend(gold);
    v
}

/// The money Gold cards still in hand pay at the end of a won round ($3 each).
pub fn held_dollars(held: &[Card]) -> f64 {
    3.0 * held.iter().filter(|c| c.enhancement == Some(Enhancement::Gold) && !c.debuff).count() as f64
}

/// A play that wins the round now while keeping as many of the cards that pay at round end
/// in hand as it can (none kept: the usual policy decides).
fn win_keeping_seals(b: &Board, hand: &[Card], need: f64) -> Option<Vec<usize>> {
    let keep = pays_at_end(b, hand);
    for r in (1..=keep.len()).rev() {
        let free: Vec<usize> = (0..hand.len()).filter(|i| !keep[..r].contains(i)).collect();
        let cards: Vec<Card> = free.iter().map(|&i| hand[i]).collect();
        if let Some(p) = best_play(b, &cards).filter(|p| p.surest >= need) {
            let v = if p.sure >= need { p.cards } else { surely_winning(&cards, &plays_tried(b, &cards), need)? };
            return Some(v.iter().map(|&j| free[j]).collect());
        }
    }
    None
}

/// The plays `best_play` tries in `hand`, each with its floor (`Scored`)
fn plays_tried(b: &Board, hand: &[Card]) -> Vec<Scored> {
    let n = hand.len().min(SUBSETS_MAX);
    let mut v = Vec::new();
    best_play_among(b, hand, (1 << n) - 1, true, None, None, Some(&mut v));
    v
}

/// The play `mask` (of `hand`'s first `SUBSETS_MAX`) in the order the search plays it (`arrange`)
fn play_of(hand: &[Card], mask: u32) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..hand.len().min(SUBSETS_MAX)).filter(|i| mask & (1 << i) != 0).collect();
    arrange(hand, &mut idx);
    idx
}

/// The finish when the round can be won now: of the plays `best_play` tries that surely win it
/// (`Play::sure` ≥ `need`), the one worth most by the round's goals (`finish_value`: the Blue Seals and
/// Gold cards it leaves in hand, the money it makes, what it leaves the jokers a round changes),
/// the highest floor on a tie. None when no play surely wins.
fn finish_play(b: &Board, g: &RoundGoals, hand: &[Card], need: f64, hands: i64) -> Option<Vec<usize>> {
    finish_among(b, g, hand, need, hands, &plays_tried(b, hand))
}

/// `finish_play` among `plays` (the hand's plays as `plays_tried` gives them)
fn finish_among(b: &Board, g: &RoundGoals, hand: &[Card], need: f64, hands: i64, plays: &[Scored]) -> Option<Vec<usize>> {
    let mut best: Option<(f64, f64, Vec<usize>)> = None;
    for &(mask, _, _, _, floor, _) in plays.iter().filter(|s| s.5 >= need) {
        let idx = play_of(hand, mask);
        // the same `seen` credit and hands left whichever play wins
        let v = finish_value(b, g, hand, &idx, hands, 0.0);
        if best.as_ref().is_none_or(|(bv, bf, _)| v > bv + 1e-12 || (v >= bv - 1e-12 && floor > *bf)) {
            best = Some((v, floor, idx));
        }
    }
    best.map(|x| x.2)
}

/// Of `plays` (the hand's plays as `plays_tried` gives them), the one with the highest floor that
/// surely wins (`Play::sure` ≥ `need`): the finish without the round's goals when the best play
/// by its floor doesn't surely win (The Hook may take the cards it holds for: gap 15)
fn surely_winning(hand: &[Card], plays: &[Scored], need: f64) -> Option<Vec<usize>> {
    let mut best: Option<&Scored> = None;
    for s in plays.iter().filter(|s| s.5 >= need) {
        if best.is_none_or(|b| s.4 > b.4) {
            best = Some(s);
        }
    }
    best.map(|s| play_of(hand, s.0))
}

/// The cards at `idx` in `hand`, in hand order (the game discards them left to right).
fn picked(hand: &[Card], idx: &[usize]) -> Vec<Card> {
    let mut idx = idx.to_vec();
    idx.sort_unstable();
    idx.iter().filter_map(|&i| hand.get(i).copied()).collect()
}

/// Money discarding `idx` from `hand` pays (the engine's discard effects).
pub fn discard_money(b: &Board, hand: &[Card], idx: &[usize]) -> f64 {
    crate::engine::discard_money(b, &picked(hand, idx))
}

/// Money a won round pays at its end for how it was played: Gold cards still in hand
/// (`held_dollars`) and the jokers' (`engine::won_round_money`: Golden Joker, Rocket, Cloud 9,
/// Satellite, Delayed Gratification). The hands left are counted apart (`RoundResult::cash`,
/// `RoundGoals::per_hand`).
fn won_money(b: &Board, held: &[Card]) -> f64 {
    held_dollars(held) + crate::engine::won_round_money(b)
}

/// How often each poker hand can be made at all from a fresh deal of your deck: deal
/// `hand_size` cards, try every 5-card subset, and note every hand it contains. High Card
/// always can; a Straight rarely does without deck-building. Indexed like the hand levels.
pub fn hand_reachability(b: &Board, deck: &[Card], hand_size: usize, samples: usize, seed: u64) -> [f64; 12] {
    let flags = b.rule_flags();
    let mut rng = Rng::new(seed ^ 0x7ea4);
    let mut counts = [0usize; 12];
    let n = hand_size.min(deck.len()).min(10);
    for _ in 0..samples {
        let mut d = deck.to_vec();
        shuffle(&mut d, &mut rng);
        let hand = &d[..n];
        let mut seen: u16 = 0;
        for mask in 1u32..(1 << n) {
            if mask.count_ones() > 5 {
                continue;
            }
            let played: Vec<Card> = (0..n).filter(|i| mask & (1 << i) != 0).map(|i| hand[i]).collect();
            seen |= hand::detect(&played, flags).contains;
        }
        for (h, c) in counts.iter_mut().enumerate() {
            if seen & (1 << h) != 0 {
                *c += 1;
            }
        }
    }
    counts.map(|c| c as f64 / samples.max(1) as f64)
}

/// Fisher–Yates.
pub fn shuffle<T>(v: &mut [T], rng: &mut Rng) {
    for i in (1..v.len()).rev() {
        let j = rng.below(i + 1);
        v.swap(i, j);
    }
}

/// Where a round starts.
#[derive(Debug, Clone, Default)]
pub struct RoundStart {
    /// Cards already in hand (empty for a fresh round).
    pub hand: Vec<Card>,
    /// The draw pile (shuffled by the simulation).
    pub deck: Vec<Card>,
    pub hand_size: i64,
    /// Hands and discards left; a fresh round's before its blind is set (the run's
    /// `round_resets` and what jokers added or removed change there: `at_blind_set` sets the rest)
    pub hands: i64,
    pub discards: i64,
    pub scored: f64,
    pub target: f64,
    /// The round hasn't started: its blind is set as it's played (`Board::at_blind_set`). A round in
    /// progress had it set when it started, its hand empty or not (the save's while the first
    /// cards are dealt).
    pub fresh: bool,
    /// The round's blind (its key; none: a round with no boss rule), set on a fresh round's
    /// board as it's played; a round in progress has it on its board already
    pub blind: String,
    /// That blind disabled (the save's: Chicot's `setting_blind`, Luchador's sale)
    pub disabled: bool,
    /// Juggle Tags held as a fresh round starts (tag.lua `Tag:apply_to_run`, `round_start_bonus`:
    /// each adds its `h_size` for that round and is used up), set by `at_blind_set`: the next
    /// round played's only (`RunState::round_start_tags`)
    pub juggle: i64,
    /// What's known of the face-down cards in `hand`, drawn anew in each simulated round
    /// (`deal_hidden`); none: they stay as they are
    pub hidden: Option<std::sync::Arc<Hidden>>,
    /// A consumable used first on cards that include face-down ones: its effect and the places
    /// in `hand` it goes on, done to the cards each round deals there (`effect_on`, in
    /// `deal_hidden`)
    pub changes: Vec<(CardEffect, Vec<usize>)>,
    /// The cards `hand` lacks of `hand_size` that no draw is owed for before its next play or
    /// discard (`RoundStart::dealt`): a hand the round's last draw left, cards taken from it
    /// since (a consumable that destroys them). The game draws only after a play or a discard
    /// (game.lua `Game:update_hand_played`, state_events.lua
    /// `G.FUNCS.discard_cards_from_highlighted`: both go to the draw-to-hand state, whose
    /// `G.FUNCS.draw_from_deck_to_hand` fills the hand), at once only into an empty hand (game.lua
    /// `Game:update`, the hand-selecting state) or for a hand size raised (cardarea.lua
    /// `CardArea:change_size`: those cards). None: the hand is filled as the round starts (a
    /// fresh round's first deal, or the draw after the move before).
    pub short: usize,
}

/// `hand` with a consumable's card effect done on the cards at `places` (`consumable::apply`) on
/// board `b`, each card's debuff what it is after (`debuff_after`); the cards at `places` seen where the game flips them
/// face up and back (card.lua `Card:use_consumeable`: a tarot that sets their enhancement, rank
/// or suit, and Sigil and Ouija on the whole hand, showing you what they became, and `Card:flip`
/// clears the mark of one drawn flipped: seen, as the save reads it; not a seal, an edition or
/// Cryptid's original), the cards it adds face up (`CardArea:emplace`), the rest as they were:
/// what it does to the cards each simulated round dealt there (gap 25). Not one that
/// destroys cards (none of those is a move: `advise::tarot_values`), nor one a card there can't
/// take (Aura on a card with an edition: the game doesn't let it be used, card.lua
/// `Card:can_use_consumeable`): the hand as it is.
pub fn effect_on(b: &Board, hand: &[Card], effect: CardEffect, places: &[usize]) -> Vec<Card> {
    // never one that takes cards away: each card keeps its own place's flags
    if matches!(effect, CardEffect::Destroy) || places.iter().any(|&p| hand.get(p).is_some_and(|c| !crate::engine::consumable::can_target(effect, c))) {
        return hand.to_vec();
    }
    let mut new = crate::engine::consumable::apply(effect, hand, places);
    let flips = !matches!(effect, CardEffect::Seal(_) | CardEffect::Edition(_) | CardEffect::Copies(_));
    for (i, c) in new.iter_mut().enumerate() {
        match hand.get(i) {
            Some(was) => *c = Card { face_down: was.face_down && !(flips && places.contains(&i)), ..debuff_after(b, was, *c, i) },
            None => c.face_down = false,
        }
    }
    new
}

/// Card `to`, which a consumable made of `was` (at place `i` in hand, on board `b`), with the
/// debuff it has after: where what the boss's rule reads changed (its suit, rank or enhancement,
/// or whether it was played this ante: an enhancement set clears that, `consumable::apply`), the
/// boss's set again on what it became (card.lua `Card:set_base`, `Card:set_ability` call
/// blind.lua `Blind:debuff_card`: The Star's diamonds free under The Club, The Lovers' Wild card
/// debuffed under The Goad, The Tower's Stone card freed under The Plant, any enhancement on a
/// card The Pillar debuffs frees it); a copy (Death, common_events.lua `copy_card`) gets the
/// copied card's, which is that rule on the same card; a seal or an edition alone (`set_seal`,
/// `set_edition`) leaves the one it had
pub fn debuff_after(b: &Board, was: &Card, mut to: Card, i: usize) -> Card {
    if (was.suit, was.rank, was.enhancement, was.played_this_ante) != (to.suit, to.rank, to.enhancement, to.played_this_ante) {
        b.debuff_card(&mut to, i);
    } else {
        to.debuff = was.debuff;
    }
    to
}

/// `uses` with each effect on places (`Use::at`, in the round's starting `hand`) turned into
/// changes of the cards that round dealt there (and the cards it adds), so it still finds them
/// once the hand has moved on; the effect kept beside them (`Placed`), so a draw of the face-down
/// cards puts it on the cards that draw has there (`Use::unplaced`)
fn uses_on(b: &Board, uses: &[Use], hand: &[Card]) -> Vec<Use> {
    uses.iter()
        .map(|u| {
            let mut u = u.clone();
            for (effect, places) in std::mem::take(&mut u.at) {
                u.placed.push(Placed::new(b, hand, effect, &places));
            }
            u
        })
        .collect()
}

/// What you can tell of the face-down cards of a round in progress, for drawing what they could
/// be (gap 25): each one's bounds among the face-up cards in the game's two sorts
/// (`model::sort_bounds`), the face-down cards' order among themselves in both sorts (the game
/// sorts them by their real key, cardarea.lua `CardArea:sort`, card.lua `Card:get_nominal`, and
/// only reorders: a card raised before a switch stays raised, so each can be followed from one
/// sort to the other), and the boss's rule (The Mark turns down only face cards: blind.lua
/// `Blind:stay_flipped`, card.lua `Card:is_face`, every card a face card with Pareidolia). The
/// cards they could be are the unseen ones: the draw pile and the face-down cards themselves.
#[derive(Debug)]
pub struct Hidden {
    /// Each face-down card's place in the round's hand
    pub pos: Vec<usize>,
    /// Its bounds, keys of `Card::sort_nominal`: `[rank sort, suit sort]`, both ends included
    pub bounds: Vec<[(f64, f64); 2]>,
    /// Its place among the face-down cards in each sort, highest first (cards alike in both
    /// keys take their places in the hand's order, the same in both sorts as the game's
    /// tie-break, `unique_val`, is)
    pub place: Vec<[usize; 2]>,
    /// Only a face card can be face down (The Mark without Pareidolia)
    pub face_only: bool,
    /// The cards the analysis drew for them (`advise.rs`): where a hand no longer holds its
    /// one (a consumable used on it without `RoundStart::changes`), that card stays as it is
    pub stand: Vec<Card>,
    /// Each simulated round's draw (by its seed, the cards drawn and the deck it leaves), worked
    /// out once and met by every move tried on that round
    pub drawn: std::sync::Mutex<std::collections::HashMap<DrawKey, Drawn>>,
    /// The furthest `draw` fell back (0 none; 1 without the boss's rule, 2 without their
    /// order too, 3 without any bound), reported as not modelled (`fell_back_note`, in Best
    /// play's tip)
    pub fell_back: std::sync::atomic::AtomicU8,
}

/// Whether pool card j could be face-down card k, given the window in both sorts the cards drawn
/// for the others leave it (`Hidden::draw`; none where their order isn't kept)
type Fits<'a> = dyn Fn(usize, usize, Option<&[(f64, f64); 2]>) -> bool + 'a;

/// The window in both sorts the pool cards drawn for the others (by index) leave face-down card k
/// (`Hidden::draw`)
type Window<'a> = dyn Fn(usize, &[Option<usize>]) -> Option<[(f64, f64); 2]> + 'a;

/// A round's draw of the face-down cards (`deal_hidden`): its seed and which were drawn
type DrawKey = (u64, Vec<bool>);

/// That draw from each deck it was made from: the deck, the cards drawn (one per face-down
/// card) and the deck it leaves
type Drawn = Vec<(Vec<Card>, Vec<Card>, Vec<Card>)>;

/// The most choices a draw of the face-down cards tries before it drops a constraint
/// (`Hidden::draw`): a budget, not a finding; a draw that keeps every constraint always exists
/// (the real cards), and is found in far fewer
const HIDDEN_TRIES: usize = 4096;

/// How many times a draw of the face-down cards redraws each card given the others
/// (`Hidden::draw`): its first pass, card by card, favours what the first cards drawn leave room
/// for; each sweep brings it closer to every assignment that fits being as likely. A budget:
/// on four face-down cards in a chain of orders (K♠ 9♥ 7♣ 4♦, every card face down), the share
/// of draws whose highest card is a Q or better was 0.27 with none, 0.56 with 4, 0.66 with 16
/// and 32 (and 0.64 with 100), measured 2026-11-01 on a copy of the draw.
const HIDDEN_SWEEPS: usize = 16;

impl Hidden {
    /// What's known of the face-down cards of `hand` (the real cards, as the save has them; the
    /// sorts show where each goes, never what it is): those at `slots`, at places `pos` in the
    /// round's hand; `face_only` the boss's rule. `stand` is filled in once the analysis draws.
    pub fn new(hand: &[Card], slots: &[usize], pos: Vec<usize>, face_only: bool) -> Hidden {
        let all = crate::model::sort_bounds(hand);
        let bounds = slots.iter().map(|&i| all[i]).collect();
        let mut place = vec![[0; 2]; slots.len()];
        for (s, p) in [false, true].into_iter().zip(0..2) {
            let mut by: Vec<usize> = (0..slots.len()).collect();
            by.sort_by(|&a, &b| hand[slots[b]].sort_nominal(s).total_cmp(&hand[slots[a]].sort_nominal(s)).then(slots[a].cmp(&slots[b])));
            for (k, &x) in by.iter().enumerate() {
                place[x][p] = k;
            }
        }
        Hidden { pos, bounds, place, face_only, stand: vec![], drawn: Default::default(), fell_back: Default::default() }
    }

    /// What the draws dropped (`fell_back`), said as not modelled; none when nothing was
    pub fn fell_back_note(&self) -> Option<String> {
        let what = match self.fell_back.load(std::sync::atomic::Ordering::Relaxed) {
            0 => return None,
            1 => "the boss's rule",
            2 if self.face_only => "the boss's rule or their order among themselves",
            2 => "their order among themselves",
            _ => "anything known of them",
        };
        Some(format!(
            "Not modelled: a face-down card no card fits as the sorts{} show it (one turned down for another reason than the boss, as in the X-ray Vision challenge, or a Pareidolia sold or debuffed since), so they were drawn without {what} (gap 25)",
            if self.face_only { " and the boss's rule" } else { "" }
        ))
    }

    /// Whether card `c` could be face-down card `k` by its bounds in the sorts
    fn fits(&self, k: usize, c: &Card) -> bool {
        (0..2).all(|s| {
            let (lo, hi) = self.bounds[k][s];
            let x = c.sort_nominal(s == 1);
            lo <= x && x <= hi
        })
    }

    /// Whether card `c` keeps the boss's rule (a face card under The Mark: `Card:is_face(true)`)
    fn boss_allows(&self, c: &Card) -> bool {
        !self.face_only || crate::engine::hand::is_face(c, 0, false, true)
    }

    /// A draw of what the face-down cards marked `live` could be, from `pool` (each card used
    /// once): the cards of `pool` for them, by index. Drawn card by card in the rank sort's
    /// order, each among the cards that fit with those before it (backtracking where none
    /// does), then redrawn one at a time given the others (`HIDDEN_SWEEPS`). Within
    /// `HIDDEN_TRIES` choices: else without the boss's rule (a card turned down for another
    /// reason, as the X-ray Vision challenge's; a Pareidolia sold or debuffed since; the sorts
    /// show the card as it is),
    /// else without their order among themselves too, and else without any bound, so a draw
    /// always comes out.
    fn draw(&self, live: &[bool], pool: &[Card], rng: &mut Rng) -> Vec<usize> {
        let mut seq: Vec<usize> = (0..self.pos.len()).filter(|&k| live[k]).collect();
        seq.sort_by_key(|&k| self.place[k][0]);
        // each pool card's place in both sorts, worked out once: the draw asks it of every card
        // many times
        let nom: Vec<[f64; 2]> = pool.iter().map(|c| [c.sort_nominal(false), c.sort_nominal(true)]).collect();
        // where face-down card `k` keeps both sorts' order with the pool cards drawn for the others
        // (`at`): in each sort, at or above every one drawn below it and at or below every one
        // drawn above it
        let window = |k: usize, at: &[Option<usize>]| -> [(f64, f64); 2] {
            std::array::from_fn(|s| {
                at.iter().enumerate().fold((f64::NEG_INFINITY, f64::INFINITY), |(lo, hi), (o, d)| match *d {
                    Some(d) if o != k => {
                        let y = nom[d][s];
                        if self.place[o][s] < self.place[k][s] { (lo, hi.min(y)) } else { (lo.max(y), hi) }
                    }
                    _ => (lo, hi),
                })
            })
        };
        for level in 0..4 {
            // with no boss's rule in force, level 1 is level 0 again
            if level == 1 && !self.face_only {
                continue;
            }
            // what doesn't change as the others are drawn: each pool card against each face-down
            // card's bounds, and the boss's rule
            let fixed: Vec<Vec<bool>> = (0..self.pos.len())
                .map(|k| {
                    pool.iter()
                        .map(|c| match level {
                            0 => self.fits(k, c) && self.boss_allows(c),
                            1 | 2 => self.fits(k, c),
                            _ => true,
                        })
                        .collect()
                })
                .collect();
            // whether pool card `j` could be face-down card `k`, its order with the others given as
            // their `window` (none past level 1)
            let ok = |k: usize, j: usize, w: Option<&[(f64, f64); 2]>| fixed[k][j] && w.is_none_or(|w| (0..2).all(|s| w[s].0 <= nom[j][s] && nom[j][s] <= w[s].1));
            let window = |k: usize, at: &[Option<usize>]| (level < 2).then(|| window(k, at));
            let mut at: Vec<Option<usize>> = vec![None; self.pos.len()];
            let mut pick = vec![usize::MAX; self.pos.len()];
            let mut used = vec![false; pool.len()];
            let mut budget = HIDDEN_TRIES;
            #[allow(clippy::too_many_arguments)]
            fn dfs(
                i: usize,
                seq: &[usize],
                pool: &[Card],
                ok: &Fits,
                window: &Window,
                at: &mut [Option<usize>],
                pick: &mut [usize],
                used: &mut [bool],
                budget: &mut usize,
                rng: &mut Rng,
            ) -> bool {
                let Some(&k) = seq.get(i) else { return true };
                let w = window(k, at);
                let mut cands: Vec<usize> = (0..pool.len()).filter(|&j| !used[j] && ok(k, j, w.as_ref())).collect();
                shuffle(&mut cands, rng);
                // each kind of card once (a kind with more copies is the likelier first pick)
                let mut tried: Vec<Card> = vec![];
                for j in cands {
                    if *budget == 0 {
                        return false;
                    }
                    if tried.iter().any(|t| t.order_eq(&pool[j])) {
                        continue;
                    }
                    tried.push(pool[j]);
                    *budget -= 1;
                    (used[j], at[k], pick[k]) = (true, Some(j), j);
                    // every card still to draw keeps a card it could be (else this pick is a
                    // dead end, found now rather than deep in the search)
                    let open = seq[i + 1..].iter().all(|&r| {
                        let w = window(r, at);
                        (0..pool.len()).any(|q| !used[q] && ok(r, q, w.as_ref()))
                    });
                    if open && dfs(i + 1, seq, pool, ok, window, at, pick, used, budget, rng) {
                        return true;
                    }
                    (used[j], at[k]) = (false, None);
                }
                false
            }
            if !dfs(0, &seq, pool, &ok, &window, &mut at, &mut pick, &mut used, &mut budget, rng) {
                continue;
            }
            self.fell_back.fetch_max(level as u8, std::sync::atomic::Ordering::Relaxed);
            for _ in 0..HIDDEN_SWEEPS {
                for &k in &seq {
                    let cur = pick[k];
                    let w = window(k, &at);
                    let cands: Vec<usize> = (0..pool.len()).filter(|&j| (j == cur || !used[j]) && ok(k, j, w.as_ref())).collect();
                    let j = cands[rng.below(cands.len())];
                    (used[cur], used[j], pick[k], at[k]) = (false, true, j, Some(j));
                }
            }
            return pick;
        }
        unreachable!("a draw without bounds always comes out")
    }

    /// `hand` with its face-down cards marked `live` drawn anew (`draw`) from `deck` and
    /// themselves, and the deck left: the cards drawn out, the face-down cards' old ones in
    pub fn deal(&self, hand: &[Card], deck: &[Card], live: &[bool], rng: &mut Rng) -> (Vec<Card>, Vec<Card>) {
        debug_assert_eq!(live.len(), self.pos.len());
        let mut pool: Vec<Card> = deck.to_vec();
        for (k, &p) in self.pos.iter().enumerate() {
            if live[k] {
                pool.push(Card { face_down: false, ..hand[p] });
            }
        }
        // in a fixed order, so how the pile happens to be listed never changes the draw
        pool.sort_by(Card::order_cmp);
        let pick = self.draw(live, &pool, rng);
        let mut hand = hand.to_vec();
        let mut taken = vec![false; pool.len()];
        for (k, &p) in self.pos.iter().enumerate() {
            if live[k] {
                hand[p] = Card { face_down: true, ..pool[pick[k]] };
                taken[pick[k]] = true;
            }
        }
        let deck = pool.into_iter().zip(taken).filter(|(_, t)| !t).map(|(c, _)| c).collect();
        (hand, deck)
    }
}

/// The hands the first `n` simulated rounds from `start` meet (`outcomes_after`'s rounds with
/// `seed`): its face-down cards as each draws them (`deal_hidden`)
pub fn hidden_hands(b: &Board, start: &RoundStart, seed: u64, n: usize) -> Vec<Vec<Card>> {
    (0..n).map(|i| hidden_hand(b, start, seed, i)).collect()
}

/// `hidden_hands` drawn on every core: the same hands (a big hand's screen, `BIG_HAND_RANK_DRAWS`)
pub fn hidden_hands_par(b: &Board, start: &RoundStart, seed: u64, n: usize) -> Vec<Vec<Card>> {
    crate::progress::par_map(&(0..n).collect::<Vec<_>>(), |&i| hidden_hand(b, start, seed, i))
}

/// The hand simulated round `i` from `start` meets (`hidden_hands`)
fn hidden_hand(b: &Board, start: &RoundStart, seed: u64, i: usize) -> Vec<Card> {
    deal_hidden(b, start, &Rng::new(seed.wrapping_add(i as u64 * 7919))).map_or_else(|| start.hand.clone(), |s| s.hand)
}

/// `start` with its face-down cards drawn anew (`Hidden`) for one simulated round, from a seed of
/// the round's own (`rng` isn't drawn from, so the round's deals stay as they were and every move
/// tried on round i meets the same cards: drawn once, `Hidden::drawn`); none when it has none
/// to draw. A consumable used first (`RoundStart::changes`) is done to what the round drew, on
/// board `b` (the round's: the boss's debuff set again on what it changes, `debuff_after`).
pub fn deal_hidden(b: &Board, start: &RoundStart, rng: &Rng) -> Option<RoundStart> {
    let h = start.hidden.as_deref()?;
    debug_assert_eq!(h.stand.len(), h.pos.len());
    let live: Vec<bool> = h.pos.iter().zip(&h.stand).map(|(&p, s)| start.hand.get(p).is_some_and(|c| c.order_eq(s))).collect();
    if !live.contains(&true) {
        // nothing to draw: a consumable used first still done, on the hand as it is
        return (!start.changes.is_empty()).then(|| {
            let hand = start.changes.iter().fold(start.hand.clone(), |h, (effect, places)| effect_on(b, &h, *effect, places));
            RoundStart { hand, hidden: None, changes: vec![], ..start.clone() }
        });
    }
    let seed = rng.clone().next_u64() ^ 0x6869_6464_656e;
    let key = (seed, live.clone());
    // the same deck (every move on a round shares it), card for card
    let cached = h.drawn.lock().ok().and_then(|m| m.get(&key).and_then(|v| v.iter().find(|x| x.0 == start.deck).map(|x| (x.1.clone(), x.2.clone()))));
    let (cards, deck) = match cached {
        Some(x) => x,
        None => {
            let (hand, deck) = h.deal(&start.hand, &start.deck, &live, &mut Rng::new(seed));
            let cards: Vec<Card> = h.pos.iter().map(|&p| hand[p]).collect();
            if let Ok(mut m) = h.drawn.lock() {
                m.entry(key).or_default().push((start.deck.clone(), cards.clone(), deck.clone()));
            }
            (cards, deck)
        }
    };
    let mut hand = start.hand.clone();
    for (k, &p) in h.pos.iter().enumerate() {
        if live[k] {
            hand[p] = cards[k];
        }
    }
    // a consumable used first, done to what the round drew
    for (effect, places) in &start.changes {
        hand = effect_on(b, &hand, *effect, places);
    }
    Some(RoundStart { hand, deck, hidden: None, changes: vec![], ..start.clone() })
}

impl RoundStart {
    /// This round in progress with its hand as the round's last draw left it: the cards it lacks
    /// of its hand size wait for the draw after its next play or discard (`short`); an empty
    /// hand is filled at once, and so is a fresh round's (its first deal, state_events.lua
    /// `new_round`)
    pub fn dealt(self) -> RoundStart {
        let short = if self.hand.is_empty() || self.fresh { 0 } else { (self.hand_size.max(1) as usize).saturating_sub(self.hand.len()) };
        RoundStart { short, ..self }
    }

    /// The round in progress at a simulated decision on `hand` (as the round's last draw left
    /// it, `dealt`)
    fn at_decision(hand: &[Card], deck: &[Card], size: usize, hands: i64, discards: i64, scored: f64, target: f64) -> RoundStart {
        RoundStart { hand: hand.to_vec(), deck: deck.to_vec(), hand_size: size as i64, hands, discards, scored, target, fresh: false, ..Default::default() }.dealt()
    }

    /// How many cards the round's first draw fills `hand` to, of `size` (`short`): all of them
    /// into an empty hand
    fn first_fill(&self, hand: &[Card], size: usize) -> usize {
        if hand.is_empty() { size } else { size.saturating_sub(self.short) }
    }

    /// The hands and discards the round is played with on `b`: a fresh round's as its blind is
    /// set on it (`Board::at_blind_set`: the boss's, then Burglar's), a
    /// round in progress its own, set when it started.
    pub fn hands_on(&self, b: &Board) -> (i64, i64) {
        let (_, s) = b.at_blind_set(self);
        (s.hands, s.discards)
    }
}

#[derive(Debug, Clone)]
pub struct RoundResult {
    pub total: f64,
    /// Beaten on score (Mr. Bones' save not counted).
    pub won: bool,
    /// Lost, but Mr. Bones would save it (one save, then he's gone).
    pub saved: bool,
    pub best_hand: f64,
    /// Hands played (type, score, whether it was a junk hand played to dig).
    pub plays: Vec<(HandType, f64, bool)>,
    /// Money earned while scoring (Lucky cards, gold seals, …).
    pub money: f64,
    /// Hands not used when it was won (each pays at cash out).
    pub hands_left: i64,
    /// Discards not used when it was won (a deck that pays for them, Green Deck, pays at cash out)
    pub discards_left: i64,
    /// Planets from Blue Seal cards held at the end of a won round.
    pub planets: f64,
    /// The most a card drawn this round adds (`RoundGoals::seen`)
    pub seen: f64,
    /// What its hands and discards changed on the jokers (`RoundGoals::grown`)
    pub grown: Grown,
}

impl RoundResult {
    /// What the round pays for how it was played, as the game pays it: its `money` (scoring,
    /// discards, the round's end: Gold cards held, the jokers' `calculate_dollar_bonus`) and, won,
    /// `per_hand` for each hand left and `per_discard` for each discard left (state_events.lua
    /// `G.FUNCS.evaluate_round`, `modifiers.money_per_hand` and `money_per_discard`). The blind's
    /// reward and interest aren't how it was played.
    pub fn cash(&self, per_hand: f64, per_discard: f64) -> f64 {
        self.money + if self.won { self.hands_left as f64 * per_hand + self.discards_left as f64 * per_discard } else { 0.0 }
    }
}

/// Cards drawn from the top of `deck` until `hand` holds `size`, each face down where the boss
/// on `b` turns it down (`Board::stays_flipped`: The House's first deal and The Fish's after a
/// play read from the board's round state; The Wheel's rolls on `flips`), else as the deck has it
/// (face up). Then `BlindRules::prepped` cleared, as blind.lua `Blind:drawn_to_hand` does after
/// every draw to hand.
fn draw(b: &mut Board, hand: &mut Vec<Card>, deck: &mut Vec<Card>, size: usize, flips: &mut Rng) {
    while hand.len() < size {
        match deck.pop() {
            Some(mut c) => {
                if b.stays_flipped(&c, hand.len(), flips) {
                    c.face_down = true;
                }
                hand.push(c);
            }
            None => break,
        }
    }
    b.blind.prepped = false;
}

/// The stream The Wheel's rolls on a round's draws come from (`draw`): the round's own, not drawn
/// from (as `deal_hidden`'s), so its deals, rolls and The Hook's picks stay as they were
fn flip_stream(rng: &Rng) -> Rng {
    Rng::new(rng.clone().next_u64() ^ 0x7768_6565_6c66)
}

/// Chance of holding `need` cards of a suit after `draws` digs, each replacing up to 5
/// off-suit cards (a discard, or a junk hand played to dig). Exact hypergeometric DP
/// over (held, suited left in deck, deck size); adapted from balatro-agent's
/// `tools/balatro_state.py::flush_odds`.
pub fn flush_odds(hold: usize, deck_suit: usize, deck_size: usize, hand_size: usize, need: usize, draws: usize) -> f64 {
    use std::cell::RefCell;
    use std::collections::HashMap;
    // The same situations recur constantly across simulations, so cache per thread.
    type Key = (usize, usize, usize, usize, usize, usize);
    thread_local! {
        static CACHE: RefCell<HashMap<Key, f64>> = RefCell::new(HashMap::new());
    }
    let key = (hold, deck_suit, deck_size, hand_size, need, draws);
    if let Some(p) = CACHE.with(|c| c.borrow().get(&key).copied()) {
        return p;
    }
    let p = flush_odds_uncached(hold, deck_suit, deck_size, hand_size, need, draws);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 200_000 {
            c.clear();
        }
        c.insert(key, p);
    });
    p
}

/// The number of ways to choose `k` of `n` (0 when `k > n`)
fn comb(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
}

fn flush_odds_uncached(hold: usize, deck_suit: usize, deck_size: usize, hand_size: usize, need: usize, draws: usize) -> f64 {
    // a BTreeMap: the sum below in a fixed order, so the odds are the same to the last bit
    // every time (a HashMap's order differs per map, and near-ties between plans flipped)
    use std::collections::BTreeMap;
    let mut dist: BTreeMap<(usize, usize, usize), f64> = BTreeMap::from([((hold, deck_suit, deck_size), 1.0)]);
    for _ in 0..draws {
        let mut next: BTreeMap<(usize, usize, usize), f64> = BTreeMap::new();
        for (&(h, s, d), &pr) in &dist {
            let k = 5.min(hand_size.saturating_sub(h)).min(d);
            if h >= need || k == 0 {
                *next.entry((h, s, d)).or_default() += pr;
                continue;
            }
            let total = comb(d, k);
            for x in 0..=k.min(s) {
                let q = comb(s, x) * comb(d - s, k - x) / total;
                if q > 0.0 {
                    *next.entry((h + x, s - x, d - k)).or_default() += pr * q;
                }
            }
        }
        dist = next;
    }
    dist.iter().filter(|((h, _, _), _)| *h >= need).map(|(_, p)| p).sum()
}

/// What the round simulation decided to do with the current hand.
#[derive(Debug, Clone)]
enum Action {
    /// cards, and whether this is a junk hand played only to dig
    Play(Vec<usize>, bool),
    Discard(Vec<usize>),
}

/// Cards that pay at the end of the round while held (`pays_at_end`) stay out of play as long
/// as the rest of the hand keeps you on pace for the target; then the usual policy decides
/// with the rest.
fn decide(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize) -> Action {
    decide_with(b, hand, deck, hands, discards, need, size, None)
}

/// `decide`, with the whole hand's search when it was already made (`HandParts` of `hand`)
#[allow(clippy::too_many_arguments)]
fn decide_with(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize, parts: Option<&HandParts>) -> Action {
    // every simulated decision: where an analysis asked to stop does (`progress`)
    crate::progress::checkpoint();
    let keep = pays_at_end(b, hand);
    if !keep.is_empty() && hands > 1 {
        let free: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i)).collect();
        let cards: Vec<Card> = free.iter().map(|&i| hand[i]).collect();
        if keeps_pace(b, &cards, hands, need) {
            return match decide_cards(b, &cards, deck, hands, discards, need, size.saturating_sub(keep.len()).max(1), true) {
                Action::Play(v, d) => Action::Play(v.into_iter().map(|k| free[k]).collect(), d),
                Action::Discard(v) => Action::Discard(v.into_iter().map(|k| free[k]).collect()),
            };
        }
    }
    decide_cards_with(b, hand, deck, hands, discards, need, size, true, parts, true)
}

/// Whether the best play in `hand` scores more with no discards left than with the board's
/// discards (jokers that pay by discards left, scored by the engine; e.g. Mystic Summit pays
/// with none left, Banner per discard kept).
pub fn scores_more_without_discards(b: &Board, hand: &[Card]) -> bool {
    best_play(b, hand).is_some_and(|p| burn_pays(b, hand, &p.cards))
}

/// Whether `play` (cards of `hand`) scores more with no discards left than with the board's:
/// two scoring passes, cheap enough for every decision.
fn burn_pays(b: &Board, hand: &[Card], play: &[usize]) -> bool {
    burn_scores(b, hand, play).is_some_and(|(now, none)| none > now * 1.001)
}

/// Whether `play` (cards of `hand`) scores differently with no discards left than with the
/// board's (a joker reads the count: Banner, Mystic Summit)
fn burn_reads(b: &Board, hand: &[Card], play: &[usize]) -> bool {
    burn_scores(b, hand, play).is_some_and(|(now, none)| none != now)
}

/// `play` (cards of `hand`) scored with the board's discards left and with none (every roll
/// failing); None with no discards left or nothing played
fn burn_scores(b: &Board, hand: &[Card], play: &[usize]) -> Option<(f64, f64)> {
    if b.discards_left <= 0 || play.is_empty() {
        return None;
    }
    let mut none = b.clone();
    none.discards_left = 0;
    let played: Vec<Card> = play.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !play.contains(i)).map(|i| hand[i]).collect();
    let info = hand::detect(&played, b.rule_flags());
    let now = score::score_detected(b, &played, &held, info.clone(), &mut Unlucky, false).score;
    Some((now, score::score_detected(&none, &played, &held, info, &mut Unlucky, false).score))
}

/// On the round's last hand, playing `play` (cards of `hand`) now as the chase counts a hand
/// (`reached`): `need` times its share of `Play::mean`'s rolls that reach it, each at the better
/// of the discards left and none where `burns` (as a plan's completions, `aim_score`)
fn play_reach(b: &Board, hand: &[Card], play: &[usize], need: f64, burns: bool) -> f64 {
    let played: Vec<Card> = play.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !play.contains(i)).map(|i| hand[i]).collect();
    let info = hand::detect(&played, b.rule_flags());
    let none = (burns && b.discards_left > 0).then(|| {
        let mut z = b.clone();
        z.discards_left = 0;
        z
    });
    let mut rolls = Rng::new(0x6d65616e);
    let mut w = Watched { rolls: Unlucky, asked: false };
    let floor = score::score_detected(b, &played, &held, info.clone(), &mut w, false).score;
    let n = if w.asked { MEAN_ROLLS } else { 1 };
    let hits = (0..n)
        .filter(|_| {
            let same = rolls.clone();
            let x = if w.asked { score::score_detected(b, &played, &held, info.clone(), &mut rolls, false).score } else { floor };
            let y = none.as_ref().map_or(x, |z| x.max(score::score_detected(z, &played, &held, info.clone(), &mut same.clone(), false).score));
            y >= need
        })
        .count();
    need * hits as f64 / n as f64
}

/// The heuristic play/discard policy (labelled as a heuristic everywhere it shows):
/// - while safe, cash the discard that pays most (`discard_money`); the last discard right
///   before the round ends;
/// - with discards left and a best play that scores more with none left, discard first;
/// - play the best hand if it surely wins, if it's the last hand and no discard is left, or if
///   playing on keeps pace (`playing_on`: its average now, then the best play of the same
///   cards each hand after, on the board the hand before leaves);
/// - otherwise, if digging for a hand (`aims`: a flush, a straight, a straight flush, one
///   more of a rank, a Full House) is worth more than playing on (`chase`: the round's
///   expected points, dig by dig as this policy plays it; on the round's last hand its chance
///   to reach the score still needed), throw away the cards outside it:
///   with a discard, or by playing them as a junk hand once discards are gone;
/// - otherwise discard the cards outside the best play and hope to improve it.
///
/// Cards that add to the best play while held (`held_value`) are never thrown away.
/// `burn`: whether to check if burning discards pays (off inside that check itself, which digs
/// whatever the pace and cashes no paying discard).
#[allow(clippy::too_many_arguments)]
fn decide_cards(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize, burn: bool) -> Action {
    decide_cards_with(b, hand, deck, hands, discards, need, size, burn, None, true)
}

/// `decide_cards`, with the best plays of `hand` and its parts already worked out (`given`);
/// `reach`: on the round's last hand, whether plans are weighed by reaching the score needed
/// (`chase`), else in points
#[allow(clippy::too_many_arguments)]
fn decide_cards_with(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize, burn: bool, given: Option<&HandParts>, reach: bool) -> Action {
    match policy_core(b, hand, deck, hands, discards, need, size, burn, given, reach, None) {
        PolicyOut::Act(a) => a,
        PolicyOut::Values(_) => unreachable!("asked for no values"),
    }
}

/// What the policy's own estimates make of each of `acts` on `hand` (`policy_core`'s values):
/// the round's expected points from here (on its last hand, the score still needed times the
/// chance to reach it; with `reach` off, in points there too, as the policy weighs it where
/// nothing reaches), as its decision weighs playing on against a plan's chase. A heuristic
/// (the policy's), labelled where it decides: the face-down choice (`decide_unseen`).
#[allow(clippy::too_many_arguments)]
fn policy_estimates(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize, given: Option<&HandParts>, reach: bool, acts: &[Action]) -> Vec<f64> {
    match policy_core(b, hand, deck, hands, discards, need, size, true, given, reach, Some(acts)) {
        PolicyOut::Values(v) => v,
        PolicyOut::Act(_) => unreachable!("asked for values"),
    }
}

/// The policy's decision (`decide_cards_with`), or with `ask` its estimate of each decision asked
/// (`policy_estimates`)
enum PolicyOut {
    Act(Action),
    Values(Vec<f64>),
}

/// `decide_cards_with`'s body; `ask`: the decisions to value instead of deciding (`policy_estimates`)
#[allow(clippy::too_many_arguments)]
fn policy_core(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize, burn: bool, given: Option<&HandParts>, reach: bool, ask: Option<&[Action]>) -> PolicyOut {
    use PolicyOut::Act;
    // the best plays of this hand and of its parts (`HandParts`)
    let own;
    let parts = match given {
        Some(p) => p,
        None => {
            own = HandParts::new(b, hand);
            &own
        }
    };
    let whole: Vec<usize> = (0..hand.len()).collect();
    // every card here is read as seen: a round's face-down cards come as one draw of what they
    // could be (`decide_unseen`, gap 25 (a))
    let Some(best) = parts.best(&whole) else { return ask.map_or(Act(Action::Play(vec![], false)), |a| PolicyOut::Values(vec![0.0; a.len()])) };
    let play_best = Action::Play(with_fillers(b, hand, &best.cards), false);
    // Discards that pay money (`discard_money`, the engine's discard effects): while the
    // round is safe (on pace with this hand), cash the discard that pays most among those
    // whose remaining hand still keeps you on pace (held cards count too: Steel, Baron's
    // Kings). With one discard left, cash it right before the winning hand (or the last
    // one): more paying cards may come by then. A threshold, not yet weighed against the
    // round's value (`RoundGoals`): see the register in design.md.
    // playing on (`playing_on`): what the best play scores this hand and the best play of the
    // same cards each hand after, on the board the one before leaves; worked out once, when needed
    let on_cell = std::cell::OnceCell::new();
    let on = || *on_cell.get_or_init(|| playing_on_with(b, hand, &best, hands, true, true, parts.shortlist()));
    let all: Vec<usize> = (0..hand.len()).collect();
    let now = discards > 1 || best.surest >= need || hands <= 1;
    // the decisions below that aren't weighed by the round's points are made only when deciding
    let deciding = ask.is_none();
    if deciding && burn && now && discards > 0 && discard_money(b, hand, &all) > 0.0 && on() >= need {
        let n = hand.len().min(12);
        let key = |v: &[usize]| {
            let mut k: Vec<_> = v.iter().map(|&i| hand[i].order_key()).collect();
            k.sort();
            k
        };
        // every discard of up to 5 cards that pays, and in which every card adds money (a card
        // that pays nothing only thins your hand): most money first, fewer cards on a tie,
        // then a fixed order
        let mut paying: Vec<(Vec<usize>, f64)> = (1u32..(1 << n))
            .filter(|m| m.count_ones() <= 5)
            .filter_map(|m| {
                let v: Vec<usize> = (0..n).filter(|i| m & (1 << i) != 0).collect();
                let money = discard_money(b, hand, &v);
                let all_pay = || (0..v.len()).all(|k| {
                    let less: Vec<usize> = v.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, &i)| i).collect();
                    discard_money(b, hand, &less) < money
                });
                (money > 0.0 && all_pay()).then_some((v, money))
            })
            .collect();
        paying.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.len().cmp(&y.0.len())).then_with(|| key(&x.0).cmp(&key(&y.0))));
        let keeps_pace = |v: &[usize]| {
            let rest: Vec<Card> = (0..hand.len()).filter(|i| !v.contains(i)).map(|i| hand[i]).collect();
            keeps_pace(b, &rest, hands, need)
        };
        if let Some((v, _)) = paying.into_iter().take(16).find(|(v, _)| keeps_pace(v)) {
            return Act(Action::Discard(v));
        }
    }
    // the last hand is played now only when no discard is left: a discard is allowed with one
    // hand left (button_callbacks.lua `G.FUNCS.can_discard`: discards left and cards picked)
    // and draws back to hand size (`G.FUNCS.draw_from_deck_to_hand`), so throwing cards
    // outside the best play can only add to it; a hand that can't win yet digs first
    if deciding && best.surest >= need {
        // the winning play worth most by the round's goals, when it has goals (`finish_play`);
        // else the best play, or when it doesn't surely win, the best that does
        return Act(match b.goals.as_ref().and_then(|g| finish_among(b, g, hand, need, hands, &parts.plays())) {
            Some(v) => Action::Play(v, false),
            None if best.sure >= need => play_best,
            None => surely_winning(hand, &parts.plays(), need).map_or(play_best, |v| Action::Play(v, false)),
        });
    }
    if deciding && ((hands <= 1 && discards <= 0) || deck.is_empty()) {
        return Act(play_best);
    }
    // When your best play scores more with no discards left (the engine knows which jokers
    // pay that way), discard before playing. Which cards: what the policy digs with when it's
    // behind (a flush chase when the odds are there, else what's outside the best play). The
    // discards are spent anyway, so keeping a draw costs nothing; being on pace only decides
    // whether to spend a discard, which the burn already has (inside it the policy digs whatever
    // the pace, toward the score still needed). Nothing to throw: play.
    if deciding && burn && discards > 0 && burn_pays(b, hand, &best.cards) {
        if let Action::Discard(v) = decide_cards(b, hand, deck, hands, discards, need, size, false) {
            return Act(Action::Discard(v));
        }
        return Act(play_best);
    }
    // Dig plans (`aims`): every hand you could dig for, each chased dig by dig as this policy
    // would (`chase`): the one whose chase is worth most, when that beats playing on (a later
    // hand played on is worth its average, `later`).
    // inside the burn's dig (`burn` off) the discards are spent anyway: never on pace
    let on_pace = burn && on() >= need;
    let later = if hands > 1 { (on() - best.mean) / (hands - 1) as f64 } else { best.mean };
    // the cards worth holding (`held_value`), worked out only when needed
    let held_cell = std::cell::OnceCell::new();
    let held_keep = || held_cell.get_or_init(|| held_value(b, hand, &best.cards, deck));
    // When on pace, a hand is only chased if it's one card away and the best hand would spend
    // 2+ of its cards (playing it would break the draw); then only if clearly better
    // (`ON_PACE_MARGIN`).
    let eligible = |a: &Aim| {
        let breaks_draw = best.hand != a.hand && best.cards.iter().filter(|i| a.keep.contains(i)).count() >= 2;
        let one_away = a.groups.iter().map(|g| g.1).sum::<usize>() == 1;
        !on_pace || (breaks_draw && one_away)
    };
    // on the round's last hand only reaching the score needed counts (`chase`, `reached`); a
    // decision asked as if behind by any amount (an unbounded `need`: playing on's alternative)
    // has no score to reach and weighs points
    let last = (reach && hands <= 1 && need.is_finite()).then_some(need);
    // on the last hand a made hand is played after the discards left are burned when that scores
    // more (the burn above), as no later hand needs them: where a joker reads the count
    // (`burn_reads`), a plan's completions are scored so
    let burns = last.is_some() && burn_reads(b, hand, &best.cards);
    let all = aims(b, hand, deck, last.is_some());
    // When the score reads held cards (a card's own held effect, a joker on held cards,
    // Blackboard; the pile's cards too, as they may be drawn), what a plan's completion and the
    // hand its dig leaves hold beside them counts: the cards the dig leaves and those drawn into
    // the rest (`Ends`, `held_beside`)
    let pile_sorted: Vec<Card> = if score::held_read(b, hand) || score::held_read(b, deck) {
        let mut v = deck.to_vec();
        v.sort_by(|p, q| p.order_cmp(q));
        v
    } else {
        vec![]
    };
    let ends = (!pile_sorted.is_empty()).then_some(Ends { pile: &pile_sorted, size });
    let best_played: Vec<Card> = best.cards.iter().map(|&i| hand[i]).collect();
    let best_held: Vec<Card> = (0..hand.len()).filter(|i| !best.cards.contains(i)).map(|i| hand[i]).collect();
    // the policy's own discard when no chase goes, off pace with a discard left (`fallback_toss`;
    // none on pace or with none left), worked out once, when needed
    let toss_cell = std::cell::OnceCell::new();
    let toss = || toss_cell.get_or_init(|| if on_pace || discards <= 0 { vec![] } else { fallback_toss(parts, &best.cards, held_keep()) });
    // what that move spends, charged to playing on (`ChaseRound::spent`): the engine's whole step
    // for its discard (`dig_board`), as a dig's is charged to the chase
    let spent_cell = std::cell::OnceCell::new();
    let spent = || *spent_cell.get_or_init(|| spent_by(b, hand, toss(), &best.cards));
    let round = ChaseRound { pile: deck.len(), size, hands, discards, need, best: best.mean, later, on_pace, takes: score::hook_takes(b, size), stays: 0, lift: 1.0, spent: 1.0, last, now: last.map_or(0.0, |n| play_reach(b, hand, &best.cards, n, burns)) };
    // the first dig for each plan (`dig_for`), and the hand it leaves (its best play: what a
    // failed chase is left with)
    type Dig = Option<(Vec<usize>, bool)>;
    let digs: Vec<std::cell::OnceCell<Dig>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let dig_of = |k: usize| digs[k].get_or_init(|| dig_for(parts, &all[k].keep, held_keep(), discards));
    // what a plan keeping `keep` holds beside its completion after a dig throwing `thrown`: with
    // `ends`, every card the dig leaves in hand, else the cards worth holding it leaves (the
    // policy's own digs never throw them)
    let kept_after = |keep: &[usize], thrown: &[usize]| -> Vec<usize> {
        let hk = held_keep();
        match ends {
            Some(_) => {
                // the cards worth holding first: a dig never throws them (`held_beside`)
                let left = |i: &usize| !keep.contains(i) && !thrown.contains(i);
                hk.iter().copied().filter(left).chain((0..hand.len()).filter(|i| left(i) && !hk.contains(i))).collect()
            }
            None => hk.iter().copied().filter(|i| !thrown.contains(i)).collect(),
        }
    };
    let kept_of = |k: usize| -> Vec<usize> { kept_after(&all[k].keep, dig_of(k).as_ref().map_or(&[][..], |d| &d.0[..])) };
    // the board a plan's first dig leaves when it discards (`dig_board`): every hand of its
    // chase after that dig is scored there; `b` where the discard changes nothing a score reads
    let boards: Vec<std::cell::OnceCell<Option<Board>>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let board_of = |k: usize| -> &Board {
        boards[k]
            .get_or_init(|| match dig_of(k) {
                Some((v, false)) => dig_board(b, hand, v, &best.cards),
                _ => None,
            })
            .as_ref()
            .unwrap_or(b)
    };
    let moved = |k: usize| !std::ptr::eq(board_of(k), b);
    // a plan's score, on its middle completion (the screen) or all of them (`aim_score`),
    // worked out once each
    let quick: Vec<std::cell::OnceCell<f64>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let full: Vec<std::cell::OnceCell<f64>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let full_of = |k: usize| *full[k].get_or_init(|| aim_score(board_of(k), &all[k], hand, &kept_of(k), ends, last, burns));
    // the screen: there every plan gets the full estimate, as one completion can't tell its share
    let quick_of = |k: usize| if last.is_some() { full_of(k) } else { *quick[k].get_or_init(|| aim_quick(board_of(k), &all[k], hand, &kept_of(k), ends)) };
    // the hand a dig throwing `thrown` leaves, played: its best play's average on the board the
    // dig leaves (`on`; `moved`: not `b`)
    let left_after = |thrown: &[usize], on: &Board, moved: bool| -> f64 {
        let rest: Vec<usize> = (0..hand.len()).filter(|i| !thrown.contains(i)).collect();
        parts.best(&rest).map_or(0.0, |p| {
            let played: Vec<Card> = p.cards.iter().map(|&i| hand[i]).collect();
            let held: Vec<Card> = rest.iter().filter(|i| !p.cards.contains(i)).map(|&i| hand[i]).collect();
            // its average on the board the dig leaves: the search's, by what that board's
            // score of it is to this one's
            let mean = if moved { p.mean * ratio(b, on, &played, &held) } else { p.mean };
            match ends {
                // the cards drawn after the dig held beside its play, on `HELD_DRAWS` draws
                Some(e) if !thrown.is_empty() => {
                    let count = e.size.saturating_sub(rest.len());
                    let own = score::score(on, &played, &held, &mut Unlucky, false).score;
                    let drawn = (0..HELD_DRAWS)
                        .map(|s| {
                            let mut h = held.clone();
                            h.extend(held_draws(e.pile, &[], count, s));
                            score::score(on, &played, &h, &mut Unlucky, false).score
                        })
                        .sum::<f64>()
                        / HELD_DRAWS as f64;
                    mean + drawn - own
                }
                _ => mean,
            }
        })
    };
    let left: Vec<std::cell::OnceCell<f64>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let left_of = |k: usize| *left[k].get_or_init(|| left_after(dig_of(k).as_ref().map_or(&[][..], |d| &d.0[..]), board_of(k), moved(k)));
    // each plan's chase (`chase`): with the lesser plans it keeps alive (a lesser hand whose
    // cards it keeps), always on their full estimate (a middle completion can be far off: a
    // flush draw's may be a straight flush), and the hand it leaves
    // a lesser plan made inside plan `k`'s chase, in the hand `k`'s dig leaves (with `ends`): its
    // completion beside `k`'s other kept cards and what `k`'s dig leaves
    let in_chase: std::cell::RefCell<Vec<((usize, usize), f64)>> = Default::default();
    let lesser_of = |k: usize, j: usize| -> f64 {
        // its own estimate where its board is `k`'s (the boards are each plan's dig's)
        if ends.is_none() && std::ptr::eq(board_of(k), board_of(j)) {
            return full_of(j);
        }
        if let Some(e) = in_chase.borrow().iter().find(|e| e.0 == (k, j)) {
            return e.1;
        }
        // `k`'s own cards first, then what its dig leaves (`held_beside` keeps them in that order);
        // on the board `k`'s dig leaves
        let kept: Vec<usize> = match ends {
            Some(_) => all[k].keep.iter().copied().filter(|i| !all[j].keep.contains(i)).chain(kept_of(k)).collect(),
            None => kept_of(j),
        };
        let v = aim_score(board_of(k), &all[j], hand, &kept, ends, last, burns);
        in_chase.borrow_mut().push(((k, j), v));
        v
    };
    // a later hand played on after the dig: playing on's, by what the board the dig leaves
    // makes of the best play now against this board (`ratio`)
    let lift_of = |k: usize| if moved(k) { ratio(b, board_of(k), &best_played, &best_held) } else { 1.0 };
    let run = |k: usize, score: &dyn Fn(usize) -> f64| {
        let lesser: Vec<(&Aim, f64)> = (0..all.len())
            .filter(|&j| (all[j].hand as usize) > (all[k].hand as usize) && all[j].keep.iter().all(|i| all[k].keep.contains(i)))
            .map(|j| (&all[j], lesser_of(k, j)))
            .collect();
        let stays = held_keep().iter().filter(|i| !all[k].keep.contains(i)).count();
        chase(&all[k], score(k), left_of(k), &lesser, &ChaseRound { stays, lift: lift_of(k), spent: spent(), ..round })
    };
    if let Some(acts) = ask {
        // a play: its own cards' average now, as played (`mean_score`), then a later hand played
        // on each hand after, as playing on counts them: the same for every play, the cards held
        // standing in for the ones drawn (on the round's last hand, its share of rolls that
        // reach the score needed, `play_reach`)
        let play_value = |v: &[usize]| -> f64 {
            if v.is_empty() {
                return 0.0;
            }
            match last {
                Some(n) => play_reach(b, hand, v, n, burns),
                None => mean_score(b, hand, v, f64::NEG_INFINITY) + (hands - 1).max(0) as f64 * later,
            }
        };
        // a dig (a discard, or a junk hand once they're gone): the better of playing on from the
        // hand it leaves and the plans it keeps whole, chased from that dig (`chase`, the cards
        // held for their value beside the plan held back: `ChaseRound::stays`), on the board it
        // leaves, screened as the decision screens them (the best of a family, the finalists).
        // Not the decision's on-pace rules (`eligible`, `ON_PACE_MARGIN`): they decide whether
        // to start a dig, not what a dig made is worth (applying them lost 1.0 to 1.3 points of
        // round wins on the bench's Mark boards and won 0.4 and 0.5 on two 4-hand, 3-discard boss
        // boards, 2026-11-07). So on a hand it sees, the policy's
        // own decision is valued at least as high as playing its best play
        let dig_value = |v: &[usize], junk: bool| -> f64 {
            let after = if junk { None } else { dig_board(b, hand, v, &best.cards) };
            let on = after.as_ref().unwrap_or(b);
            let lift = if after.is_some() { ratio(b, on, &best_played, &best_held) } else { 1.0 };
            let left = left_after(v, on, after.is_some());
            let rest: Vec<usize> = (0..hand.len()).filter(|i| !v.contains(i)).collect();
            let h = if junk { hands - 1 } else { hands };
            // a junk hand on the round's last (not made: the policy plays its best there) is a play
            if h <= 0 {
                return play_value(v);
            }
            let stay = reached(left, last) + (h - 1) as f64 * later * lift;
            if v.is_empty() {
                return stay;
            }
            let chase_of = |k: usize, full: bool| {
                let kept = kept_after(&all[k].keep, v);
                let score = if full || last.is_some() { aim_score(on, &all[k], hand, &kept, ends, last, burns) } else { aim_quick(on, &all[k], hand, &kept, ends) };
                let lesser: Vec<(&Aim, f64)> = (0..all.len())
                    .filter(|&j| (all[j].hand as usize) > (all[k].hand as usize) && all[j].keep.iter().all(|i| all[k].keep.contains(i)))
                    .map(|j| {
                        let kj: Vec<usize> = match ends {
                            Some(_) => all[k].keep.iter().copied().filter(|i| !all[j].keep.contains(i)).chain(kept.iter().copied()).collect(),
                            None => kept_after(&all[j].keep, v),
                        };
                        (&all[j], aim_score(on, &all[j], hand, &kj, ends, last, burns))
                    })
                    .collect();
                let stays = held_keep().iter().filter(|i| rest.contains(i) && !all[k].keep.contains(i)).count();
                // playing on's charge (`spent`) only decides `go`, which isn't read here
                chase(&all[k], score, left, &lesser, &ChaseRound { stays, lift, spent: 1.0, ..round })
            };
            // screened on one completion each, the best few on the full estimate (as deciding)
            let mut fit: Vec<(usize, f64)> = (0..all.len()).filter(|&k| all[k].keep.iter().all(|i| rest.contains(i))).map(|k| (k, chase_of(k, false).value)).collect();
            fit.sort_by(|a, c| c.1.total_cmp(&a.1));
            let mut families: Vec<(HandType, i32)> = vec![];
            fit.retain(|(k, _)| match all[*k].family {
                Some(f) if families.contains(&(all[*k].hand, f)) => false,
                Some(f) => {
                    families.push((all[*k].hand, f));
                    true
                }
                None => true,
            });
            fit.truncate(AIM_FINALISTS);
            fit.into_iter().map(|(k, _)| chase_of(k, true)).map(|c| c.value).fold(stay, f64::max)
        };
        return PolicyOut::Values(
            acts.iter()
                .map(|a| match a {
                    Action::Play(v, false) => play_value(v),
                    Action::Play(v, true) => dig_value(v, true),
                    Action::Discard(v) => dig_value(v, false),
                })
                .collect(),
        );
    }
    // screened on one completion each; the best few get the full estimate
    // (a plan with nothing to throw can't be chased)
    let mut plan = (0..all.len()).filter(|&k| eligible(&all[k]) && dig_of(k).is_some()).map(|k| (k, run(k, &quick_of))).collect::<Vec<_>>();
    plan.sort_by(|a, c| c.1.value.total_cmp(&a.1.value));
    // the best of a family only (`Aim::family`): its variants mustn't take every slot
    let mut families: Vec<(HandType, i32)> = vec![];
    plan.retain(|(k, _)| match all[*k].family {
        Some(f) if families.contains(&(all[*k].hand, f)) => false,
        Some(f) => {
            families.push((all[*k].hand, f));
            true
        }
        None => true,
    });
    plan.truncate(AIM_FINALISTS);
    let plan: Vec<(usize, Chase)> = plan.into_iter().map(|(k, _)| (k, run(k, &full_of))).collect();
    // on the last hand, where neither playing now nor any plan reaches the score needed on its
    // estimate, the estimates can't tell the plans apart, though the round still can be won (a
    // completion counts only its plan's own hand, not a pair among the cards drawn or the cards
    // held a later dig gathers, and the policy's fallback discard isn't valued, gap 6): they are
    // weighed in points, which say which comes nearest
    if last.is_some() && round.now <= 0.0 && !plan.is_empty() && plan.iter().all(|(_, c)| c.value <= 0.0) {
        return Act(decide_cards_with(b, hand, deck, hands, discards, need, size, burn, Some(parts), false));
    }
    let plan = plan
        .into_iter()
        .filter(|(_, c)| c.go)
        .max_by(|a, c| a.1.value.total_cmp(&c.1.value));
    if let Some((k, _)) = plan
        && let Some((v, junk)) = dig_of(k).clone()
    {
        return Act(if junk { Action::Play(v, true) } else { Action::Discard(v) });
    }
    if on_pace {
        return Act(play_best);
    }
    if !toss().is_empty() {
        return Act(Action::Discard(toss().clone()));
    }
    Act(play_best)
}

/// Whether `hand`'s best play keeps you on pace for `need` over `hands` hands (`playing_on`):
/// the one pace every decision uses
fn keeps_pace(b: &Board, hand: &[Card], hands: i64, need: f64) -> bool {
    best_play(b, hand).is_some_and(|p| playing_on(b, hand, &p, hands) >= need)
}

/// What playing on scores over `hands` hands from now: the best play now, then each later hand
/// the best play of the same cards again (taken to come back; among the plays the last full
/// search left unbeaten, `later_best`) on the board the hand before
/// leaves (`Board::after_hand`, the game's own step: what the jokers grow or fade by, Spare
/// Trousers, Green Joker, Ice Cream; the hand's level, The Arm; the round's memory of it, Card
/// Sharp, Obelisk; the hands left, Acrobat; The Mouth's lock, not The Eye's ban), each at its average
/// (`Play::mean`). The policy's pace and its "playing on" (`chase`); with nothing changing,
/// the best play times the hands, as before. A card a joker changes (Vampire, Hiker) comes back
/// as it was.
fn playing_on(b: &Board, hand: &[Card], best: &Play, hands: i64) -> f64 {
    playing_on_with(b, hand, best, hands, true, true, None)
}


/// `playing_on`, `shortcut`: whether a play whose jokers and score didn't move is taken to stay
/// the best without searching again (off only to test that it changes nothing); `shortlist`:
/// whether a later hand searches only the plays the last full search left unbeaten
/// (`later_best`; off only to test it); `start`: the plays this hand's own search left unbeaten
/// on `b` (`HandParts::shortlist`), where they begin (else the first later hand searches in full)
fn playing_on_with(b: &Board, hand: &[Card], best: &Play, hands: i64, shortcut: bool, shortlist: bool, start: Option<Shortlist>) -> f64 {
    let cards = |play: &[usize]| -> (Vec<Card>, Vec<Card>) {
        (play.iter().map(|&i| hand[i]).collect(), (0..hand.len()).filter(|i| !play.contains(i)).map(|i| hand[i]).collect())
    };
    if hands <= 1 {
        return best.mean;
    }
    let (mut total, mut last) = (best.mean, best.mean);
    // a start found on a board with other hands left than the round's (Dusk, Acrobat) is redone
    let mut short: Option<Shortlist> = if shortlist { start.map(|s| Shortlist { stale: s.stale || b.hands_left != hands, ..s }) } else { None };
    let mut board = b.clone();
    // the round counts down from `hands` (Acrobat and Dusk read it), as the round's own loop sets it
    board.hands_left = hands;
    let mut play = best.cards.clone();
    let (mut played, mut held) = cards(&play);
    let mut o = score::score(&board, &played, &held, &mut Unlucky, false);
    for _ in 1..hands {
        let jokers = board.jokers.clone();
        // The Eye's ban on the hands played this round isn't carried: the cards held stand in for
        // the ones drawn, which make other hands, and the real decision at that hand sees them
        // (with it, every later hand under The Eye was a lesser one of these cards and the player
        // dug where it should play, 8-9 points lower on the benchmark); without it a growth The
        // Eye would stop is counted. The Mouth's lock is carried: it holds whatever is drawn.
        let eye = board.blind.eye_seen;
        let apart = board.after_hand(&o);
        board.blind.eye_seen = eye;
        if let Some(s) = short.as_mut() {
            s.stale |= apart;
        }
        // the same play again when nothing it scores by moved (the jokers as they were, and it
        // scores as it did: what The Eye or The Arm do to it would show): it stays the best,
        // without searching again
        let again = score::score(&board, &played, &held, &mut Unlucky, false);
        if shortcut && board.jokers == jokers && again.score == o.score {
            total += last;
            o = again;
            continue;
        }
        let next = if shortlist { later_best(&board, hand, &mut short) } else { best_play(&board, hand) };
        let Some(next) = next else { break };
        (total, last) = (total + next.mean, next.mean);
        play = next.cards;
        (played, held) = cards(&play);
        o = score::score(&board, &played, &held, &mut Unlucky, false);
    }
    total
}

/// Whether a play's score follows its chips and Mult (`front_of`): not when The Hook's pick
/// changes it, which an estimate averages (gap 15) while the chips and Mult are one pick's; then
/// every later hand searches in full
fn front_holds(b: &Board, hand: &[Card]) -> bool {
    !score::hook_pick_matters(b, hand)
}

/// Of each hand type, the plays (masks, ascending) no other play of it beats in both chips and
/// Mult, from a search's `scored` plays: by chips (most first), then Mult, then the earlier mask
/// (the one the search keeps on a tie), each kept while its Mult beats every play before it
fn front_of(mut scored: Vec<Scored>) -> Vec<u32> {
    scored.sort_by(|x, y| (x.1 as usize).cmp(&(y.1 as usize)).then(y.2.total_cmp(&x.2)).then(y.3.total_cmp(&x.3)).then(x.0.cmp(&y.0)));
    let mut plays = Vec::new();
    let mut top: Option<(HandType, f64)> = None;
    for &(mask, h, _, mult, _, _) in &scored {
        if top.is_none_or(|(th, tm)| th != h || mult > tm) {
            plays.push(mask);
            top = Some((h, mult));
        }
    }
    plays.sort_unstable();
    plays
}

/// The plays a later hand in `playing_on` searches (masks, ascending), and whether the board
/// moved plays of one hand apart since they were found (`Board::after_hand`): then the next
/// later hand searches in full again
struct Shortlist {
    plays: Vec<u32>,
    stale: bool,
}

/// `best_play` of a later hand in `playing_on`, among the plays the last full search left
/// unbeaten: of each hand type, those no other play of it beats in both chips and Mult (the
/// pareto front). The board one hand on adds the same to every play (its jokers' growth or
/// fading: +Chips, +Mult, ×Mult; a hand type's level, The Arm, Card Sharp, Obelisk, Supernova
/// to every play of it), which keeps a beaten play beaten (chips and Mult before a deck balances
/// them: Plasma Deck). Searched in full again when a hand's step changed the board in more than
/// that (`Board::after_hand`: a hand's level, The Arm; a joker gone; the last hand with a joker
/// that retriggers played cards). Still missed: a joker effect that differs by play scored after
/// a joker whose numbers moved, unless both add (a ×Mult that differs by play after a growing
/// +Mult: Flower Pot, Seeing Double, The Duo to The Tribe; a +Mult that differs by play after a
/// growing ×Mult: Jolly to Crafty, Half Joker after Obelisk, Vampire, Loyalty Card): a budget,
/// not a finding (register).
fn later_best(b: &Board, hand: &[Card], short: &mut Option<Shortlist>) -> Option<Play> {
    let n = hand.len().min(SUBSETS_MAX);
    if short.as_ref().is_some_and(|s| s.stale) {
        *short = None;
    }
    let mut scored = Vec::new();
    let found = match short.as_ref() {
        Some(s) => best_play_among(b, hand, (1 << n) - 1, true, None, Some(&s.plays), None),
        None => best_play_among(b, hand, (1 << n) - 1, true, None, None, Some(&mut scored)),
    };
    if short.is_none() && front_holds(b, hand) {
        *short = Some(Shortlist { plays: front_of(scored), stale: false });
    }
    // its mean as `best_play_with` works it out
    found.map(|(mut p, rolled)| {
        p.mean = if rolled || n < hand.len() { mean_score(b, hand, &p.cards, p.floor) } else { p.floor };
        p
    })
}

/// The first dig for a plan keeping `keep`: up to 5 of the cards outside it, never those worth
/// holding (`held`), thrown with a discard (in `throw_order`: what their discard leaves the
/// round's later hands first, then the lowest); once discards are gone, played as a junk hand:
/// the best hand the cards it may throw make (so it still scores), topped up with the lowest of
/// them to 5. The cards it removes from the hand, and whether it's a junk hand; none when
/// there's nothing to throw.
fn dig_for(parts: &HandParts, keep: &[usize], held: &[usize], discards: i64) -> Option<(Vec<usize>, bool)> {
    let hand = parts.hand;
    let free: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i) && !held.contains(i)).collect();
    if free.is_empty() {
        return None;
    }
    if discards > 0 {
        let mut toss = throw_order(parts, &free);
        toss.truncate(5);
        return Some((toss, false));
    }
    let mut toss = free.clone();
    toss.sort_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips()));
    toss.truncate(5);
    let mut dig: Vec<usize> = parts.best(&free).map(|p| p.cards).unwrap_or_default();
    for i in toss {
        if dig.len() >= 5 {
            break;
        }
        if !dig.contains(&i) {
            dig.push(i);
        }
    }
    Some((dig, true))
}

/// The cards `free` (of `parts.hand`) in the order a discard throws them: the lowest first,
/// except where which card is thrown changes what a score reads (`HandParts::card_discards`:
/// the jokers' `discard` context, card.lua `Card:calculate_joker`, e.g. Hit the Road's ×Mult
/// for a Jack, Castle's Chips for its suit); there first the cards whose discard alone leaves
/// the best play of the cards that stay (those outside `free`, with the rest of them held)
/// scoring most, so an effect every card has alike (Green Joker's) changes nothing; then the
/// lowest.
fn throw_order(parts: &HandParts, free: &[usize]) -> Vec<usize> {
    let hand = parts.hand;
    let mut out = free.to_vec();
    out.sort_by(|a, c| hand[*a].rank.chips().total_cmp(&hand[*c].rank.chips()));
    let Some(after) = parts.card_discards() else { return out };
    let stay: Vec<usize> = (0..hand.len()).filter(|i| !free.contains(i)).collect();
    let Some(r) = (!stay.is_empty()).then(|| parts.best(&stay)).flatten() else { return out };
    let played: Vec<Card> = r.cards.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = stay.iter().filter(|i| !r.cards.contains(i)).map(|&i| hand[i]).collect();
    // each board scored once (Hit the Road: a Jack's and the rest's)
    let mut seen: Vec<(usize, f64)> = vec![];
    let mut value = |i: usize| match seen.iter().find(|(j, _)| after[*j].jokers == after[i].jokers && after[*j].levels == after[i].levels && after[*j].dollars == after[i].dollars) {
        Some(&(_, v)) => v,
        None => {
            let v = score::score(&after[i], &played, &held, &mut Unlucky, false).score;
            seen.push((i, v));
            v
        }
    };
    let values: Vec<(usize, f64)> = out.iter().map(|&i| (i, value(i))).collect();
    let at = |i: &usize| values.iter().find(|v| v.0 == *i).map_or(0.0, |v| v.1);
    out.sort_by(|a, c| at(c).total_cmp(&at(a)));
    out
}

/// The board discarding `thrown` (of `hand`) leaves, for the hands after it: what the engine's
/// step (`Board::discard`) does to the board's state (the jokers' `discard` context, card.lua
/// `Card:calculate_joker`: Hit the Road, Castle, Green Joker's Mult; Burnt Joker's level; the
/// money it pays; the count of discards left, which Banner's Chips and Mystic Summit's Mult
/// read). Off pace with a discard left, the policy's alternative to a dig is a discard too when
/// it has something to throw, charged the same way (`spent_by`, `ChaseRound::spent`): charging
/// the dig's side alone cost the `discards` board (Banner, Mystic Summit) 2.5 to 16 points
/// (design.md gap 8). `None` where the cards change nothing a score reads (the jokers, levels and deck size the same, and
/// `probe`, a play of `hand`, scoring the same on it).
fn dig_board(b: &Board, hand: &[Card], thrown: &[usize], probe: &[usize]) -> Option<Board> {
    let mut after = b.clone();
    let _ = after.discard(&picked(hand, thrown));
    if after.jokers != b.jokers || after.levels != b.levels || after.playing_cards != b.playing_cards {
        return Some(after);
    }
    if after.dollars == b.dollars && after.discards_left == b.discards_left {
        return None;
    }
    let played: Vec<Card> = probe.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !probe.contains(i)).map(|i| hand[i]).collect();
    (ratio(b, &after, &played, &held) != 1.0).then_some(after)
}

/// The policy's discard when no chase goes (its caller checks it's off pace with a discard
/// left): the cards outside `best` (the best play) and `keep` (those worth holding), in the order
/// a discard throws them (`throw_order`), up to 5. Empty: it plays the best play and keeps its
/// discards.
fn fallback_toss(parts: &HandParts, best: &[usize], keep: &[usize]) -> Vec<usize> {
    let free: Vec<usize> = (0..parts.hand.len()).filter(|i| !best.contains(i) && !keep.contains(i)).collect();
    let mut v = throw_order(parts, &free);
    v.truncate(5);
    v
}

/// What discarding `toss` (of `hand`; none: no discard) does to `best`, a play of `hand`: the
/// board it leaves (`dig_board`, the engine's whole step) against `b`, as a factor (`ratio`); 1
/// where it changes nothing a score reads. The policy's fallback discard, charged to playing on
/// as a dig's discard is charged to its chase (`ChaseRound::spent`).
fn spent_by(b: &Board, hand: &[Card], toss: &[usize], best: &[usize]) -> f64 {
    if toss.is_empty() {
        return 1.0;
    }
    let held: Vec<usize> = (0..hand.len()).filter(|i| !best.contains(i)).collect();
    dig_board(b, hand, toss, best).map_or(1.0, |after| ratio(b, &after, &picked(hand, best), &picked(hand, &held)))
}

/// What board `on` makes of a play (`played`, with `held`) against board `b`, as a factor (1
/// where `b` scores it nothing)
fn ratio(b: &Board, on: &Board, played: &[Card], held: &[Card]) -> f64 {
    let now = score::score(b, played, held, &mut Unlucky, false).score;
    if now <= 0.0 {
        return 1.0;
    }
    score::score(on, played, held, &mut Unlucky, false).score / now
}

thread_local! {
    /// Futures per alternative for the oracle player (0: off, the default)
    static ORACLE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// In futures played by the policy alone (`policy_outcomes`): the oracle stays out
    static IN_ROLLOUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Decisions the oracle made in this thread (tests)
    #[cfg(test)]
    static ORACLE_DECISIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Decisions made with a face-down card in hand in this thread (`decide_unseen`; tests)
    #[cfg(test)]
    static UNSEEN_DECISIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Each valuation `decide_unseen` made in this thread (tests)
    #[cfg(test)]
    static UNSEEN_VALUED: std::cell::RefCell<Vec<UnseenValued>> = const { std::cell::RefCell::new(vec![]) };
}

/// One valuation `decide_unseen` made (tests): the draws' actions, each draw's hand and pile,
/// every action's summed estimate, the vote's ranking and the valuation's pick
#[cfg(test)]
#[derive(Clone)]
struct UnseenValued {
    acts: Vec<Action>,
    draws: Vec<(Vec<Card>, Vec<Card>)>,
    sum: Vec<f64>,
    vote: Vec<usize>,
    pick: usize,
}

/// A yardstick for the simulated player and for picks a change moved (`advise::Options::judge`),
/// never used by the advice itself: with `rollouts` > 0, every
/// decision in this thread's simulated rounds that isn't a win on the table compares the
/// policy's move with the alternatives from the hand's structure (`oracle_moves`), each on
/// `rollouts` futures played on by the policy, and takes one only when it's clearly better on
/// the same futures (paired, 2 standard errors). Hundreds of times slower than the policy:
/// for measuring how much it leaves on the table (`tests/player.rs`). Per thread: rounds run
/// on other threads (`round_odds` from 16 rounds, the advice's `par_map` workers) play without
/// it unless they set it themselves, as `Options::judge` does in each worker. With a face-down
/// card in hand it can't see it either: it weighs the player's decision on the draws once,
/// outside them (`oracle_unseen`).
pub fn set_oracle(rollouts: usize) {
    ORACLE.with(|o| o.set(rollouts));
}

/// `outcomes_after` played on by the policy alone, with the oracle off in them: the futures of
/// the oracle's alternatives, and the policy's own look-ahead (`play_on_instead`), whose
/// estimate is the policy's even in a round the oracle plays. The flag goes back to what it
/// was, so a look-ahead inside one of the oracle's futures doesn't switch the oracle back on
/// for the rest of that future.
fn policy_outcomes(b: &Board, start: &RoundStart, first: &Move, range: std::ops::Range<usize>, seed: u64, uses: &[Use]) -> Vec<Outcome> {
    without_oracle(|| outcomes_after(b, start, first, range, seed, uses))
}

/// `f` run with the oracle out of it (`IN_ROLLOUT`), the flag back to what it was after
fn without_oracle<T>(f: impl FnOnce() -> T) -> T {
    // restored on drop: also when a stopped analysis unwinds through here (`progress::checkpoint`)
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            IN_ROLLOUT.with(|x| x.set(self.0));
        }
    }
    let _restore = Restore(IN_ROLLOUT.with(|x| x.replace(true)));
    f()
}

/// Whether the oracle decides here (`set_oracle`: on, and not inside a future it plays)
fn oracle_on() -> bool {
    ORACLE.with(|o| o.get()) >= 2 && !IN_ROLLOUT.with(|x| x.get())
}

#[allow(clippy::too_many_arguments)]
fn oracle_decide(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, scored: f64, target: f64, size: usize, uses: &[Use], rng: &mut Rng) -> Option<Action> {
    if !oracle_on() || hand.is_empty() {
        return None;
    }
    let base = decide(b, hand, deck, hands, discards, target - scored, size);
    let alts = oracle_alts(b, hand, deck, hands, discards, &base);
    let start = RoundStart::at_decision(hand, deck, size, hands, discards, scored, target);
    oracle_pick(b, &start, base, alts, uses, rng)
}

/// The oracle's alternatives to the policy's decision `base` on `hand`: the hand's structure
/// (`oracle_moves`) and, with goals that count jokers, the play the policy's look-ahead would
/// weigh for growth beside `base`'s play (`sparing_play`), as the policy's own rounds do
fn oracle_alts(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, base: &Action) -> Vec<Move> {
    let spare = match (base, b.goals.as_ref()) {
        (Action::Play(v, _), Some(g)) => g.jokers_now(b).and_then(|was| sparing_play(b, g, &was, hand, &plays_tried(b, hand), v, hands)).map(|s| Move::Play(s.1)),
        _ => None,
    };
    oracle_moves(b, hand, deck, discards).into_iter().chain(spare).collect()
}

/// The oracle's choice between the policy's decision `base` and the alternatives `alts` from
/// `start` (a round in progress): each played first on the oracle's futures (`ORACLE` of them,
/// the policy playing on), one taken over `base` only when it's clearly better on the same
/// futures (`paired_pick`). None when there's nothing else to weigh.
fn oracle_pick(b: &Board, start: &RoundStart, base: Action, alts: Vec<Move>, uses: &[Use], rng: &mut Rng) -> Option<Action> {
    let r = ORACLE.with(|o| o.get());
    let norm = |m: &Move| match m {
        Move::Play(v) => (0, { let mut v = v.clone(); v.sort(); v }),
        Move::Discard(v) => (1, { let mut v = v.clone(); v.sort(); v }),
    };
    let mut cands: Vec<Move> = vec![match &base {
        Action::Play(v, _) => Move::Play(v.clone()),
        Action::Discard(v) => Move::Discard(v.clone()),
    }];
    for m in alts {
        if !cands.iter().any(|x| norm(x) == norm(&m)) {
            cands.push(m);
        }
    }
    if cands.len() <= 1 {
        return None;
    }
    #[cfg(test)]
    ORACLE_DECISIONS.with(|x| x.set(x.get() + 1));
    let seed = rng.next_u64();
    let g = b.goals.clone();
    let vals: Vec<Vec<f64>> = cands
        .iter()
        .map(|m| policy_outcomes(b, start, m, 0..r, seed, uses).iter().map(|o| g.as_ref().map_or(o.won, |g| g.value(o))).collect())
        .collect();
    let pick = paired_pick(&vals).unwrap_or(0);
    if pick == 0 {
        return Some(base);
    }
    // its only play is the best hand (`oracle_moves`): a play for points, not a dig
    Some(match cands.swap_remove(pick) {
        Move::Play(v) => Action::Play(v, false),
        Move::Discard(v) => Action::Discard(v),
    })
}

/// The oracle's alternatives, kept simple and apart from the policy's own plans: playing the
/// best hand, and (with a discard) throwing away up to 5 of the lowest cards outside each set
/// worth keeping: each suit you hold 2+ of, each straight's run of ranks (`straight_runs`) you
/// hold all but 1 or 2 of (one card a rank), the cards that pair, the biggest rank group, the
/// best play; each also with the cards that pay at round end kept. By the game's hand rules
/// as `aims` reads them (misc_functions.lua): suits as flushes count them (`hand::is_suit`:
/// Wild cards, Smeared Joker), runs of 4 with Four Fingers and with Shortcut's gaps, ranks as
/// straights and sets see them (`hand::card_id`: Stone cards have none).
fn oracle_moves(b: &Board, hand: &[Card], deck: &[Card], discards: i64) -> Vec<Move> {
    let mut out = vec![];
    let Some(best) = best_play(b, hand) else { return out };
    out.push(Move::Play(with_fillers(b, hand, &best.cards)));
    out.extend(structure_discards(b, hand, deck, discards, best.cards));
    out
}

/// The discards a hand's structure gives (`oracle_moves`, `big_hand_moves`): up to 5 of the
/// lowest cards outside each set worth keeping (each suit you hold 2+ of, each straight's run
/// you hold all but 1 or 2 of, the cards that pair, the biggest rank group, the best play
/// `best`), each also with the cards that pay at round end kept. None without a discard or a
/// card to draw.
fn structure_discards(b: &Board, hand: &[Card], deck: &[Card], discards: i64, best: Vec<usize>) -> Vec<Move> {
    let mut out = vec![];
    if discards <= 0 || deck.is_empty() {
        return out;
    }
    let f = b.rule_flags();
    let need = if f.four_fingers { 4 } else { 5 };
    let mut sets: Vec<Vec<usize>> = vec![];
    for s in Suit::ALL {
        let g: Vec<usize> = (0..hand.len()).filter(|&i| hand::is_suit(&hand[i], s, false, true, f.smeared)).collect();
        if g.len() >= 2 && !sets.contains(&g) {
            sets.push(g);
        }
    }
    let ids: Vec<i32> = (0..hand.len()).map(|i| hand::card_id(&hand[i], i)).collect();
    for run in straight_runs(need, f.shortcut).iter() {
        let g: Vec<usize> = run.iter().filter_map(|&r| (0..hand.len()).find(|&i| ids[i] == if r == 1 { 14 } else { r })).collect();
        if g.len() + 2 >= need && !sets.contains(&g) {
            sets.push(g);
        }
    }
    let count = |r: i32| ids.iter().filter(|&&x| x == r).count();
    let paired: Vec<usize> = (0..hand.len()).filter(|&i| count(ids[i]) >= 2).collect();
    if let Some(&top) = paired.iter().max_by_key(|&&i| (count(ids[i]), ids[i])) {
        sets.push((0..hand.len()).filter(|&i| ids[i] == ids[top]).collect());
        sets.push(paired);
    }
    sets.push(best);
    let pay = pays_at_end(b, hand);
    for keep in sets {
        for with_pay in [false, true] {
            if with_pay && pay.is_empty() {
                continue;
            }
            let mut toss: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i) && !(with_pay && pay.contains(i))).collect();
            toss.sort_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips()).then(a.cmp(&c)));
            toss.truncate(5);
            if !toss.is_empty() {
                out.push(Move::Discard(toss));
            }
        }
    }
    out
}

/// A hand the player could dig for: the cards of the hand it keeps, the hand they'd make, and
/// what the draw pile must still give (each: the cards that fit, how many are needed).
#[derive(Debug)]
struct Aim {
    /// plans that differ only in how many singles they keep beside a group (the group's rank):
    /// one of them at most gets the full estimate
    family: Option<i32>,
    hand: HandType,
    keep: Vec<usize>,
    groups: Vec<(Vec<Card>, usize)>,
}

/// Every hand worth digging for, from the game's own hand rules (misc_functions.lua
/// `get_flush`, `get_straight`, `get_X_same`, with Four Fingers, Shortcut and Smeared Joker): a
/// flush of each suit with 2+ cards of it; each straight with all but 1 or 2 of its ranks in
/// hand (one card of each kept), and each straight flush (a suit's flush part with a straight,
/// misc_functions.lua `evaluate_poker_hand`); one more of a rank you hold 2+ of (Three, Four,
/// Five of a Kind); a Full House from Two Pair; one more of a single card's rank beside a Pair
/// (Two Pair) or Three of a Kind (a Full House). Hands already made aren't aims (the best play
/// has them), but on the round's last hand (`last`) a flush made is: one more of its suit, as the
/// sets and straights already have a plan up from a made one (a bigger set, another run).
fn aims(b: &Board, hand: &[Card], deck: &[Card], last: bool) -> Vec<Aim> {
    use HandType::*;
    let f = b.rule_flags();
    let need = if f.four_fingers { 4 } else { 5 };
    let mut out = vec![];
    let suited = |c: &Card, s: Suit| hand::is_suit(c, s, false, true, f.smeared);
    for s in Suit::ALL {
        let keep: Vec<usize> = (0..hand.len()).filter(|&i| suited(&hand[i], s)).collect();
        let fits: Vec<Card> = deck.iter().filter(|c| suited(c, s)).copied().collect();
        if keep.len() >= 2 && keep.len() < need && fits.len() + keep.len() >= need {
            let n = need - keep.len();
            out.push(Aim { family: None, hand: Flush, keep, groups: vec![(fits, n)] });
        } else if last && keep.len() >= need && keep.len() < hand.len() && !fits.is_empty() {
            // on the round's last hand, a flush made: one more of its suit, the best five of them
            // played (a flush short of the score needed has no other way up)
            out.push(Aim { family: None, hand: Flush, keep, groups: vec![(fits, 1)] });
        }
    }
    // a card's rank as straights see it (Stone cards have none); an Ace is also 1
    let id = |c: &Card, i: usize| hand::card_id(c, i);
    let ids: Vec<i32> = (0..hand.len()).map(|i| id(&hand[i], i)).collect();
    let deck_ids: Vec<i32> = deck.iter().enumerate().map(|(i, c)| id(c, 100 + i)).collect();
    let runs = straight_runs(need, f.shortcut);
    let rank_fits = |m: i32, ok: &dyn Fn(&Card) -> bool| -> Vec<Card> {
        let m = if m == 1 { 14 } else { m };
        deck.iter().zip(&deck_ids).filter(|(c, r)| **r == m && ok(c)).map(|(c, _)| *c).collect()
    };
    // of two copies of a rank, the one that counts as the suit you hold most (a Wild counts as
    // every suit), so a straight flush stays possible
    let held_of = Suit::ALL.map(|s| hand.iter().filter(|c| suited(c, s)).count());
    let pref: Vec<usize> = hand.iter().map(|c| Suit::ALL.iter().zip(held_of).filter(|(s, _)| suited(c, **s)).map(|(_, n)| n).max().unwrap_or(0)).collect();
    let copy_of = |r: i32, ok: &dyn Fn(&Card) -> bool| -> Option<usize> {
        let r = if r == 1 { 14 } else { r };
        (0..hand.len()).filter(|&i| ids[i] == r && ok(&hand[i])).max_by_key(|&i| (pref[i], std::cmp::Reverse(i)))
    };
    let any = |_: &Card| true;
    // `copy_of` for every rank a run can have (1 to 14), worked out once
    let copies = |ok: &dyn Fn(&Card) -> bool| -> [Option<usize>; 15] { std::array::from_fn(|r| if r == 0 { None } else { copy_of(r as i32, ok) }) };
    let any_copy = copies(&any);
    // runs keeping the same cards and missing one rank are one draw (open-ended: either end's
    // rank completes it); one missing two ranks needs both
    // (the cards kept, the ranks one away, the pairs of ranks two away)
    type Draw = (Vec<usize>, Vec<i32>, Vec<Vec<i32>>);
    let mut straights: Vec<Draw> = vec![];
    for run in runs.iter() {
        let mut keep: Vec<usize> = run.iter().filter_map(|&r| any_copy[r as usize]).collect();
        keep.sort();
        keep.dedup();
        let missing: Vec<i32> = run.iter().copied().filter(|&r| any_copy[r as usize].is_none()).map(|r| if r == 1 { 14 } else { r }).collect();
        let s = straights.iter().position(|s| s.0 == keep);
        match (missing.len(), s) {
            (1, Some(s)) if !straights[s].1.contains(&missing[0]) => straights[s].1.push(missing[0]),
            (1, None) => straights.push((keep, missing, vec![])),
            (2, Some(s)) if !straights[s].2.contains(&missing) => straights[s].2.push(missing),
            (2, None) => straights.push((keep, vec![], vec![missing])),
            _ => {}
        }
    }
    let one_away: Vec<Vec<usize>> = straights.iter().filter(|s| !s.1.is_empty()).map(|s| s.0.clone()).collect();
    for (keep, one, two) in straights {
        if !one.is_empty() {
            let g: Vec<Card> = one.iter().flat_map(|&m| rank_fits(m, &any)).collect();
            if !g.is_empty() {
                out.push(Aim { family: None, hand: Straight, keep, groups: vec![(g, 1)] });
            }
            continue;
        }
        // two ranks away: the likeliest way to fill it (the most cards that fit), unless these
        // cards are part of a straight one rank away (that draw is the likelier one)
        if one_away.iter().any(|k| keep.iter().all(|i| k.contains(i))) {
            continue;
        }
        let best = two
            .iter()
            .map(|m| m.iter().map(|&r| (rank_fits(r, &any), 1)).collect::<Vec<(Vec<Card>, usize)>>())
            .filter(|g| g.iter().all(|x| !x.0.is_empty()))
            .max_by_key(|g| g.iter().map(|x| x.0.len()).product::<usize>());
        if let Some(groups) = best {
            out.push(Aim { family: None, hand: Straight, keep, groups });
        }
    }
    // Straight flushes, by the game's rule: a flush part and a straight part found apart
    // (misc_functions.lua `evaluate_poker_hand`; with Four Fingers each is 4 of the 5 cards
    // played), so a play holds `need` cards of the suit and at most 5 − need others. For each
    // suit and straight: the suit's copy of each rank, another only within that allowance, else
    // the rank is drawn; then the suit's other cards in hand, as many as there's room for (the
    // flush part). One card away, its fits are the cards that make a straight flush with what's
    // kept (`hand::detect`); two away, a card of each missing rank, as many of them of the suit
    // as the flush part still lacks.
    let off_suit = 5 - need;
    for s in Suit::ALL {
        let in_suit = |c: &Card| suited(c, s);
        // the play needs `need` cards of the suit, and digs bring at most 2 of them
        if hand.iter().filter(|c| in_suit(c)).count() + 2 < need {
            continue;
        }
        let suit_copy = copies(&in_suit);
        let mut plans: Vec<(Vec<usize>, Vec<i32>)> = vec![];
        for run in runs.iter() {
            let (mut keep, mut missing, mut off) = (vec![], vec![], 0);
            for &r in run.iter() {
                if let Some(i) = suit_copy[r as usize] {
                    keep.push(i);
                } else if let Some(i) = any_copy[r as usize].filter(|_| off < off_suit) {
                    keep.push(i);
                    off += 1;
                } else {
                    missing.push(if r == 1 { 14 } else { r });
                }
            }
            if missing.len() > 2 || keep.len() + missing.len() > 5 {
                continue;
            }
            // the flush part: the suit's other cards, the highest, while there's room (a slot for
            // each card drawn, at least one; a run held whole of the suit has none: no card makes
            // a straight flush with it, as 6 cards are no hand: misc_functions.lua `get_flush`)
            let mut more: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i) && in_suit(&hand[*i])).collect();
            more.sort_by(|&a, &c| hand[c].rank.chips().total_cmp(&hand[a].rank.chips()).then(a.cmp(&c)));
            more.truncate((5 - keep.len()).saturating_sub(missing.len().max(1)));
            keep.extend(more);
            keep.sort();
            keep.dedup();
            if !plans.iter().any(|p| p.0 == keep && p.1 == missing) {
                plans.push((keep, missing));
            }
        }
        let mut one_away: Vec<Vec<usize>> = vec![];
        for (keep, _) in plans.iter().filter(|p| p.1.len() <= 1) {
            // open-ended: both ends keep the same cards, one plan
            if one_away.contains(keep) {
                continue;
            }
            let kept: Vec<Card> = keep.iter().map(|&i| hand[i]).collect();
            // a card can only make a straight flush with them if it makes a flush (hand.rs
            // `flush`: `need` cards of one suit, as `hand::is_suit` counts them): checked first,
            // detection decides
            let flush_suit = |c: &Card, s: Suit| hand::is_suit(c, s, false, true, f.smeared);
            let kept_of = Suit::ALL.map(|s| kept.iter().filter(|k| flush_suit(k, s)).count());
            let makes = |c: &Card| {
                if !Suit::ALL.iter().zip(kept_of).any(|(&s, n)| n + flush_suit(c, s) as usize >= need) {
                    return false;
                }
                let mut v = kept.clone();
                v.push(*c);
                hand::detect(&v, f).contains(StraightFlush)
            };
            let g: Vec<Card> = deck.iter().filter(|c| makes(c)).copied().collect();
            // already made (no card needed) isn't a plan; a made straight short of the suit is
            if !g.is_empty() && !hand::detect(&kept, f).contains(StraightFlush) {
                one_away.push(keep.clone());
                out.push(Aim { family: None, hand: StraightFlush, keep: keep.clone(), groups: vec![(g, 1)] });
            }
        }
        for (keep, missing) in plans.iter().filter(|p| p.1.len() == 2) {
            if one_away.iter().any(|k| keep.iter().all(|i| k.contains(i))) {
                continue;
            }
            let suited_draws = need.saturating_sub(keep.iter().filter(|&&i| in_suit(&hand[i])).count());
            let ways: &[[bool; 2]] = match suited_draws {
                0 => &[[false, false]],
                1 => &[[true, false], [false, true]],
                2 => &[[true, true]],
                _ => continue,
            };
            let best = ways
                .iter()
                .map(|w| missing.iter().zip(w).map(|(&r, &su)| (if su { rank_fits(r, &in_suit) } else { rank_fits(r, &any) }, 1)).collect::<Vec<(Vec<Card>, usize)>>())
                .filter(|g| g.iter().all(|x| !x.0.is_empty()))
                .max_by_key(|g| g.iter().map(|x| x.0.len()).product::<usize>());
            if let Some(groups) = best {
                out.push(Aim { family: None, hand: StraightFlush, keep: keep.clone(), groups });
            }
        }
    }
    let rank_of = |r: i32| -> Vec<usize> { (0..hand.len()).filter(|&i| id(&hand[i], i) == r).collect() };
    let fits = |r: i32| -> Vec<Card> { deck.iter().enumerate().filter(|(i, c)| id(c, 100 + i) == r).map(|(_, c)| *c).collect() };
    let mut pairs = vec![];
    for r in 2..=14 {
        let keep = rank_of(r);
        let more = match keep.len() {
            2 => ThreeOfAKind,
            3 => FourOfAKind,
            4 => FiveOfAKind,
            _ => continue,
        };
        if keep.len() == 2 {
            pairs.push(r);
        }
        let g = fits(r);
        if !g.is_empty() {
            out.push(Aim { family: None, hand: more, keep, groups: vec![(g, 1)] });
        }
    }
    for (k, &r1) in pairs.iter().enumerate() {
        for &r2 in &pairs[k + 1..] {
            let mut keep = rank_of(r1);
            keep.extend(rank_of(r2));
            let mut g = fits(r1);
            g.extend(fits(r2));
            if !g.is_empty() {
                out.push(Aim { family: None, hand: FullHouse, keep, groups: vec![(g, 1)] });
            }
        }
    }
    // one more of a single card's rank beside a group (misc_functions.lua `get_X_same`): Two
    // Pair from a Pair, a Full House from Three of a Kind; the group with one to `AIM_SINGLES`
    // singles, those with the most cards of their rank left first (then the higher), any of
    // whose ranks completes it
    let groups_of = |n: usize| (2..=14).filter(|&r| rank_of(r).len() >= n).count();
    let mut singles: Vec<i32> = (2..=14).filter(|&r| rank_of(r).len() == 1 && !fits(r).is_empty()).collect();
    singles.sort_by(|&a, &c| fits(c).len().cmp(&fits(a).len()).then(c.cmp(&a)));
    for r in 2..=14 {
        let group = rank_of(r);
        let made = match group.len() {
            // a hand already made isn't a plan: two groups make Two Pair, a Full House with three
            2 if groups_of(2) == 1 => TwoPair,
            3 if groups_of(2) == 1 => FullHouse,
            _ => continue,
        };
        for k in 1..=singles.len().min(AIM_SINGLES) {
            let mut keep = group.clone();
            let mut g = vec![];
            for &x in &singles[..k] {
                keep.extend(rank_of(x));
                g.extend(fits(x));
            }
            if !g.is_empty() {
                out.push(Aim { family: Some(r), hand: made, keep, groups: vec![(g, 1)] });
            }
        }
    }
    // the same plan twice (Smeared Joker: Hearts and Diamonds are one suit) counts once
    // (each group's fits in `Card::order_cmp` order, compared by `order_eq`: the same as
    // comparing their sorted `order_key`s)
    type Key = (HandType, Vec<usize>, Vec<(Vec<Card>, usize)>);
    let same = |x: &Key, y: &Key| {
        x.0 == y.0 && x.1 == y.1 && x.2.len() == y.2.len() && x.2.iter().zip(&y.2).all(|(g, h)| g.1 == h.1 && g.0.len() == h.0.len() && g.0.iter().zip(&h.0).all(|(c, d)| c.order_eq(d)))
    };
    let mut seen: Vec<Key> = vec![];
    out.retain(|a| {
        let mut k = a.keep.clone();
        k.sort();
        let fits = |g: &(Vec<Card>, usize)| {
            let mut v = g.0.clone();
            v.sort_by(Card::order_cmp);
            (v, g.1)
        };
        let key = (a.hand, k, a.groups.iter().map(fits).collect::<Vec<_>>());
        let new = !seen.iter().any(|s| same(s, &key));
        seen.push(key);
        new
    });
    out
}

/// Every run of `need` ranks a straight can be (misc_functions.lua `get_straight`): consecutive
/// ranks, or with Shortcut a gap of one rank between any two; an Ace is 1 or 14. Computed once
/// per rule set.
fn straight_runs(need: usize, shortcut: bool) -> std::rc::Rc<Vec<Vec<i32>>> {
    type Runs = std::rc::Rc<Vec<Vec<i32>>>;
    thread_local! {
        static RUNS: std::cell::RefCell<Vec<((usize, bool), Runs)>> = const { std::cell::RefCell::new(vec![]) };
    }
    if let Some(r) = RUNS.with(|c| c.borrow().iter().find(|e| e.0 == (need, shortcut)).map(|e| e.1.clone())) {
        return r;
    }
    let mut runs: Vec<Vec<i32>> = vec![];
    for lo in 1..=14 {
        let mut stack = vec![vec![lo]];
        while let Some(run) = stack.pop() {
            if run.len() == need {
                runs.push(run);
                continue;
            }
            let last = *run.last().unwrap();
            for step in if shortcut { 1..=2 } else { 1..=1 } {
                if last + step <= 14 {
                    let mut r = run.clone();
                    r.push(last + step);
                    stack.push(r);
                }
            }
        }
    }
    let runs = std::rc::Rc::new(runs);
    RUNS.with(|c| c.borrow_mut().push(((need, shortcut), runs.clone())));
    runs
}

/// The chance of completing `aim` within `digs` digs, each throwing away up to 5 cards not in
/// it, from `slots` slots of the hand: its size less the cards held back beside the plan, which
/// a dig doesn't throw (`ChaseRound::stays`). None when its cards and the cards it still needs
/// don't fit in those slots (a dig never makes room by throwing the cards held back). A flush: the exact draw (`flush_odds`: the suited cards you draw stay, so later digs
/// see fewer). Others: the cards the digs would see if each refilled every slot outside the
/// aim (up to 5), taken as one draw from the pile (`draw_odds`). As if every one of them went
/// to this plan: `chase` asks it dig by dig.
fn aim_odds(aim: &Aim, pile: usize, slots: usize, digs: usize) -> f64 {
    if slots < aim.keep.len() + aim.groups.iter().map(|g| g.1).sum::<usize>() {
        return 0.0;
    }
    if aim.hand == HandType::Flush {
        let (fits, n) = &aim.groups[0];
        return flush_odds(aim.keep.len(), fits.len(), pile, slots, aim.keep.len() + n, digs);
    }
    let seen = (digs * slots.saturating_sub(aim.keep.len()).min(5)).min(pile);
    let groups: Vec<(usize, usize)> = aim.groups.iter().map(|g| (g.0.len(), g.1)).collect();
    draw_odds(&groups, pile, seen)
}

/// The round a chase is played out in: the draw pile, hand size, hands and discards left, the
/// score still needed, the best play's average, a later hand's when playing on (`playing_on`)
/// and whether it keeps pace.
#[derive(Clone, Copy)]
struct ChaseRound {
    pile: usize,
    size: usize,
    hands: i64,
    discards: i64,
    need: f64,
    best: f64,
    /// a later hand played on, on average (`playing_on`)
    later: f64,
    on_pace: bool,
    /// the most cards the boss's press-play step discards from those held once a hand is played
    /// (`score::hook_takes`: The Hook's 2), a junk hand too
    takes: usize,
    /// the cards held beside the plan's that a dig never throws (`held_value`, `dig_for`): they
    /// take slots a dig would refill
    stays: usize,
    /// what the first dig's discard does to a later hand played on (`dig_board`): the best play
    /// now on the board it leaves against this board's (`ratio`); 1 when it leaves the scores
    lift: f64,
    /// what the policy's alternative to a chase does: off pace with a discard left, it discards
    /// now (the cards outside the best play and those worth holding, `decide_cards_with`'s last
    /// step), so playing on is on the board that discard leaves (`spent_by`: the engine's whole
    /// step, as `lift` is the dig's), the best play now there against this board's (`ratio`); 1
    /// on pace or with nothing to throw (it plays on and keeps its discards), with none left, or
    /// where the discard changes nothing a score reads
    spent: f64,
    /// on the round's last hand, the score still needed when only reaching it counts (`reached`);
    /// None where points count
    last: Option<f64>,
    /// on the round's last hand, playing the best play now as a plan's completion is counted
    /// (`play_reach`: per roll, after the burn), in place of its average
    now: f64,
}

/// What chasing a plan is worth, as the policy plays it out.
struct Chase {
    /// the points the rest of the round is expected to score with it
    value: f64,
    /// whether the policy chases it: it's worth more than playing on
    go: bool,
}

/// On pace, a chase is made only when it's worth more than playing on by this share of the
/// best play (heuristic, in the register; 0.25 and 0.5 measured within noise of each other,
/// before the hand a chase leaves counted the cards worth holding).
const ON_PACE_MARGIN: f64 = 0.5;

/// Chasing `aim` (scoring `score`) dig by dig, against playing on: the points the rest of the
/// round is expected to score either way. The chase is worked out from the last dig back
/// (discards first, then junk hands, each one hand fewer): a later dig is spent only while the
/// chase from there is worth more than playing the hand the first dig leaves (`left`, the same
/// at every dig), then a later hand played on (`later`): a simpler test than the policy's own at its
/// next decision, which sees the new cards and, on pace, its margin and rules. At each dig, the plan is made with its
/// chance given the digs before failed (`aim_odds` between them, with the slots the plan's cards
/// and the cards held back beside them leave, `stays`); else a lesser plan it keeps
/// alive (`lesser`: every card it keeps, the chase keeps) can be made first, and ends it when
/// it puts the round on pace (its score × the hands left after the dig: the flat pace, its own growth
/// isn't worked out); else the chase goes
/// on, or stops with the hand it leaves (`left`: the best play of the hand the first dig leaves). A
/// hand made is played once, the hands after it at a later hand's (`later`), and a junk hand
/// scores nothing; playing on now is the best play now, then `later` each hand after. After a
/// first dig that discards, every hand is on the board its discard leaves (`dig_board`): `score`,
/// `left` and the lesser plans come scored there, and a later hand played on is `later` times
/// `ChaseRound::lift`; the digs after the first count no lift of their own. At the first dig,
/// off pace, playing on is charged the discard the policy makes instead of the chase
/// (`ChaseRound::spent`), so a discard's step is weighed on both sides; playing on after a later
/// dig isn't, as that dig counts none. A lesser plan's chance per
/// dig: its cards among the dig's new ones (other than the plan's own fits; cards drawn
/// toward it in earlier digs aren't counted, the chase throws them away), the best of them
/// when several turn up. On pace, the chase must beat playing on by `ON_PACE_MARGIN` of the
/// best play, but on the round's last hand. On the round's last hand (one hand left, a discard to dig with) the round is that
/// hand alone, so only reaching the score still needed counts: every hand it can end with is
/// worth that score when it reaches it and nothing when it doesn't (`reached`; the caller's
/// `score` and `lesser` come measured so, their share of completions that reach it), playing
/// now by its share of rolls that reach it (`ChaseRound::now`, `play_reach`: no `spent` is
/// charged there) and the hand a failed chase leaves by its average; a lesser plan ends the
/// chase when its completion reaches it.
fn chase(aim: &Aim, score: f64, left: f64, lesser: &[(&Aim, f64)], r: &ChaseRound) -> Chase {
    // the hand's slots a dig refills: all but the cards it holds back (`stays`)
    let room = r.size.saturating_sub(r.stays);
    let t = room.saturating_sub(aim.keep.len()).min(5);
    let digs = (r.discards + r.hands - 1).max(0) as usize;
    // made within j digs, for j up to every dig
    let made: Vec<f64> = (0..=digs).map(|j| aim_odds(aim, r.pile, room, j)).collect();
    let fits: Vec<&Card> = aim.groups.iter().flat_map(|g| g.0.iter()).collect();
    // each lesser plan's groups without the plan's own fits, the best first
    let mut lesser: Vec<(Vec<(usize, usize)>, f64)> = lesser
        .iter()
        .map(|(l, sc)| (l.groups.iter().map(|g| (g.0.iter().filter(|c| !fits.contains(c)).count(), g.1)).collect(), *sc))
        .collect();
    lesser.sort_by(|a, c| c.1.total_cmp(&a.1));
    // a later hand played on after the first dig, on the board its discard leaves (`lift`;
    // `score`, `left` and the lesser plans come scored there); playing on now without it
    let later = r.later * r.lift;
    // the round's last hand: a hand counts the score needed when it reaches it, else nothing
    let last = r.last;
    let left = reached(left, last);
    // the hands left at dig s (a junk hand once the discards are gone)
    let hands_at = |s: usize| if s as i64 >= r.discards { r.hands - (s as i64 - r.discards) } else { r.hands };
    // playing on at dig s instead of digging (at the first, charged the policy's own discard)
    let stay = |s: usize| {
        let h = hands_at(s) as f64;
        if s == 0 && last.is_some() { r.now } else if s == 0 { (r.best + (h - 1.0) * r.later) * r.spent } else { left + (h - 1.0) * later }
    };
    let mut out = Chase { value: stay(0), go: false };
    // the chase from the next dig on, when the policy goes on with it
    let mut next: Option<f64> = None;
    for s in (0..digs).rev() {
        let (f0, f1) = (made[s], made[s + 1]);
        let hit = if f0 < 1.0 { ((f1 - f0) / (1.0 - f0)).clamp(0.0, 1.0) } else { 1.0 };
        let hands = hands_at(s);
        let after = if s as i64 >= r.discards { hands - 1 } else { hands };
        let ends = |x: f64| x + (after - 1).max(0) as f64 * later;
        let pile = r.pile.saturating_sub(t * s).saturating_sub(fits.len());
        let (mut miss, mut stop) = (1.0, 0.0);
        for (groups, sc) in &lesser {
            let q = draw_odds(groups, pile, t.min(pile));
            // on the last hand: made, it stops the chase when its completion reaches the score
            // needed (`sc`, measured so, is that score times its share that does)
            if let Some(need) = last {
                let q = q * (sc / need).min(1.0);
                stop += miss * q * need;
                miss *= 1.0 - q;
                continue;
            }
            // whether the policy stops there: the made hand's score every hand left, the flat pace
            // (the made hand's own growth isn't worked out; the current hand's `later` would credit
            // it with growth it may not give, a Flush with the Two Pair's on Spare Trousers)
            if sc * (after as f64) < r.need {
                continue;
            }
            stop += miss * q * ends(*sc);
            miss *= 1.0 - q;
        }
        let mut value = hit * ends(score) + (1.0 - hit) * (stop + miss * next.unwrap_or_else(|| stay(s + 1)));
        // a junk hand is a played hand: the press-play step then discards from the cards held
        // (the plan's, those kept for their held value and the rest the junk hand, of up to 5,
        // leaves), the plan's among them; the plan goes on only when none of its cards is
        // taken, else the round plays on from the hand that dig leaves (the cards drawn in
        // place of those taken aren't counted toward it)
        if s as i64 >= r.discards && r.takes > 0 {
            let held = r.size - r.size.saturating_sub(aim.keep.len() + r.stays).min(5);
            let k = r.takes.min(held);
            let spared = comb(held.saturating_sub(aim.keep.len()), k) / comb(held, k).max(1.0);
            value = spared * value + (1.0 - spared) * stay(s + 1);
        }
        // the margin is in points: on the last hand the values are the score needed times a
        // chance, so none there
        let margin = if s == 0 && r.on_pace && last.is_none() { ON_PACE_MARGIN * r.best } else { 0.0 };
        out = Chase { value, go: value > stay(s) + margin };
        next = out.go.then_some(value);
    }
    out
}

/// The chance that `seen` cards drawn from a pile of `pile` hold at least `need` of each
/// (disjoint) group of `size` cards: the multivariate hypergeometric, exactly.
fn draw_odds(groups: &[(usize, usize)], pile: usize, seen: usize) -> f64 {
    use std::cell::RefCell;
    use std::collections::HashMap;
    // The same draws recur constantly across plans and simulations (as `flush_odds`), so cache
    // per thread, by the groups in order (the sum's order), the pile and the cards seen packed
    // in one number: up to 3 groups of up to 255 cards needing up to 15, piles up to 1023
    thread_local! {
        static CACHE: RefCell<HashMap<u64, f64, std::hash::BuildHasherDefault<OneWord>>> = RefCell::new(HashMap::default());
    }
    if groups.len() > 3 || pile > 1023 || seen > 1023 || groups.iter().any(|&(size, need)| size > 255 || need > 15) {
        return draw_odds_uncached(groups, pile, seen);
    }
    let key = groups.iter().fold(((groups.len() as u64) << 20) | ((pile as u64) << 10) | seen as u64, |k, &(size, need)| (k << 12) | ((size as u64) << 4) | need as u64);
    if let Some(v) = CACHE.with(|c| c.borrow().get(&key).copied()) {
        return v;
    }
    let v = draw_odds_uncached(groups, pile, seen);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 200_000 {
            c.clear();
        }
        c.insert(key, v);
    });
    v
}

/// A hasher for keys that are one number (`draw_odds`): a multiply, no more
#[derive(Default)]
struct OneWord(u64);

impl std::hash::Hasher for OneWord {
    fn finish(&self) -> u64 {
        // the product's high bits mix every bit of the key; fold them into the low ones
        self.0 ^ (self.0 >> 29)
    }
    fn write(&mut self, bytes: &[u8]) {
        for &x in bytes {
            self.0 = (self.0.rotate_left(8) ^ x as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
    }
    fn write_u64(&mut self, x: u64) {
        self.0 = (self.0 ^ x).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

fn draw_odds_uncached(groups: &[(usize, usize)], pile: usize, seen: usize) -> f64 {
    let in_groups: usize = groups.iter().map(|g| g.0).sum();
    if in_groups > pile {
        return 0.0;
    }
    let total = comb(pile, seen);
    if total == 0.0 {
        return 0.0;
    }
    // ways over the groups' counts (each at least its need), the rest from outside them
    fn ways(groups: &[(usize, usize)], left: usize, rest: usize, comb: &dyn Fn(usize, usize) -> f64) -> f64 {
        match groups.split_first() {
            None => comb(rest, left),
            Some((&(size, need), more)) => (need..=size.min(left)).map(|x| comb(size, x) * ways(more, left - x, rest, comb)).sum(),
        }
    }
    (ways(groups, seen, pile - in_groups, &comb) / total).min(1.0)
}

/// Plans given the full estimate (`aim_score`) after the screen on one completion
/// (`aim_quick`)
const AIM_FINALISTS: usize = 2;

/// The most single cards a Two Pair or Full House plan keeps beside its group (`aims`): each
/// more is another rank that completes it and one card fewer drawn per dig. Measured
/// (2026-10-10, `tests/player.rs`): 1: 60.97%, 2: 61.03%, 3: 60.73% (with three, plans overrate
/// themselves: a pair among the cards drawn isn't counted, and fewer are drawn)
const AIM_SINGLES: usize = 2;

/// The screen for `aim_score`: its middle completion only (not the best, not the worst), every
/// random roll failing, with the cards held beside it (`held_beside`: the first draw).
fn aim_quick(b: &Board, aim: &Aim, hand: &[Card], held: &[usize], ends: Option<Ends>) -> f64 {
    let kept: Vec<Card> = held.iter().filter(|i| !aim.keep.contains(i)).map(|&i| hand[i]).collect();
    let mut all = aim_completions_held(b, aim, hand);
    let (cards, _, rest) = all.swap_remove(all.len() / 2);
    let held = held_beside(aim, hand, &cards, &rest, &kept, ends, 0);
    score::score(b, &cards, &held, &mut Unlucky, false).score
}

/// The draw pile (in a fixed order) and the hand size, when the score reads held cards: the
/// cards a completed plan holds beside it then count the ones drawn into the slots it doesn't keep
#[derive(Clone, Copy)]
struct Ends<'a> {
    pile: &'a [Card],
    size: usize,
}

/// The draws a plan's completions are scored with when they hold drawn cards (`held_beside`),
/// the same for every plan and for the hand a dig leaves: each completion one at least, every
/// draw used (more when the rolls ask for more; heuristic, in the register)
const HELD_DRAWS: usize = 4;

/// The cards held beside a plan's completion `cards`: `kept` (what its dig leaves in hand beside
/// the plan) and `rest` (the plan's cards beyond the five played), and, when the score reads held
/// cards (`ends`), the hand filled to its size from the pile: draw `s` of `held_draws`, without
/// the completion's own fits. Without `ends`, `kept` alone.
fn held_beside(aim: &Aim, hand: &[Card], cards: &[Card], rest: &[Card], kept: &[Card], ends: Option<Ends>, s: usize) -> Vec<Card> {
    let Some(e) = ends else { return kept.to_vec() };
    // the plan's own cards stay, then `kept` in its order (the cards worth holding first), up to
    // the hand's size: a guard, as a plan whose cards don't fit beside the cards held back is
    // never made (`aim_odds`)
    let mut held: Vec<Card> = rest.iter().chain(kept).copied().collect();
    held.truncate(e.size.saturating_sub(cards.len()));
    // the fits: the completion's cards that aren't the plan's own
    let mut fits: Vec<Card> = cards.iter().chain(rest).copied().collect();
    for &i in &aim.keep {
        if let Some(p) = fits.iter().position(|c| *c == hand[i]) {
            fits.swap_remove(p);
        }
    }
    let count = e.size.saturating_sub(cards.len() + held.len());
    held.extend(held_draws(e.pile, &fits, count, s));
    held
}

/// `count` cards drawn from `pile` (in a fixed order) without the cards `taken`: draw `s` is one
/// order of the whole pile, seeded by `s` alone, whose first cards not taken are drawn, so every
/// plan, and the hand a dig leaves, sees the same draws but where a plan's own fit falls, and a
/// draw of more cards begins with those of fewer
fn held_draws(pile: &[Card], taken: &[Card], count: usize, s: usize) -> Vec<Card> {
    let mut order: Vec<Card> = pile.to_vec();
    shuffle(&mut order, &mut Rng::new(0x4e1d_0000 + s as u64));
    let mut skip: Vec<Card> = taken.to_vec();
    let mut out = Vec::with_capacity(count);
    for c in order {
        if out.len() >= count {
            break;
        }
        match skip.iter().position(|t| *t == c) {
            Some(p) => {
                skip.swap_remove(p);
            }
            None => out.push(c),
        }
    }
    out
}

/// Random completions drawn for a plan more than one card away
const AIM_FILLS: usize = 4;

/// The ways `aim` can be completed, each with its weight: with one card missing from each
/// group (at most two groups), every distinct card that fits, weighted by its copies (exact);
/// otherwise `AIM_FILLS` draws at
/// random from what fits (a fixed seed: the same draws for every call). Not a fixed "typical"
/// card: the middle ones are neighbouring ranks, so a flush draw looked like a straight flush;
/// spread ones never are, though with Four Fingers and Shortcut one often is.
#[cfg(test)]
fn aim_completions(b: &Board, aim: &Aim, hand: &[Card]) -> Vec<(Vec<Card>, f64)> {
    aim_completions_held(b, aim, hand).into_iter().map(|(c, w, _)| (c, w)).collect()
}

/// `aim_completions`, each with the cards of it left out of the play (a plan keeping five or
/// more: its kept cards and fits beyond the best five), which stay in hand
fn aim_completions_held(b: &Board, aim: &Aim, hand: &[Card]) -> Vec<(Vec<Card>, f64, Vec<Card>)> {
    let base: Vec<Card> = aim.keep.iter().map(|&i| hand[i]).collect();
    let distinct = |fits: &[Card]| -> Vec<(Card, f64)> {
        let mut v: Vec<(Card, f64)> = vec![];
        for c in fits {
            // a debuffed copy (The Pillar's) apart from a live one
            match v.iter_mut().find(|x| x.0.same_kind(c) && x.0.debuff == c.debuff) {
                Some(x) => x.1 += 1.0,
                None => v.push((*c, 1.0)),
            }
        }
        v.sort_by(|x, y| x.0.order_cmp(&y.0));
        v
    };
    let mut out = vec![];
    if aim.groups.len() <= 2 && aim.groups.iter().all(|g| g.1 == 1) {
        let firsts = distinct(&aim.groups[0].0);
        let seconds = aim.groups.get(1).map(|g| distinct(&g.0));
        for (c, w) in &firsts {
            match &seconds {
                None => out.push((base.iter().copied().chain([*c]).collect(), *w)),
                Some(v) => out.extend(v.iter().map(|(d, w2)| (base.iter().copied().chain([*c, *d]).collect(), w * w2))),
            }
        }
    } else {
        let mut rng = Rng::new(0x1d1e);
        for _ in 0..AIM_FILLS {
            let mut cards = base.clone();
            for (fits, n) in &aim.groups {
                let mut pool = fits.clone();
                for _ in 0..*n {
                    if !pool.is_empty() {
                        cards.push(pool.swap_remove(rng.below(pool.len())));
                    }
                }
            }
            out.push((cards, 1.0));
        }
    }
    // a completion is what you'd play of it: more than five cards (a plan keeping five or more)
    // give their best play, which holds the card that completes it; the rest stay in hand
    out.into_iter()
        .map(|(mut cards, w)| {
            let mut rest = vec![];
            if cards.len() > 5 {
                let mut play: Vec<Card> = best_play(b, &cards).map(|p| p.cards.iter().map(|&i| cards[i]).collect()).unwrap_or_else(|| cards.clone());
                play.truncate(5);
                for c in &play {
                    if let Some(p) = cards.iter().position(|d| d == c) {
                        cards.swap_remove(p);
                    }
                }
                rest = cards;
                cards = play;
            }
            (cards, w, rest)
        })
        .collect()
}

/// What completing `aim` is expected to score: the weighted average over its completions
/// (`aim_completions`), random effects at their average (`MEAN_ROLLS` rolls in all) when the
/// middle completion or any Lucky card in it is random. Held beside it: `held`, and when the
/// score reads held cards (`ends`), the hand as it is then (`held_beside`: what the dig leaves
/// and the draws into the rest, `HELD_DRAWS` of them over the completions). `last`: the score
/// still needed when the hand is the round's last, where only reaching it counts (`chase`): each
/// completion is then worth that score if it reaches it, else nothing. `burns`: each completion
/// scored at the better of the board's discards left and none, as the policy burns them before
/// playing it when that scores more (`burn_pays`).
fn aim_score(b: &Board, aim: &Aim, hand: &[Card], held: &[usize], ends: Option<Ends>, last: Option<f64>, burns: bool) -> f64 {
    let kept: Vec<Card> = held.iter().filter(|i| !aim.keep.contains(i)).map(|&i| hand[i]).collect();
    let all = aim_completions_held(b, aim, hand);
    // each completion's hand detected once
    let flags = b.rule_flags();
    let infos: Vec<hand::HandInfo> = all.iter().map(|x| hand::detect(&x.0, flags)).collect();
    let (mid, mid_info) = (&all[all.len() / 2].0, &infos[all.len() / 2]);
    let lucky = |c: &Card| c.enhancement == Some(Enhancement::Lucky);
    // the middle completion's floor, when worked out for the check (it's its score when nothing
    // is random); a floor that asked for no roll (`Watched`) is its ceiling too
    let mut mid_floor = None;
    let mid_held = held_beside(aim, hand, mid, &all[all.len() / 2].2, &kept, ends, 0);
    let random = all.iter().any(|x| x.0.iter().any(lucky)) || {
        let mut w = Watched { rolls: Unlucky, asked: false };
        let floor = score::score_detected(b, mid, &mid_held, mid_info.clone(), &mut w, false).score;
        mid_floor = Some(floor);
        w.asked && score::score_detected(b, mid, &mid_held, mid_info.clone(), &mut crate::engine::Lucky, false).score > floor
    };
    let none = (burns && b.discards_left > 0).then(|| {
        let mut z = b.clone();
        z.discards_left = 0;
        z
    });
    // the better of the two boards, on the same roll (`same`: the rolls before this score)
    let burned = |x: f64, cards: &[Card], held: &[Card], info: &hand::HandInfo, same: Option<Rng>| match &none {
        Some(z) => x.max(match same {
            Some(mut r) => score::score_detected(z, cards, held, info.clone(), &mut r, false).score,
            None => score::score_detected(z, cards, held, info.clone(), &mut Unlucky, false).score,
        }),
        None => x,
    };
    let mut rolls = Rng::new(0x1d1e);
    let per = if random { MEAN_ROLLS.div_ceil(all.len()) } else { 1 };
    // with drawn cards held, every completion one draw at least, and every one of the
    // `HELD_DRAWS` draws used
    let per = if ends.is_some() { per.max(HELD_DRAWS.div_ceil(all.len())) } else { per };
    let (mut total, mut weight) = (0.0, 0.0);
    let mut s = 0;
    for (k, ((cards, w, rest), info)) in all.iter().zip(&infos).enumerate() {
        for r in 0..per {
            // every plan, and the hand a dig leaves, on the same `HELD_DRAWS` draws
            let held = held_beside(aim, hand, cards, rest, &kept, ends, s % HELD_DRAWS);
            s += 1;
            let same = random.then(|| rolls.clone());
            let x = match (random, mid_floor) {
                (true, _) => score::score_detected(b, cards, &held, info.clone(), &mut rolls, false).score,
                (false, Some(f)) if ends.is_none() && k == all.len() / 2 && r == 0 => f,
                (false, _) => score::score_detected(b, cards, &held, info.clone(), &mut Unlucky, false).score,
            };
            total += w * reached(burned(x, cards, &held, info, same), last);
            weight += w;
        }
    }
    total / weight
}

/// A score as the round counts it (`chase`): as it is, or on the round's last hand (`last`: the
/// score still needed) the score needed when it reaches it, else nothing
fn reached(x: f64, last: Option<f64>) -> f64 {
    match last {
        Some(need) if x >= need => need,
        Some(_) => 0.0,
        None => x,
    }
}

/// The cards of `hand` outside `play` that add to its score while held (Baron's Kings, Steel
/// cards, Shoot the Moon's Queens, …: the engine's held effects): thrown away, they'd cost
/// the play now. One scoring per card, left out. Under a press-play discard that takes some of
/// the cards held (The Hook), when the score reads held cards, a card is measured against what
/// throwing it puts in its place, a card from `pile` (each of its cards as likely), over every
/// set the step could take: a discard or a dig draws back to hand size, so the step picks from
/// as many cards either way, and the card counts only when it stays, with the cards it may stay
/// with (Raised Fist reads the lowest one left). Left out instead, every plain card beside a
/// Steel one looked worth holding, as a card the Hook might take in its place (4.7-7.7 points
/// on the enhanced deck's Hook rounds, gap 15); against a card that adds nothing, any card Raised
/// Fist could read. What the step's discard of the card itself does (Castle, Hit the Road)
/// isn't counted. When the step takes every card held, none is worth holding.
fn held_value(b: &Board, hand: &[Card], play: &[usize], pile: &[Card]) -> Vec<usize> {
    let played: Vec<Card> = play.iter().map(|&i| hand[i]).collect();
    let rest: Vec<usize> = (0..hand.len()).filter(|i| !play.contains(i)).collect();
    let n = rest.len();
    let full: Vec<Card> = rest.iter().map(|&i| hand[i]).collect();
    let takes = score::hook_takes(b, n);
    if takes > 0 && takes >= n {
        return vec![];
    }
    let of = |held: &[Card]| score::score_pressed(b, &played, held, &mut Unlucky, false).score;
    // with no held card read, none adds (and a drawn one only could: a Steel card)
    if takes == 0 || pile.is_empty() || !score::held_read(b, &full) {
        let all = of(&full);
        return (0..n).filter(|&k| of(&full.iter().enumerate().filter(|&(j, _)| j != k).map(|(_, c)| *c).collect::<Vec<_>>()) < all).map(|k| rest[k]).collect();
    }
    // the pile's cards, each kind once with its copies, in a fixed order
    let mut sorted = pile.to_vec();
    sorted.sort_by(|x, y| x.order_cmp(y));
    let mut kinds: Vec<(Card, f64)> = vec![];
    for c in sorted {
        match kinds.last_mut() {
            Some(last) if last.0 == c => last.1 += 1.0,
            _ => kinds.push((c, 1.0)),
        }
    }
    // the play's score with the held cards but those of `gone` (by place in `rest`; a hand holds
    // fewer than 64), and a card of the pile when given: each scored once
    let mut seen: std::collections::HashMap<(u64, Option<usize>), f64> = std::collections::HashMap::new();
    let mut score = |gone: u64, drawn: Option<usize>| -> f64 {
        *seen.entry((gone, drawn)).or_insert_with(|| {
            let mut held: Vec<Card> = (0..n).filter(|j| gone & (1 << j) == 0).map(|j| full[j]).collect();
            held.extend(drawn.map(|d| kinds[d].0));
            of(&held)
        })
    };
    let sets = score::hook_sets(b, n);
    let mut keep = vec![];
    for (k, &i) in rest.iter().enumerate() {
        // what a card drawn in its place scores over the card, on each set that leaves its place
        // (a set that takes it, or what replaced it, scores the same either way): differences,
        // so a card that adds no more than any drawn one comes to 0 exactly
        let mut gain = 0.0;
        for g in sets.iter().filter(|g| !g.contains(&k)) {
            let gone = g.iter().fold(0u64, |m, &j| m | (1 << j));
            let own = score(gone, None);
            for (d, (_, w)) in kinds.iter().enumerate() {
                gain += w * (score(gone | (1 << k), Some(d)) - own);
            }
        }
        if gain < 0.0 {
            keep.push(i);
        }
    }
    keep
}

/// Simulates the rest of a round with the `decide` policy.
pub fn sim_round(board: &Board, start: &RoundStart, rng: &mut Rng) -> RoundResult {
    sim_round_uses(board, start, rng, &[])
}

/// A consumable held in a blind, as the round simulation can use it: the cards it changes in
/// hand (each must still be there), the cards it puts into your hand, and levels it adds.
#[derive(Debug, Clone, Default)]
pub struct Use {
    pub name: String,
    /// The consumable's key: once used, it's no longer held (its `RoundGoals::seen` entries go)
    pub key: String,
    pub swap: Vec<(Card, Card)>,
    /// A card effect on places in the round's starting hand (one that goes on a face-down card),
    /// done to whatever cards are there (`effect_on`; `uses_on` puts it on the cards a round
    /// dealt, `placed`)
    pub at: Vec<(CardEffect, Vec<usize>)>,
    pub add: Vec<Card>,
    pub levels: [i64; 12],
    /// A Planet card: Constellation grows ×0.1 when it's used
    pub planet: bool,
    /// The effects on places (`at`) as `uses_on` put them on the cards a round dealt there: done
    /// after `swap`, their cards added after `add`
    pub placed: Vec<Placed>,
}

/// An effect on places (`Use::at`) on the cards a round dealt there (`uses_on`): the cards at its
/// places, the change it makes to them (`swap`, each card it changes) and the cards it adds
/// (Cryptid's copies), with the effect itself kept, so while one of its cards is face down each
/// draw of what that card could be (`decide_unseen`) puts it on the cards the draw has at those
/// places (`Use::unplaced`), not on the card it really is (gap 25 (a))
#[derive(Debug, Clone)]
pub struct Placed {
    pub effect: CardEffect,
    /// The cards at its places, in the places' order (`effect_on` reads which is which: Death's
    /// left and right); none when a place is past the hand's end
    pub cards: Vec<Card>,
    pub swap: Vec<(Card, Card)>,
    pub add: Vec<Card>,
}

impl Placed {
    /// `effect` on the cards at `places` in `hand` (`effect_on`)
    pub fn new(b: &Board, hand: &[Card], effect: CardEffect, places: &[usize]) -> Placed {
        let new = effect_on(b, hand, effect, places);
        Placed {
            effect,
            cards: if places.iter().all(|&p| p < hand.len()) { places.iter().map(|&p| hand[p]).collect() } else { vec![] },
            swap: places.iter().filter(|&&p| p < hand.len() && new[p] != hand[p]).map(|&p| (hand[p], new[p])).collect(),
            add: new.iter().skip(hand.len()).copied().collect(),
        }
    }
}

/// Where card `c` is in `hand`, among the places not `taken`: the card itself (its debuff aside;
/// face down or up as it was first), else a face-up one alike but for its face, else a face-up one
/// of its kind (two alike but one played this ante, The Pillar's): a face-down card is never
/// taken for another by what it really is
fn find_card(hand: &[Card], c: &Card, taken: &[bool]) -> Option<usize> {
    let key = |x: &Card, face: bool| Card { debuff: false, face_down: face && x.face_down, ..*x };
    (0..hand.len())
        .find(|&i| !taken[i] && key(&hand[i], true) == key(c, true))
        .or_else(|| (0..hand.len()).find(|&i| !taken[i] && !hand[i].face_down && key(&hand[i], false) == key(c, false)))
        .or_else(|| (0..hand.len()).find(|&i| !taken[i] && !hand[i].face_down && hand[i].same_kind(c)))
}

/// Where each of `cards` is in `hand` (`find_card`, each place once); none: one isn't there
fn find_cards(hand: &[Card], cards: &[Card]) -> Option<Vec<usize>> {
    let mut taken = vec![false; hand.len()];
    cards
        .iter()
        .map(|c| {
            let i = find_card(hand, c, &taken)?;
            taken[i] = true;
            Some(i)
        })
        .collect()
}

impl Use {
    pub fn apply(&self, b: &Board, hand: &[Card]) -> Option<(Board, Vec<Card>)> {
        // every card a place effect reads still in hand, the one it leaves as it is too (Death's
        // right card, Cryptid's original): the game uses it only on cards selected in hand
        // (card.lua `Card:can_use_consumeable`); an effect on each card alone (The World's suit)
        // needs only the cards it changes, which `swap` finds
        let reads = |p: &Placed| matches!(p.effect, CardEffect::CopyLeftToRight | CardEffect::Copies(_));
        if self.placed.iter().any(|p| reads(p) && find_cards(hand, &p.cards).is_none()) {
            return None;
        }
        let mut h = hand.to_vec();
        let mut done = vec![false; h.len()];
        for (from, to) in self.swap.iter().chain(self.placed.iter().flat_map(|p| &p.swap)) {
            let i = find_card(&h, from, &done)?;
            // a card a consumable changed is seen (the game flips it face up and back, card.lua
            // `Card:use_consumeable`, which clears the mark of one drawn flipped: gap 25), its
            // debuff what it is after (`debuff_after`)
            h[i] = Card { face_down: false, ..debuff_after(b, &h[i], *to, i) };
            done[i] = true;
        }
        for (effect, places) in &self.at {
            if places.iter().any(|&p| p >= h.len()) {
                return None;
            }
            h = effect_on(b, &h, *effect, places);
        }
        h.extend(self.add.iter().chain(self.placed.iter().flat_map(|p| &p.add)).copied());
        let mut b = b.clone();
        if let Some(g) = b.goals.as_mut() {
            g.seen.retain(|(k, _, _)| k != &self.key);
        }
        // the cards it adds to your deck: `add`, and Cryptid's copies of a card at a place
        b.playing_cards += (h.len() - hand.len()) as i64;
        for (l, d) in b.levels.iter_mut().zip(self.levels) {
            if d != 0 {
                *l = l.with_level(l.level + d);
            }
        }
        if self.planet {
            for j in b.jokers.iter_mut().filter(|j| j.key == "j_constellation") {
                j.x_mult += 0.1;
            }
        }
        // its slot is free for a Blue Seal's planet
        b.planet_slots += 1;
        Some((b, h))
    }

    /// This consumable with each place effect whose cards include one face down in `hand` turned
    /// back into an effect on the places its cards have now (`at`), so a draw of the face-down
    /// cards (`decide_unseen`) or a round that deals them anew (the oracle's futures) puts it on
    /// the cards it has there (`uses_on`), not on the card it really is; the rest as it is
    fn unplaced(&self, hand: &[Card]) -> Use {
        let mut u = self.clone();
        let mut kept = vec![];
        for p in std::mem::take(&mut u.placed) {
            // where each of its cards is now, as `apply` finds them
            match find_cards(hand, &p.cards).filter(|at| !p.cards.is_empty() && at.iter().any(|&i| hand[i].face_down)) {
                Some(at) => u.at.push((p.effect, at)),
                None => kept.push(p),
            }
        }
        u.placed = kept;
        u
    }
}

/// What a won round is worth beyond winning it, in the advice's measure (by Ante 8, and in Best
/// play's round also in this ante's blinds still ahead, `value::Ahead`): each planet a Blue Seal
/// makes, by the hand played last (it's that hand's planet), each dollar, each card worth
/// drawing, and what the jokers a round changes are left with.
/// The round simulation uses it to choose between winning now and playing on for more.
#[derive(Debug, Clone, Default)]
pub struct RoundGoals {
    /// By Ante 8: a planet of each hand, a dollar (as a share of the run's value)
    pub planet: [f64; 12],
    pub dollar: f64,
    /// In this ante's blinds still ahead (`value::Ahead`): the shift a planet of each hand and a
    /// dollar give the log-odds of getting through them, from `ahead`
    pub planet_ahead: [f64; 12],
    pub dollar_ahead: f64,
    /// The log-odds of getting through this ante's blinds still ahead with your board as it is
    /// (with the shops before them, `value::Ahead::base`), where the shifts are taken from;
    /// nothing counted there: every shift none
    pub ahead: f64,
    pub per_hand: f64,
    /// Cards worth drawing this round, and by how much: (the held consumable's key, the card,
    /// how much more it's worth used on that card than on the best card in your hand). Gone
    /// once the consumable is used.
    pub seen: Vec<(String, Card, f64)>,
    /// A future already being looked ahead into: it plays to the same goals, but doesn't
    /// look ahead again (`play_on_instead`)
    pub no_lookahead: bool,
    /// The `seen` credit the round already has (per consumable), when a future continues it
    pub carried: Vec<(String, f64)>,
    /// What the numbers each joker a round changes (`engine::CHANGE_IN_ROUND`) leave at its end
    /// are worth (`grown`); empty: none counted
    pub jokers: std::sync::Arc<Vec<JokerWorth>>,
}

/// What a joker's numbers left at a round's end are worth (`RoundGoals::jokers`), against its
/// numbers when the analysis started (`start`, as `Joker::lasting` counts them), in two parts
/// kept apart, as a choice is worth its chance to get through times the long-run value of what
/// it leaves (`JokerWorth::of`, `RoundGoals::value`): by Ante 8, one more Mult, ×Mult and Chip
/// each (`per`, taken as linear) and the joker used up (`gone`: Ice Cream, Seltzer, Ramen), with
/// `floor` never less than that, so a fall costs at most what losing it does; and in this ante's
/// blinds still ahead (`ahead`, Best play's round only)
#[derive(Debug, Clone)]
pub struct JokerWorth {
    /// The joker (`Joker::id`: its place on your board, not 0) and its key
    pub id: u32,
    pub key: String,
    pub start: [f64; 3],
    pub per: [f64; 3],
    pub gone: f64,
    /// Whether a fall is floored at `gone`: not in the projection that asks what a joker kept
    /// and played for is worth (`LongRun::played_for`), where the floor would make every fall
    /// below a typical find free; there it's linear in its numbers, used up as its numbers at
    /// none (Mult 0, ×1, Chips 0, as `grown_over` counts it), so using up one that already fell
    /// earns nothing (and one worth only its loss, Seltzer, counts nothing: `round_worths` drops it)
    pub floor: bool,
    pub ahead: AheadWorth,
}

/// What a joker's numbers do to the chance of getting through this ante's blinds still ahead,
/// as a shift of its log-odds (`value::Ahead::joker`; linear in the log-odds, not in the chance,
/// so many steps never pass a sure pass: measured 2026-10-18, below): per unit above `start`
/// (`up`); below it, per unit down to `knot` (a step below, `near`) and past it down to none
/// (`far`), as a blind's chance isn't linear in them (a fall of a step on a board on pace may
/// move nothing where a reset loses the boss); and the joker used up (`gone`), never less than
/// that. All nothing: not counted (the growth runs' goals). On 31 joker steps of 43 saved Small
/// and Big Blind states (2026-10-18), 5 and 10 steps up measured against one step extended:
/// extended in the log-odds, off by 0.12 and 0.14 of the chance's share on average; extended
/// linearly in the share, by 0.51 and 1.05.
#[derive(Debug, Clone, Default)]
pub struct AheadWorth {
    pub up: [f64; 3],
    pub near: [f64; 3],
    pub far: [f64; 3],
    pub knot: [f64; 3],
    pub gone: f64,
    /// Whether its loss was measured (`gone`, 0 where the runs didn't show it): not for a joker
    /// followed only for its counter, which a round can't use up (Loyalty Card)
    pub lost: bool,
    /// Its counter (`Joker::counter`), where it has one the round can move
    pub counter: Option<CounterWorth>,
}

/// What a joker's counter (`Joker::counter`: Loyalty Card's place in its cycle, Yorick's
/// discards to its next step, Seltzer's hands left) left at a round's end does in this ante's
/// blinds still ahead (`value::Ahead::counter`): the shift of the log-odds at each value the
/// round can leave it at (`at`, measured, the start's none), against its value as the analysis
/// started (`start`). Counted only there: the projection to Ante 8 doesn't follow a counter
/// (Loyalty Card's cycle turns many times by then and Seltzer runs out, `LongRun::lasts`, but
/// Yorick's count is progress to its next ×Mult step, which its numbers' worth gives whole to
/// the discard that crosses it: gap 18).
#[derive(Debug, Clone, Default)]
pub struct CounterWorth {
    pub start: f64,
    /// (value, shift), by value, the start's (none) among them
    pub at: Vec<(f64, f64)>,
}

impl CounterWorth {
    /// The shift at counter `v`: measured there, else linear between the values measured either
    /// side of it (`value::COUNTER_POINTS`, a budget), else the nearest
    fn of(&self, v: f64) -> f64 {
        let i = self.at.partition_point(|p| p.0 < v);
        match (i.checked_sub(1).map(|k| self.at[k]), self.at.get(i)) {
            (_, Some(&(x, s))) if x == v => s,
            (Some((x0, s0)), Some(&(x1, s1))) => s0 + (s1 - s0) * (v - x0) / (x1 - x0),
            (Some((_, s)), None) | (None, Some(&(_, s))) => s,
            (None, None) => 0.0,
        }
    }
}

impl AheadWorth {
    /// Whether anything is counted
    pub fn counts(&self) -> bool {
        self.up.iter().chain(&self.near).chain(&self.far).any(|&x| x > 0.0) || self.gone != 0.0 || self.counter.as_ref().is_some_and(|c| c.at.iter().any(|p| p.1 != 0.0))
    }

    /// Its counter at `v` (`CounterWorth::of`): nothing where none is followed
    fn counter_of(&self, v: f64) -> f64 {
        self.counter.as_ref().map_or(0.0, |c| c.of(v))
    }

    /// Its numbers at none (Mult 0, ×1, Chips 0) against `start`, before the floor at `gone`:
    /// what losing it costs at least (`value::Ahead::joker`)
    pub fn at_none(&self, start: &[f64; 3]) -> f64 {
        let none = [0.0, 1.0, 0.0];
        (0..3).filter(|&f| start[f] > none[f]).map(|f| self.near[f] * (self.knot[f] - start[f]) + self.far[f] * (none[f] - self.knot[f])).sum()
    }

    /// Losing it, from the shift measured without it (`shift`, nothing where the runs didn't
    /// show it): never a gain, and at least what its numbers at none cost (`at_none`), so a fall
    /// the runs showed isn't floored away where its loss alone wasn't shown
    pub fn loss(&self, shift: f64, start: &[f64; 3]) -> f64 {
        shift.min(0.0).min(self.at_none(start))
    }

    /// The joker with these numbers (`None`: used up), against `start`: the log-odds' shift
    fn of(&self, start: &[f64; 3], now: Option<[f64; 3]>) -> f64 {
        let Some(n) = now else { return self.gone };
        let at = |f: usize| {
            let d = n[f] - start[f];
            if d >= 0.0 {
                self.up[f] * d
            } else if n[f] >= self.knot[f] {
                self.near[f] * d
            } else {
                self.near[f] * (self.knot[f] - start[f]) + self.far[f] * (n[f] - self.knot[f])
            }
        };
        (0..3).map(at).sum::<f64>().max(self.gone)
    }
}

/// The least factor a joker's fall leaves the run's value by Ante 8 at (`JokerWorth::of`): its
/// log stays finite where a fall (without the floor) or a loss is worth the whole run
const LEAST_FACTOR: f64 = 1e-6;

impl JokerWorth {
    /// The joker as the analysis started (`start`, and its counter where it has one)
    fn at_start(&self) -> JokerNow {
        JokerNow { numbers: self.start, counter: self.ahead.counter.as_ref().map(|c| c.start) }
    }

    /// What the joker as it stands (`None`: used up) is worth against its start, in the two
    /// parts `Grown` keeps: by Ante 8 the log of 1 + its numbers' worth there (so several jokers'
    /// falls multiply the run's value, each at most to nothing, where added they'd pass it; its
    /// counter nothing there, `AheadWorth::counter`), and its shift in the blinds ahead (its
    /// numbers' and its counter's, added)
    fn of(&self, now: Option<JokerNow>) -> Grown {
        let counter = now.and_then(|n| n.counter).map_or(0.0, |v| self.ahead.counter_of(v));
        let now = now.map(|n| n.numbers);
        let linear = |n: [f64; 3]| (0..3).map(|f| self.per[f] * (n[f] - self.start[f])).sum::<f64>();
        let long = match (now, self.floor) {
            (Some(n), true) => linear(n).max(self.gone),
            (None, true) => self.gone,
            (Some(n), false) => linear(n),
            (None, false) => linear([0.0, 1.0, 0.0]),
        };
        // numbers and counter together never below losing it, where that loss was measured
        let ahead = self.ahead.of(&self.start, now) + counter;
        let ahead = if self.ahead.lost { ahead.max(self.ahead.gone) } else { ahead };
        Grown { long: (1.0 + long).max(LEAST_FACTOR).ln(), ahead }
    }
}

/// A counted joker as the board stands (`RoundGoals::jokers_now`): its numbers
/// (`Joker::lasting`) and its counter (`Joker::counter`, `None`: it has none)
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JokerNow {
    pub numbers: [f64; 3],
    pub counter: Option<f64>,
}

/// The counted jokers before a hand or a discard, by `RoundGoals::jokers` entry (`None`: used up)
type JokersNow = Vec<Option<JokerNow>>;

/// `RoundGoals::jokers_now` on the board's goals
fn jokers_now(b: &Board) -> Option<JokersNow> {
    b.goals.as_ref().and_then(|g| g.jokers_now(b))
}

/// `RoundGoals::grown` since `was` (`jokers_now`)
fn grown_since(b: &Board, was: &Option<JokersNow>) -> Grown {
    match (&b.goals, was) {
        (Some(g), Some(w)) => g.grown(w, b),
        _ => Grown::default(),
    }
}

impl RoundGoals {
    /// A simulated round's value: 0 if lost, else what it leaves you as a choice is worth (the
    /// chance to get through, times the long-run value of what it leaves): 1 plus its planets,
    /// money and the card it drew by Ante 8, times the factor the jokers' numbers leave the run's
    /// value at by Ante 8 (`Grown::long`), times the chance of getting through this ante's blinds
    /// still ahead with what it leaves against the chance with your board as it is (`ahead`
    /// shifted by the jokers', the planets' and the money's shifts: `chance_ratio`). Each part is
    /// at least nothing, so a won round is never worth less than a lost one (money a boss takes,
    /// The Tooth, the only part that could pass it, is floored). `o.grown` is against the
    /// analysis start (an outcome from a board mid-round starts at `at` of it), so outcomes from
    /// any point of a round are valued alike.
    pub fn value(&self, o: &Outcome) -> f64 {
        let (planet, planet_ahead) = o.last.map_or((0.0, 0.0), |h| (self.planet[h as usize], self.planet_ahead[h as usize]));
        let money = o.spare * self.per_hand + o.cash;
        let long = (1.0 + planet * o.planets + self.dollar * money + o.seen).max(0.0) * o.grown.long.exp();
        o.won * long * chance_ratio(self.ahead, o.grown.ahead + planet_ahead * o.planets + self.dollar_ahead * money, 0.0)
    }

    /// What `add` adds to a won round (`value`) where the jokers' numbers are at `at` (`at`, as
    /// a round so far left them): the one measure for the simulated player's thresholds
    fn gain(&self, at: Grown, add: Outcome) -> f64 {
        let won = Outcome { won: 1.0, grown: at, ..Default::default() };
        self.value(&Outcome { won: 1.0, grown: at + add.grown, ..add }) - self.value(&won)
    }

    /// What a dollar more adds to a won round (`gain`), the jokers at `at`
    pub fn dollar_worth(&self, at: Grown) -> f64 {
        self.gain(at, Outcome { cash: 1.0, ..Default::default() })
    }

    /// What a planet of hand `h` adds to a won round whose last hand is `h` (`gain`), the jokers
    /// at `at`
    pub fn planet_worth(&self, h: HandType, at: Grown) -> f64 {
        let last = Outcome { won: 1.0, grown: at, last: Some(h), ..Default::default() };
        self.value(&Outcome { planets: 1.0, ..last }) - self.value(&last)
    }

    /// What the counted jokers' numbers on `b` are worth against the analysis start
    /// (`JokerWorth::of`, both parts): where a round's outcome starts from, so every outcome is
    /// valued against the same start
    pub fn at(&self, b: &Board) -> Grown {
        self.jokers_now(b).map_or(Grown::default(), |now| self.at_now(&now))
    }

    fn at_now(&self, now: &[Option<JokerNow>]) -> Grown {
        // against each joker at its start, so the analysis start is none in both parts (a joker
        // the projection keeps though weaker than a typical find, an eternal one, has a floor,
        // `gone` by Ante 8, above nothing)
        self.jokers.iter().zip(now).fold(Grown::default(), |s, (w, n)| s + (w.of(*n) - w.of(Some(w.at_start()))))
    }

    /// The counted jokers' numbers and counters now (`None`: used up), by entry, for `grown`;
    /// `None` when none is counted
    pub fn jokers_now(&self, b: &Board) -> Option<JokersNow> {
        (!self.jokers.is_empty()).then(|| self.jokers.iter().map(|w| b.jokers.iter().find(|j| j.id == w.id).map(|j| JokerNow { numbers: j.lasting(), counter: j.counter(b.hands_played) })).collect())
    }

    /// What a hand or a discard changed on the counted jokers since `was` (`jokers_now` before
    /// it), by what each is worth before and after it (`JokerWorth::of`), each found by its
    /// place on your board (`Joker::id`, kept through the round's steps)
    pub fn grown(&self, was: &[Option<JokerNow>], b: &Board) -> Grown {
        let now = self.jokers_now(b).unwrap_or_default();
        self.jokers.iter().zip(was).zip(&now).fold(Grown::default(), |s, ((w, before), after)| s + (w.of(*after) - w.of(*before)))
    }

    /// `grown` as what it adds to a won round (`gain`), from the jokers at `was`
    pub fn grown_worth(&self, was: &[Option<JokerNow>], b: &Board) -> f64 {
        self.gain(self.at_now(was), Outcome { grown: self.grown(was, b), ..Default::default() })
    }

    /// What `b` took off the jokers since `was` (a reset, a joker used up), joker by joker, as
    /// `grown_worth` counts it, with what it added left out (so one joker's growth doesn't hide
    /// another's loss): none or less
    pub fn lost_worth(&self, was: &[Option<JokerNow>], b: &Board) -> f64 {
        let now = self.jokers_now(b).unwrap_or_default();
        let lost = self.jokers.iter().zip(was).zip(&now).fold(Grown::default(), |s, ((w, before), after)| {
            let d = w.of(*after) - w.of(*before);
            s + Grown { long: d.long.min(0.0), ahead: d.ahead.min(0.0) }
        });
        self.gain(self.at_now(was), Outcome { grown: lost, ..Default::default() }).min(0.0)
    }

    /// The most a card in `cards` adds (`seen`); a face-down card none (you can't tell it's the
    /// one to use the consumable on)
    pub fn seen_gain(&self, cards: &[Card]) -> f64 {
        cards.iter().filter(|c| !c.face_down).flat_map(|c| self.seen.iter().filter(move |(_, k, _)| k.same_kind(c)).map(|(_, _, g)| *g)).fold(0.0, f64::max)
    }
}

/// Simulated futures per choice when the round simulation weighs winning now against
/// playing on.
const LOOKAHEAD_ROLLOUTS: usize = 8;

/// The least a different finish could add (in `RoundGoals::value`, where winning is 1) for the
/// round simulation to look ahead at all (`play_on_instead`): a cost cut.
const LOOKAHEAD_MIN: f64 = 0.01;

/// A win is on the table: is playing on for a better finish worth more? Compares winning now
/// with `win` against playing on, on a few simulated futures, by `RoundGoals`: the move the
/// policy would make if it weren't finishing (digging with the cards that don't pay at round
/// end), when keeping them could add something (a Blue Seal's planet, the best hand's, a Gold
/// card's money, against what this finish keeps); the play that doesn't
/// win yet and grows the jokers most (`growth_play`), when that growth could be worth more than
/// the hand it spends. The futures play on without looking ahead, by the policy alone
/// (`policy_outcomes`).
#[allow(clippy::too_many_arguments)]
/// `finish`: the finish with its uses (`finish_with_uses`: board, hand, winning play, the
/// consumables still held after them), when there is one, else winning now with `win`;
/// `seen_by`: the round's `seen` credit so far.
#[allow(clippy::type_complexity)]
fn play_on_instead(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, scored: f64, target: f64, size: usize, win: &[usize], finish: Option<(&Board, &[Card], &[usize], &[Use])>, seen_by: &[(String, f64)], uses: &[Use], rng: &Rng) -> Option<Action> {
    let g = b.goals.as_ref()?;
    if hands <= 1 || g.no_lookahead {
        return None;
    }
    // the finish as it would be played (with its uses)
    let (fb, fh, fw, fu) = finish.map_or((b, hand, win, uses), |f| (f.0, f.1, f.2, f.3));
    let played: Vec<Card> = fw.iter().map(|&i| fh[i]).collect();
    let held: Vec<Card> = (0..fh.len()).filter(|i| !fw.contains(i)).map(|i| fh[i]).collect();
    let h = score::score(fb, &played, &held, &mut Unlucky, false).hand;
    // the most a different finish could add by the cards that pay at round end: every one in
    // hand kept, a Blue Seal's planet the best one, against what this finish keeps
    let paying: Vec<Card> = pays_at_end(b, hand).into_iter().map(|i| hand[i]).collect();
    let at = g.at(b);
    let best = HandType::ALL.iter().map(|&h| g.planet_worth(h, at)).fold(0.0, f64::max);
    let dollar = g.dollar_worth(at);
    let most = best * seal_planets(b, &paying) + dollar * held_dollars(&paying);
    // what this finish keeps of them once the press-play step went (The Hook), on average
    let kept = over_hook_sets(fb, &held, |left| (g.planet_worth(h, g.at(fb)) * seal_planets(fb, left) + dollar * held_dollars(left), 0.0)).0;
    let for_held = most - kept >= LOOKAHEAD_MIN;
    // what winning now takes off the jokers (Ride the Bus reset by a scored face card), which
    // playing on can spare; what it adds is still there to win later
    let loses = g.jokers_now(fb).map_or(0.0, |was| {
        let mut after = fb.clone();
        let _ = after.after_hand(&score::score(fb, &played, &held, &mut Unlucky, false));
        -g.lost_worth(&was, &after)
    });
    let for_growth = growth_play(b, g, hand, target - scored, win, loses);
    if !for_held && for_growth.is_none() {
        return None;
    }
    let mut alts: Vec<Move> = vec![];
    if for_held {
        // what the policy does when not finishing now, with the cards that pay at round end kept
        let keep = pays_at_end(b, hand);
        let free: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i)).collect();
        let cards: Vec<Card> = free.iter().map(|&i| hand[i]).collect();
        let alt = match decide_cards(b, &cards, deck, hands, discards, f64::INFINITY, size.saturating_sub(keep.len()).max(1), true) {
            Action::Play(v, _) => Move::Play(v.into_iter().map(|k| free[k]).collect()),
            Action::Discard(v) => Move::Discard(v.into_iter().map(|k| free[k]).collect()),
        };
        let same = |v: &[usize]| {
            let mut a = v.to_vec();
            let mut w = win.to_vec();
            a.sort();
            w.sort();
            a == w
        };
        if !matches!(&alt, Move::Play(v) if same(v) || v.is_empty()) {
            alts.push(alt);
        }
    }
    if let Some(v) = for_growth {
        alts.push(Move::Play(v));
    }
    if alts.is_empty() {
        return None;
    }
    // winning now (with its uses) and each way of playing on, on the same futures
    let start = RoundStart::at_decision(hand, deck, size, hands, discards, scored, target);
    let fstart = RoundStart { hand: fh.to_vec(), ..start.clone() };
    let mut moves: Vec<(&Board, &RoundStart, Move, &[Use])> = vec![(fb, &fstart, Move::Play(fw.to_vec()), fu)];
    moves.extend(alts.iter().map(|m| (b, &start, m.clone(), uses)));
    let vals = lookahead_values(&moves, seen_by, lookahead_seed(rng, hands, discards));
    paired_pick(&vals).map(|k| match alts.swap_remove(k - 1) {
        Move::Play(v) => Action::Play(v, true),
        Move::Discard(v) => Action::Discard(v),
    })
}

/// The look-ahead's futures' seed, from the round's random stream without drawing from it (a
/// round's later rolls and The Hook's picks stay the same whether or not it looked ahead), and
/// the hands and discards left (each decision of a round has its own)
fn lookahead_seed(rng: &Rng, hands: i64, discards: i64) -> u64 {
    rng.clone().next_u64() ^ 0x6c6f_6f6b_6168_6561 ^ ((hands as u64) << 16 | discards as u64)
}

/// Each move from its board and round start, with the consumables still held, valued by the
/// board's `RoundGoals` on the same few simulated futures (`LOOKAHEAD_ROLLOUTS`, from `seed`),
/// each future's value: they play to the same goals without looking ahead again, by the policy
/// alone (`policy_outcomes`); `seen_by`: the round's `seen` credit so far (less what a move's
/// board has used up).
#[allow(clippy::type_complexity)]
fn lookahead_values(moves: &[(&Board, &RoundStart, Move, &[Use])], seen_by: &[(String, f64)], seed: u64) -> Vec<Vec<f64>> {
    lookahead_values_in(moves, seen_by, seed, 0..LOOKAHEAD_ROLLOUTS)
}

/// `lookahead_values` on the futures numbered `range`
#[allow(clippy::type_complexity)]
fn lookahead_values_in(moves: &[(&Board, &RoundStart, Move, &[Use])], seen_by: &[(String, f64)], seed: u64, range: std::ops::Range<usize>) -> Vec<Vec<f64>> {
    moves
        .iter()
        .map(|(b, start, m, uses)| {
            let Some(g) = b.goals.as_ref() else { return vec![0.0; LOOKAHEAD_ROLLOUTS] };
            let mut fb = (*b).clone();
            if let Some(pg) = fb.goals.as_mut() {
                pg.no_lookahead = true;
            }
            let mut carried = seen_by.to_vec();
            drop_used_seen(&fb, &mut carried);
            if let Some(pg) = fb.goals.as_mut() {
                pg.carried = carried;
            }
            policy_outcomes(&fb, start, m, range.clone(), seed, uses).iter().map(|o| g.value(o)).collect()
        })
        .collect()
}

/// Of moves valued on the same rounds (`vals[i]`: move i's value on each), the one clearly
/// better than the first: its mean gain on the first, paired round by round, above two standard
/// errors of it, the largest such gain (the first move on none); `None` when the first stays.
fn paired_pick(vals: &[Vec<f64>]) -> Option<usize> {
    let r = vals.first()?.len();
    if r < 2 {
        return None;
    }
    let mut pick = 0;
    let mut gain = 0.0;
    for (i, v) in vals.iter().enumerate().skip(1) {
        let d: Vec<f64> = v.iter().zip(&vals[0]).map(|(x, y)| x - y).collect();
        let m = d.iter().sum::<f64>() / r as f64;
        let sd = (d.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (r - 1) as f64).sqrt();
        if m > 2.0 * sd / (r as f64).sqrt() + 1e-9 && m > gain {
            pick = i;
            gain = m;
        }
    }
    (pick > 0).then_some(pick)
}

/// The play to grow the jokers a round changes before winning (`play_on_instead`): of the plays
/// `best_play` tries in `hand` that don't surely win yet (floor below `need`), the one that
/// grows them most (`RoundGoals::grown`), then one that leaves the winning play `win` in hand,
/// then the highest floor; only when its growth and what winning now `loses` the jokers (which
/// playing on can spare) are worth more than the hand it spends (a spare hand's money) by
/// `LOOKAHEAD_MIN`, the most playing on could add; topped up with fillers (`with_fillers`) when
/// they leave its growth as it is. None without such jokers.
fn growth_play(b: &Board, g: &RoundGoals, hand: &[Card], need: f64, win: &[usize], loses: f64) -> Option<Vec<usize>> {
    let was = g.jokers_now(b)?;
    let mut best: Option<(f64, bool, f64, Vec<usize>)> = None;
    for &(mask, _, _, _, floor, _) in plays_tried(b, hand).iter().filter(|s| s.5 < need) {
        let idx = play_of(hand, mask);
        let grown = play_growth(b, g, &was, hand, &idx);
        let keeps = !idx.iter().any(|i| win.contains(i));
        let better = best.as_ref().is_none_or(|(bg, bk, bf, _)| {
            if (grown - bg).abs() > 1e-12 {
                grown > *bg
            } else if keeps != *bk {
                keeps
            } else {
                floor > *bf
            }
        });
        if better {
            best = Some((grown, keeps, floor, idx));
        }
    }
    let (grown, keeps, _, idx) = best.filter(|x| x.0 + loses - g.dollar_worth(g.at_now(&was)) * g.per_hand >= LOOKAHEAD_MIN)?;
    // topped up with the cards a play can spare (`with_fillers`: more drawn next hand) when that
    // grows the jokers as much and leaves the winning play in hand as the play did
    let filled = with_fillers(b, hand, &idx);
    let keeps_too = |v: &[usize]| !keeps || !v.iter().any(|i| win.contains(i));
    Some(if filled.len() > idx.len() && keeps_too(&filled) && play_growth(b, g, &was, hand, &filled) >= grown - 1e-12 { filled } else { idx })
}

/// What playing `idx` (cards of `hand`) changes on the jokers the round's goals count
/// (`RoundGoals::grown` from `was`, `jokers_now`): the hand scored with every roll failing and
/// stepped (`Board::after_hand`)
fn play_growth(b: &Board, g: &RoundGoals, was: &[Option<JokerNow>], hand: &[Card], idx: &[usize]) -> f64 {
    let played: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
    let mut after = b.clone();
    let _ = after.after_hand(&score::score(b, &played, &held, &mut Unlucky, false));
    g.grown_worth(was, &after)
}

/// Simulated futures per choice when a play that doesn't win is weighed for growth
/// (`spare_instead`): twice the look-ahead's, as 8 took Square's 4-card play on noise
/// (`player_values` against the player before, before the pace screen: the Square board −0.0026
/// ± 0.0016 of a round with 8, −0.0008 ± 0.0007 with 16; Spare Trousers +0.0067 and +0.0104 with
/// 8, +0.0132 and +0.0183 with 16); the second half only when the first shows a gain (a budget:
/// it only ever rejects)
const SPARE_ROLLOUTS: usize = 16;

/// No win on the table: is a play that grows the jokers the round's goals count clearly more
/// worth more than the policy's `chosen` play (chosen by points: the best play, or a junk hand
/// that digs)? The play `sparing_play` gives (Ride the Bus spared a scored face card, Obelisk its
/// reset, Square's 4 cards, Spare Trousers' Two Pair), looked at only when the round played on
/// after it keeps pace and scores at least what it would after `chosen` (`line_pace`, both alike:
/// each once, then the policy's best play on the board it leaves, so what a reset or a growth
/// does to this round's later hands is in it; a budget, in the register: a play that gives up
/// points by the policy's own measure isn't weighed), then weighed against
/// `chosen` as `play_on_instead` weighs a finish: by `RoundGoals::value` on the same futures
/// (`SPARE_ROLLOUTS`, `lookahead_values_in`: they play on by the policy without looking ahead, so
/// a growth a later play would reset is weighed as the policy would reset it), taken only when
/// clearly better (`paired_pick`). None without goals that count jokers, inside a future, or
/// without such a play.
#[allow(clippy::too_many_arguments)]
fn spare_instead(b: &Board, parts: &HandParts, deck: &[Card], hands: i64, discards: i64, scored: f64, target: f64, size: usize, chosen: &[usize], seen_by: &[(String, f64)], uses: &[Use], rng: &Rng) -> Option<Vec<usize>> {
    let g = b.goals.as_ref().filter(|g| !g.no_lookahead)?;
    let was = g.jokers_now(b)?;
    let hand = parts.hand;
    if chosen.is_empty() {
        return None;
    }
    let (idx, spare) = sparing_play(b, g, &was, hand, &parts.plays(), chosen, hands)?;
    let pace = line_pace(b, hand, &idx, hands);
    if pace < target - scored || pace < line_pace(b, hand, chosen, hands) {
        return None;
    }
    // the second half of the futures only when the first shows a gain
    let round = RoundStart::at_decision(hand, deck, size, hands, discards, scored, target);
    let moves = [(b, &round, Move::Play(chosen.to_vec()), uses), (b, &round, Move::Play(spare.clone()), uses)];
    let seed = lookahead_seed(rng, hands, discards);
    let half = SPARE_ROLLOUTS / 2;
    let mut vals = lookahead_values_in(&moves, seen_by, seed, 0..half);
    if vals[1].iter().sum::<f64>() <= vals[0].iter().sum::<f64>() {
        return None;
    }
    for (v, more) in vals.iter_mut().zip(lookahead_values_in(&moves, seen_by, seed, half..SPARE_ROLLOUTS)) {
        v.extend(more);
    }
    paired_pick(&vals).map(|_| spare)
}

/// The play `spare_instead` weighs against `chosen` (and the oracle tries): of `plays` (the
/// hand's, as `best_play` tries them; cards that pay at the end of the round left out while a
/// hand is left after it, as the policy keeps them), the one with the highest floor among those
/// that grow the jokers the goals count (`play_growth`, from `was`) by at least `LOOKAHEAD_MIN`
/// more than `chosen` does (one tried: a budget). The play, and the play topped up with the
/// cards it can spare (`with_fillers`) when that grows the jokers as much.
fn sparing_play(b: &Board, g: &RoundGoals, was: &[Option<JokerNow>], hand: &[Card], plays: &[Scored], chosen: &[usize], hands: i64) -> Option<(Vec<usize>, Vec<usize>)> {
    let base = play_growth(b, g, was, hand, chosen);
    // (a play covers the first `SUBSETS_MAX` cards, as `play_of`)
    let keep: u32 = if hands > 1 { pays_at_end(b, hand).iter().filter(|&&i| i < SUBSETS_MAX).fold(0, |m, &i| m | 1 << i) } else { 0 };
    let mut order: Vec<&Scored> = plays.iter().filter(|s| s.0 & keep == 0).collect();
    order.sort_by(|x, y| y.4.total_cmp(&x.4).then(x.0.cmp(&y.0)));
    let (idx, grown) = order.iter().find_map(|s| {
        let idx = play_of(hand, s.0);
        let grown = play_growth(b, g, was, hand, &idx);
        (grown >= base + LOOKAHEAD_MIN).then_some((idx, grown))
    })?;
    let filled = with_fillers(b, hand, &idx);
    let spare = if filled.len() > idx.len() && filled.iter().all(|&i| i >= SUBSETS_MAX || keep & (1 << i) == 0) && play_growth(b, g, was, hand, &filled) >= grown - 1e-12 { filled } else { idx.clone() };
    Some((idx, spare))
}

/// What playing `idx` (cards of `hand`) now and playing on after it scores over `hands` hands:
/// its average now, then on the board it leaves (`Board::after_hand`, without The Eye's ban, as
/// `playing_on` steps it) the best play and playing on from there (`playing_on`, the one pace
/// every decision uses), so a play that spares a joker's reset, or a junk hand that digs, is
/// followed by the play the policy would make, not by itself again
fn line_pace(b: &Board, hand: &[Card], idx: &[usize], hands: i64) -> f64 {
    let played: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
    let mut board = b.clone();
    board.hands_left = hands;
    let o = score::score(&board, &played, &held, &mut Unlucky, false);
    let now = mean_score(&board, hand, idx, o.score);
    if hands <= 1 {
        return now;
    }
    let eye = board.blind.eye_seen;
    let _ = board.after_hand(&o);
    board.blind.eye_seen = eye;
    now + best_play(&board, hand).map_or(0.0, |p| playing_on(&board, hand, &p, hands - 1))
}

/// What winning now with `win` is worth (`RoundGoals::value`, the measure every move is ranked
/// by): the planets of the Blue Seals left in hand (the planet of the hand played), the money
/// it pays and the Gold cards' held, the hands left over, and the best card drawn for a
/// consumable still held (`seen`).
fn finish_value(b: &Board, g: &RoundGoals, hand: &[Card], win: &[usize], hands: i64, seen: f64) -> f64 {
    let played: Vec<Card> = win.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !win.contains(i)).map(|i| hand[i]).collect();
    let value = |s: &crate::engine::Outcome, money: f64, planets: f64| {
        // the jokers as the hand leaves them, against the analysis start
        let grown = {
            let mut after = b.clone();
            let _ = after.after_hand(s);
            g.at(&after)
        };
        let o = Outcome { won: 1.0, spare: (hands - 1) as f64, cash: s.dollars + money, planets, last: Some(s.hand), seen, grown, ..Default::default() };
        g.value(&o)
    };
    // the boss's press-play discard first (The Hook): when which cards go changes the hand, the
    // value of each set it could take, on average; else the hand once, with what the cards held
    // pay at round end on average (`held_at_end`)
    if score::hook_pick_matters(b, &held) {
        let sets = score::hook_sets(b, held.len());
        let total: f64 = sets
            .iter()
            .map(|gone| {
                let s = score::score(b, &played, &held, &mut crate::engine::Given(gone.clone()), false);
                let left: Vec<Card> = (0..held.len()).filter(|i| !gone.contains(i)).map(|i| held[i]).collect();
                value(&s, won_money(b, &left), seal_planets(b, &left))
            })
            .sum();
        return total / sets.len() as f64;
    }
    let s = score::score(b, &played, &held, &mut Unlucky, false);
    let (money, planets) = held_at_end(b, &held);
    value(&s, money, planets)
}

/// The average of `pays` over the cards held while the winning hand scores that are still held at
/// the round's end: every set the boss's press-play step could discard first (`score::hook_sets`:
/// The Hook's 2, any as likely), each once
fn over_hook_sets(b: &Board, held: &[Card], pays: impl Fn(&[Card]) -> (f64, f64)) -> (f64, f64) {
    let sets = score::hook_sets(b, held.len());
    if sets.is_empty() {
        return pays(held);
    }
    let (mut x, mut y) = (0.0, 0.0);
    for gone in &sets {
        let left: Vec<Card> = (0..held.len()).filter(|i| !gone.contains(i)).map(|i| held[i]).collect();
        let (a, c) = pays(&left);
        (x, y) = (x + a, y + c);
    }
    (x / sets.len() as f64, y / sets.len() as f64)
}

/// What the cards held while the winning hand scores pay at the round's end (`won_money`,
/// `seal_planets`), on average over what the press-play step leaves (`over_hook_sets`)
fn held_at_end(b: &Board, held: &[Card]) -> (f64, f64) {
    over_hook_sets(b, held, |h| (won_money(b, h), seal_planets(b, h)))
}

/// Drops the `seen` credit of consumables already used (theirs went with them)
fn drop_used_seen(b: &Board, seen_by: &mut Vec<(String, f64)>) {
    if let Some(g) = &b.goals {
        seen_by.retain(|(k, _)| g.seen.iter().any(|(k2, _, _)| k2 == k));
    }
}

/// Before a winning play: the held consumables whose use makes the win worth more
/// (`finish_value`: a Blue Seal or a Gold card more in hand at the end, a slot freed for a
/// planet), each with the effect it carries, the best first, as long as one adds something.
/// The board, hand and winning play after them, the consumables used, and the finish's value;
/// `None` when none adds anything.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn finish_with_uses(b: &Board, g: &RoundGoals, hand: &[Card], win: &[usize], uses: &[Use], used: &[bool], need: f64, hands: i64, seen_by: &[(String, f64)]) -> Option<(Board, Vec<Card>, Vec<usize>, Vec<usize>, f64)> {
    // the best card drawn for a consumable still held, after these are used (theirs is gone)
    let seen = |b: &Board| {
        let mut s = seen_by.to_vec();
        drop_used_seen(b, &mut s);
        s.iter().map(|e| e.1).fold(0.0, f64::max)
    };
    let mut cur = (b.clone(), hand.to_vec(), win.to_vec(), vec![], finish_value(b, g, hand, win, hands, seen(b)));
    let mut improved = false;
    loop {
        let mut best: Option<(Board, Vec<Card>, Vec<usize>, usize, f64)> = None;
        for (k, u) in uses.iter().enumerate() {
            if used[k] || cur.3.contains(&k) {
                continue;
            }
            let Some((b2, h2)) = u.apply(&cur.0, &cur.1) else { continue };
            let Some(w2) = finish_play(&b2, g, &h2, need, hands) else { continue };
            let v = finish_value(&b2, g, &h2, &w2, hands, seen(&b2));
            // the biggest gain first; equal gains by key (not slot order)
            let better = match &best {
                None => v > cur.4 + 1e-9,
                Some(x) => v > x.4 + 1e-9 || ((v - x.4).abs() <= 1e-9 && u.key < uses[x.3].key),
            };
            if better {
                best = Some((b2, h2, w2, k, v));
            }
        }
        match best {
            Some((b2, h2, w2, k, v)) => {
                cur.3.push(k);
                cur = (b2, h2, w2, cur.3, v);
                improved = true;
            }
            None => break,
        }
    }
    improved.then_some(cur)
}

/// Notes, per consumable, the best `RoundGoals::seen` gain among the cards in `hand`; a face-down
/// card none, as `RoundGoals::seen_gain` (you can't tell it's the one to use it on), so the
/// decisions that read the credit don't read the card (gap 25 (a))
fn note_seen(b: &Board, hand: &[Card], acc: &mut Vec<(String, f64)>) {
    let Some(g) = &b.goals else { return };
    for (k, card, gain) in &g.seen {
        if hand.iter().any(|c| !c.face_down && c.same_kind(card)) {
            match acc.iter_mut().find(|e| &e.0 == k) {
                Some(e) => e.1 = e.1.max(*gain),
                None => acc.push((k.clone(), *gain)),
            }
        }
    }
}

/// Uses held consumables for score: each that lifts the best play in hand by more than 1%,
/// the biggest lift first (ties by key, not slot order). One that would put a card that pays
/// at round end into your hand (`pays_at_end`: a Blue Seal, a Gold card) is held while the
/// best hand keeps you on pace for the `need` left in `hands`: used at the finish
/// (`finish_with_uses`) it's worth more. Others gain nothing by waiting. The ones used, in order.
fn use_if_better(b: &mut Board, hand: &mut Vec<Card>, uses: &[Use], used: &mut [bool], need: f64, hands: i64) -> Vec<usize> {
    let mut order = vec![];
    while used.iter().any(|u| !u) {
        let now = best_play(b, hand).map_or(0.0, |p| p.mean);
        let on_pace = keeps_pace(b, hand, hands.max(1), need);
        let pays = |b: &Board, h: &[Card]| pays_at_end(b, h).len();
        let mut best: Option<(f64, usize, Board, Vec<Card>)> = None;
        for (k, u) in uses.iter().enumerate() {
            if used[k] {
                continue;
            }
            let Some((b2, h2)) = u.apply(b, hand) else { continue };
            // on the same board (the slot it frees is freed whenever it's used)
            if on_pace && pays(&b2, &h2) > pays(&b2, hand) {
                continue;
            }
            let lift = best_play(&b2, &h2).map_or(0.0, |p| p.mean);
            if lift <= now * 1.01 {
                continue;
            }
            let better = best.as_ref().is_none_or(|(l, kb, _, _)| lift > *l || (lift == *l && u.key < uses[*kb].key));
            if better {
                best = Some((lift, k, b2, h2));
            }
        }
        let Some((_, k, b2, h2)) = best else { return order };
        *b = b2;
        *hand = h2;
        used[k] = true;
        order.push(k);
    }
    order
}

/// Deals of the round's cards a Verdant Leaf sale is weighed on besides the hand held
/// (`leaf_sale`): a budget
const LEAF_DEALS: usize = 8;

/// Verdant Leaf (blind.lua `Blind:debuff_card`: every card debuffed until a joker is sold, card.lua
/// `Card:sell_card`): the joker the player sells (`leaf_choice`), sold, and the blind's debuffs set
/// again on every card (`Board::debuff_cards`). Returns the money.
fn leaf_sale(b: &mut Board, hand: &mut [Card], deck: &mut [Card], size: usize, hands: i64) -> f64 {
    match leaf_choice(b, hand, deck, size, hands) {
        Some(j) => {
            let money = b.sell_joker(j);
            b.debuff_cards(hand, deck);
            money
        }
        None => 0.0,
    }
}

/// The joker the player sells to lift Verdant Leaf (by its place on `b`), or none: the one whose
/// sale leaves the round scoring most, when that beats keeping them all; equal ones by the most
/// money, then by key, then by place. What the round scores: the policy's own pace (`playing_on`
/// over the `hands` left: the jokers a round's later hands grow or pay, Card Sharp's repeated hand)
/// from the hand held and from `LEAF_DEALS` deals of `size` from every card the round still has
/// (hand and pile, in a fixed order, on a seed of their own): the hand held alone missed the joker
/// the round needs (Shoot the Moon with the Queens in the pile, Card Sharp). An eternal joker
/// can't be sold. Remembered per board, hand and cards left (the same in every round of a
/// start whose hand is dealt: Best play's moves, a round in progress's runs).
pub fn leaf_choice(b: &Board, hand: &[Card], deck: &[Card], size: usize, hands: i64) -> Option<usize> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::hash::{Hash, Hasher};
    thread_local! {
        static CACHE: RefCell<HashMap<u64, Option<usize>>> = RefCell::new(HashMap::new());
    }
    if b.jokers.iter().all(|j| j.eternal) {
        return None;
    }
    let mut all: Vec<Card> = hand.iter().chain(deck.iter()).copied().collect();
    all.sort_by(Card::order_cmp);
    let mut h = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(b).unwrap_or_default().hash(&mut h);
    for c in hand {
        c.order_key().hash(&mut h);
    }
    (hand.len(), size, hands).hash(&mut h);
    for c in &all {
        c.order_key().hash(&mut h);
    }
    let key = h.finish();
    if let Some(c) = CACHE.with(|c| c.borrow().get(&key).copied()) {
        return c;
    }
    let mut rng = Rng::new(0x1eaf);
    let mut deals = vec![hand.to_vec()];
    for _ in 0..LEAF_DEALS {
        shuffle(&mut all, &mut rng);
        deals.push(all[..size.min(all.len())].to_vec());
    }
    let worth = |b: &Board| {
        deals
            .iter()
            .map(|d| {
                let mut d = d.clone();
                b.debuff_cards(&mut d, &mut []);
                best_play(b, &d).map_or(0.0, |p| playing_on(b, &d, &p, hands))
            })
            .sum::<f64>()
    };
    let now = worth(b);
    let mut best: Option<(f64, f64, usize)> = None;
    for j in (0..b.jokers.len()).filter(|&j| !b.jokers[j].eternal) {
        let mut b2 = b.clone();
        let money = b2.sell_joker(j);
        let w = worth(&b2);
        let better = best.is_none_or(|(m, d, k)| w > m + 1e-9 || ((w - m).abs() <= 1e-9 && (money > d || (money == d && b.jokers[j].key < b.jokers[k].key))));
        if better {
            best = Some((w, money, j));
        }
    }
    let choice = best.filter(|&(w, _, _)| w > now + 1e-9).map(|(_, _, j)| j);
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 4096 {
            c.clear();
        }
        c.insert(key, choice);
    });
    choice
}

/// `sim_round` with consumables held (see `Use`).
pub fn sim_round_uses(board: &Board, start: &RoundStart, rng: &mut Rng, uses: &[Use]) -> RoundResult {
    play_round(board, start, rng, uses).0
}

/// One decision of the round's player on the hand it holds (`play_round`): the held consumables
/// it uses first (their places in `uses`, in the order used; done on `b` and `hand`, `used` and
/// `seen_by` kept up), then what it plays or throws, by its places in the hand they leave.
#[allow(clippy::too_many_arguments)]
fn decide_step(b: &mut Board, hand: &mut Vec<Card>, deck: &[Card], used: &mut [bool], seen_by: &mut Vec<(String, f64)>, uses: &[Use], hands: i64, discards: i64, total: f64, target: f64, size: usize, rng: &mut Rng) -> (Vec<usize>, Action) {
    let (order, act, ..) = decide_step_open(b, hand, deck, used, seen_by, uses, hands, discards, total, target, size, rng);
    (order, act)
}

/// `decide_step`, whether it decided with no finish on the table (where the oracle,
/// `set_oracle`, weighs the decision: `decide_unseen` asks it there only), whether its best
/// play surely reaches the score (a win on the table by the policy's own test, `decide_cards`,
/// with goals or without: where `decide_unseen` values its choice), and, where it used no
/// consumable, what its search of the hand found (`HandParts::searched`: of the board and hand
/// it was given, as nothing changed them; none where one changed them)
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn decide_step_open(b: &mut Board, hand: &mut Vec<Card>, deck: &[Card], used: &mut [bool], seen_by: &mut Vec<(String, f64)>, uses: &[Use], hands: i64, discards: i64, total: f64, target: f64, size: usize, rng: &mut Rng) -> (Vec<usize>, Action, bool, bool, Option<Searched>) {
    let mut order = use_if_better(b, hand, uses, used, target - total, hands);
    drop_used_seen(b, seen_by);
    // a win on the table: the finish (`finish_play` by the round's goals; without them, while
    // a card that pays at round end can stay in hand) and whether to play on instead
    // the whole hand's search, shared with the decision when nothing wins (`decide_with`)
    let parts = HandParts::new(b, hand);
    let all: Vec<usize> = (0..hand.len()).collect();
    let win = match &b.goals {
        Some(g) => parts.best(&all).filter(|p| p.surest >= target - total).and_then(|_| finish_among(b, g, hand, target - total, hands, &parts.plays())),
        None => win_keeping_seals(b, hand, target - total),
    };
    let open = win.is_none();
    // a best play that surely reaches the score (without goals or a card that pays at round end
    // held, `win_keeping_seals` finds no finish, though the policy then plays the win)
    let sure = win.is_some() || parts.best(&all).is_some_and(|p| p.surest >= target - total);
    // the board and hand a finish using consumables leaves, taken on once the search's finds of
    // this hand are handed on
    let mut finished: Option<(Board, Vec<Card>)> = None;
    let act = match win {
        Some(v) => {
            // the finish, with any held consumable that makes it worth more used first
            let fin = b.goals.as_ref().filter(|_| used.iter().any(|u| !u)).and_then(|g| finish_with_uses(b, g, hand, &v, uses, used, target - total, hands, seen_by));
            let unused: Vec<Use> = uses.iter().zip(used.iter()).filter(|(_, u)| !**u).map(|(x, _)| x.clone()).collect();
            // the finish's own: without the ones it uses
            let fin_unused: Vec<Use> = fin.as_ref().map_or(vec![], |f| uses.iter().enumerate().filter(|(k, _)| !used[*k] && !f.3.contains(k)).map(|(_, x)| x.clone()).collect());
            let finish = fin.as_ref().map(|f| (&f.0, f.1.as_slice(), f.2.as_slice(), fin_unused.as_slice()));
            match play_on_instead(b, hand, deck, hands, discards, total, target, size, &v, finish, seen_by, &unused, rng) {
                Some(a) => a,
                None => match fin {
                    Some((b2, h2, v2, ks, _)) => {
                        finished = Some((b2, h2));
                        ks.iter().for_each(|&k| used[k] = true);
                        order.extend(ks);
                        Action::Play(v2, false)
                    }
                    // a discard would pay: the policy may cash it first, while the round
                    // stays safe (`decide_cards`, a threshold in the register), never with a
                    // card that pays at round end; else the finish
                    None if b.goals.is_some() && discards > 0 && discard_money(b, hand, &(0..hand.len()).collect::<Vec<_>>()) > 0.0 => match decide_with(b, hand, deck, hands, discards, target - total, size, Some(&parts)) {
                        Action::Discard(d) if discard_money(b, hand, &d) > 0.0 && !pays_at_end(b, hand).iter().any(|i| d.contains(i)) => Action::Discard(d),
                        _ => Action::Play(v, false),
                    },
                    None => Action::Play(v, false),
                },
            }
        }
        None => {
            // the yardstick (`set_oracle`), when it's on
            let oracle = (ORACLE.with(|o| o.get()) > 0)
                .then(|| {
                    let unused: Vec<Use> = uses.iter().zip(used.iter()).filter(|(_, u)| !**u).map(|(x, _)| x.clone()).collect();
                    oracle_decide(b, hand, deck, hands, discards, total, target, size, &unused, rng)
                })
                .flatten();
            // the whole hand's search shared with the decision (made above for the win test);
            // with goals, a play that grows the jokers less than another that keeps pace may
            // give way to it (`spare_instead`)
            oracle.unwrap_or_else(|| {
                let need = target - total;
                match decide_with(b, hand, deck, hands, discards, need, size, Some(&parts)) {
                    Action::Play(v, dig) if b.goals.as_ref().is_some_and(|g| !g.no_lookahead && !g.jokers.is_empty()) => {
                        let unused: Vec<Use> = uses.iter().zip(used.iter()).filter(|(_, u)| !**u).map(|(x, _)| x.clone()).collect();
                        spare_instead(b, &parts, deck, hands, discards, total, target, size, &v, seen_by, &unused, rng).map_or(Action::Play(v, dig), |s| Action::Play(s, false))
                    }
                    a => a,
                }
            })
        }
    };
    let found = order.is_empty().then(|| parts.searched());
    if let Some((b2, h2)) = finished {
        *b = b2;
        *hand = h2;
        drop_used_seen(b, seen_by);
    }
    (order, act, open, sure, found)
}

/// How many draws of the face-down cards in hand the simulated player decides on and values its
/// choice over (`decide_unseen`): a budget, not a finding (8 measured 2026-11-07: within noise of
/// 4, at 1.5 times the player's cost)
const UNSEEN_DRAWS: usize = 4;

/// `decide_step` with a face-down card in `hand` (gap 25 (a)): the player can't see it, so it
/// decides on `UNSEEN_DRAWS` draws of what the face-down cards could be, each played as if seen
/// (a consumable held for a face-down card's place on what that draw put there: `Use::unplaced`),
/// and uses the consumable most draws use first (the earliest draw's on a tie: they're drawn at
/// random, not ordered by the cards), on the cards as they are, then decides again on the hand
/// it leaves (the cards it changed seen, as the game shows them); with none used first, of the
/// actions those draws make, where not every draw has a win on the table, the one the policy's own
/// estimates value most summed over every draw (`policy_estimates`, a heuristic: no futures, and
/// on what each draw's decision already searched of its hand, `HandParts::with`; on the round's
/// last hand, where none reaches the score needed, in points), else the one most of them make. It stops drawing once the draws so far
/// all decided the same and the rest can't outvote them (`unseen_done`: two alike of four; a
/// budget, as the later draws' dissent isn't valued). A draw keeps what you
/// can tell of them now (`Hidden`): their bounds among the cards you see in the game's two sorts,
/// their order among themselves, the boss's rule (`Board::turns_down_only_faces`), from the
/// unseen cards (the pile and themselves, as the game's deck view counts them); the pile it
/// decides with is the one that draw leaves, so it can't tell them by what's missing from it.
/// What a card seen before showed of them (one played since) isn't kept: you could remember it.
/// Where the consumable can't be used on the cards as they are (a card it goes on isn't
/// there), the next most used; none: the first draw's decision without the consumables.
#[allow(clippy::too_many_arguments)]
fn decide_unseen(b: &mut Board, hand: &mut Vec<Card>, deck: &[Card], used: &mut [bool], seen_by: &mut Vec<(String, f64)>, uses: &[Use], hands: i64, discards: i64, total: f64, target: f64, size: usize, rng: &Rng) -> Action {
    #[cfg(test)]
    UNSEEN_DECISIONS.with(|x| x.set(x.get() + 1));
    let slots: Vec<usize> = (0..hand.len()).filter(|&i| hand[i].face_down).collect();
    let known = Hidden::new(hand, &slots, slots.clone(), b.turns_down_only_faces(&b.blind.key));
    let live = vec![true; slots.len()];
    // from the round's stream without drawing from it (as `lookahead_seed`), so a move that
    // keeps a face-down card meets the same deals and rolls after it as one that doesn't
    let mut draws = Rng::new(lookahead_seed(rng, hands, discards) ^ 0x756e_7365_656e);
    // one draw: the hand as the player imagines it, the pile it leaves, and a seed for the decision
    let mut imagine = || {
        let (mut h, mut d) = known.deal(hand, deck, &live, &mut draws);
        for &s in &slots {
            h[s].face_down = false;
        }
        shuffle(&mut d, &mut draws);
        (h, d, draws.next_u64())
    };
    let first = imagine();
    let unplaced: Vec<Use> = uses.iter().map(|u| u.unplaced(hand)).collect();
    // the yardstick (`set_oracle`) weighs the decision made below on futures that draw the
    // face-down cards anew (`oracle_unseen`), never inside a draw, where it would see them
    let oracle = oracle_on();
    // each draw's decision: the consumables used, in order, and the action
    let mut steps: Vec<(Vec<usize>, Action)> = vec![];
    // whether some draw has no finish on the table (`decide_step_open`): one is on the table for
    // a player who can't see the face-down cards only when it is in every draw, and the oracle
    // stays out then, as it does on a hand you see with a finish on it
    let mut open = false;
    // whether some draw has no sure win (its best play surely reaching the score): the decisions
    // are valued over the draws only then
    let mut unsure = false;
    // each draw's hand and pile, for the valuation, and what its decision's search of that hand
    // found (`decide_step_open`)
    let mut seen_draws: Vec<(Vec<Card>, Vec<Card>)> = vec![];
    let mut found: Vec<Option<Searched>> = vec![];
    for k in 0..UNSEEN_DRAWS {
        let (mut h, d, seed) = if k == 0 { first.clone() } else { imagine() };
        seen_draws.push((h.clone(), d.clone()));
        let (mut b2, mut u2, mut s2) = (b.clone(), used.to_vec(), seen_by.clone());
        // the consumables held for a face-down card's place, on what this draw put there
        let imagined = uses_on(b, &unplaced, &h);
        let (order, act, o, sure, f) = without_oracle(|| decide_step_open(&mut b2, &mut h, &d, &mut u2, &mut s2, &imagined, hands, discards, total, target, size, &mut Rng::new(seed)));
        open |= o;
        unsure |= !sure;
        found.push(f);
        steps.push((order, act));
        // once the draws so far all agree and the draws left can't outvote them, the rest are
        // skipped: a budget, as a later draw that dissents could have been valued over them
        // (below), not a finding. Only on no consumable and an action:
        // where a consumable leads, whether it can be used decides between it and the next, and
        // where it shows every face-down card the decision after it is seeded from the draws'
        // stream past the last draw (`draws`), which stopping early would move.
        // Not for the yardstick, which weighs every draw's decision.
        if !oracle && unseen_done(&steps, UNSEEN_DRAWS - k - 1) {
            break;
        }
    }
    // the consumable most draws use first, or none (the earliest on a tie: a stable sort); the
    // next one is decided on the hand it leaves
    let (orders, made) = vote_tallies(&steps);
    // where not every draw has a win on the table (so none is on it for the player), none used
    // a consumable first, and they disagree: the action the policy's own estimates value most
    // over the draws (`policy_estimates`), the vote's order on a tie. Valuing only where no draw
    // had a win lost up to 2.9 points of round wins on the bench's boss boards (2026-11-07)
    let valued = (unsure && made.len() >= 2 && steps.iter().all(|x| x.0.is_empty())).then(|| {
        let acts: Vec<Action> = made.iter().map(|m| m.0.clone()).collect();
        // each draw on the board and hand its decision was made on (no consumable used), with
        // what that decision's search found: the same estimates, without searching the hand again
        let parts: Vec<HandParts> = seen_draws.iter().zip(found.drain(..)).map(|((h, _), f)| HandParts::with(b, h, f.unwrap_or_default())).collect();
        let sum_by = |reach: bool| {
            let mut sum = vec![0.0; acts.len()];
            for ((h, d), p) in seen_draws.iter().zip(&parts) {
                for (s, v) in sum.iter_mut().zip(policy_estimates(b, h, d, hands, discards, target - total, size, Some(p), reach, &acts)) {
                    *s += v;
                }
            }
            sum
        };
        // on the round's last hand, where none reaches the score needed on any draw, in points,
        // which say which comes nearest (as the policy weighs it there)
        let mut sum = sum_by(true);
        if hands <= 1 && sum.iter().all(|&v| v <= 0.0) {
            sum = sum_by(false);
        }
        let mut rank = vote_ranking(&tally_counts(&made));
        #[cfg(test)]
        let vote = rank.clone();
        rank.sort_by(|&x, &y| sum[y].total_cmp(&sum[x]));
        #[cfg(test)]
        UNSEEN_VALUED.with(|x| x.borrow_mut().push(UnseenValued { acts: acts.clone(), draws: seen_draws.clone(), sum: sum.clone(), vote, pick: rank[0] }));
        rank
    });
    for order in vote_ranking(&tally_counts(&orders)).into_iter().map(|i| &orders[i].0) {
        if order.is_empty() {
            // none: the action most of those draws make, the earliest on a tie
            let rank = valued.clone().unwrap_or_else(|| vote_ranking(&tally_counts(&made)));
            let act = made[rank[0]].0.clone();
            if oracle && open {
                // the other draws' actions, as the yardstick's alternatives, and the consumables
                // still held on the futures' own deals of the face-down cards
                let others: Vec<Action> = rank[1..].iter().map(|&i| made[i].0.clone()).collect();
                let unused: Vec<Use> = unplaced.iter().zip(used.iter()).filter(|(_, u)| !**u).map(|(x, _)| x.clone()).collect();
                let mut orng = Rng::new(draws.next_u64());
                return oracle_unseen(b, hand, deck, hands, discards, total, target, size, act.clone(), &others, &unused, &mut orng).unwrap_or(act);
            }
            return act;
        }
        let mut now = (b.clone(), hand.clone());
        if order.iter().all(|&k| uses[k].apply(&now.0, &now.1).map(|x| now = x).is_some()) {
            // using one is a move of its own, after which the cards it changed are seen (card.lua
            // `Card:use_consumeable` flips them face up): what's next (another consumable, the
            // play) is decided again on the hand it leaves, not what a draw chose for what it
            // imagined there
            (*b, *hand) = now;
            order.iter().for_each(|&k| used[k] = true);
            drop_used_seen(b, seen_by);
            return if hand.iter().any(|c| c.face_down) {
                decide_unseen(b, hand, deck, used, seen_by, uses, hands, discards, total, target, size, rng)
            } else {
                decide_step(b, hand, deck, used, seen_by, uses, hands, discards, total, target, size, &mut Rng::new(draws.next_u64())).1
            };
        }
    }
    let (mut h, d, seed) = first;
    let act = without_oracle(|| decide_step(&mut b.clone(), &mut h, &d, &mut used.to_vec(), &mut seen_by.clone(), &[], hands, discards, total, target, size, &mut Rng::new(seed)).1);
    if oracle && open {
        let unused: Vec<Use> = unplaced.iter().zip(used.iter()).filter(|(_, u)| !**u).map(|(x, _)| x.clone()).collect();
        let mut orng = Rng::new(draws.next_u64());
        return oracle_unseen(b, hand, deck, hands, discards, total, target, size, act.clone(), &[], &unused, &mut orng).unwrap_or(act);
    }
    act
}

/// How many fresh draws of the face-down cards the oracle takes its own alternatives from
/// (`oracle_unseen`): a budget of the yardstick's, apart from the player's (`UNSEEN_DRAWS`), so
/// a change to the player's doesn't move the oracle that judges it
const ORACLE_DRAWS: usize = 4;

/// The oracle (`set_oracle`) on a hand holding face-down cards: it can't see them either. It
/// weighs `base` (the player's decision on the draws) against the other draws' decisions
/// (`others`) and its own alternatives on fresh draws of the hand (`oracle_alts`, `ORACLE_DRAWS`
/// of them), each played first on futures that draw the face-down cards anew
/// (`RoundStart::hidden`, as Best play's first move does), the player playing on. With no card
/// in `hand` face down any more (a consumable used first showed them), the same on the hand as
/// it is. None: nothing else to weigh.
#[allow(clippy::too_many_arguments)]
fn oracle_unseen(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, scored: f64, target: f64, size: usize, base: Action, others: &[Action], uses: &[Use], rng: &mut Rng) -> Option<Action> {
    let slots: Vec<usize> = (0..hand.len()).filter(|&i| hand[i].face_down).collect();
    let mut alts: Vec<Move> = others
        .iter()
        .map(|a| match a {
            Action::Play(v, _) => Move::Play(v.clone()),
            Action::Discard(v) => Move::Discard(v.clone()),
        })
        .collect();
    if slots.is_empty() {
        alts.extend(oracle_alts(b, hand, deck, hands, discards, &base));
        let start = RoundStart::at_decision(hand, deck, size, hands, discards, scored, target);
        return oracle_pick(b, &start, base, alts, uses, rng);
    }
    let mut known = Hidden::new(hand, &slots, slots.clone(), b.turns_down_only_faces(&b.blind.key));
    known.stand = slots.iter().map(|&s| hand[s]).collect();
    let live = vec![true; slots.len()];
    for _ in 0..ORACLE_DRAWS {
        let (mut h, d) = known.deal(hand, deck, &live, rng);
        for &s in &slots {
            h[s].face_down = false;
        }
        alts.extend(oracle_alts(b, &h, &d, hands, discards, &base));
    }
    let start = RoundStart { hidden: Some(std::sync::Arc::new(known)), ..RoundStart::at_decision(hand, deck, size, hands, discards, scored, target) };
    oracle_pick(b, &start, base, alts, uses, rng)
}

/// The vote's tallies (`decide_unseen`) over each draw's decision (`steps`): each consumable a
/// draw uses first (none: empty) and how many draws use it, then the action of each draw using
/// none and how many make it (`same_step`), each in the order first made
#[allow(clippy::type_complexity)]
fn vote_tallies(steps: &[(Vec<usize>, Action)]) -> (Vec<(Vec<usize>, usize)>, Vec<(Action, usize)>) {
    let (mut orders, mut made): (Vec<(Vec<usize>, usize)>, Vec<(Action, usize)>) = (vec![], vec![]);
    for (order, act) in steps {
        let first: Vec<usize> = order.iter().take(1).copied().collect();
        match orders.iter_mut().find(|x| x.0 == first) {
            Some(x) => x.1 += 1,
            None => orders.push((first, 1)),
        }
        if order.is_empty() {
            match made.iter_mut().find(|m| same_step(&(vec![], m.0.clone()), &(vec![], act.clone()))) {
                Some(m) => m.1 += 1,
                None => made.push((act.clone(), 1)),
            }
        }
    }
    (orders, made)
}

/// Whether the vote (`decide_unseen`) over `steps` is settled on using no consumable and on
/// one action, with `left` draws still to go: no draw left could make a consumable win
/// (`vote_settled` on the orders) or another action (on the actions of those using none)
fn unseen_settled(steps: &[(Vec<usize>, Action)], left: usize) -> bool {
    let (orders, made) = vote_tallies(steps);
    vote_settled(&tally_counts(&orders), left).is_some_and(|l| orders[l].0.is_empty()) && vote_settled(&tally_counts(&made), left).is_some()
}

/// Whether `decide_unseen` stops drawing, with `left` draws still to go: the draws so far all
/// made the same decision, using no consumable, and the rest can't outvote it
/// (`unseen_settled`). A budget: a later draw that dissents would have been valued over them.
fn unseen_done(steps: &[(Vec<usize>, Action)], left: usize) -> bool {
    unseen_settled(steps, left) && steps.iter().all(|x| same_step(x, &steps[0]))
}

/// How many draws made each decision of a tally (`vote_tallies`)
fn tally_counts<T>(tally: &[(T, usize)]) -> Vec<usize> {
    tally.iter().map(|x| x.1).collect()
}

/// The vote's ranking (`decide_unseen`), its one tie rule: of `counts` (how many draws made each
/// decision, in the order first made), the decisions most made first, the earliest first on a
/// tie (they're drawn at random, not ordered by the cards)
fn vote_ranking(counts: &[usize]) -> Vec<usize> {
    let mut rank: Vec<usize> = (0..counts.len()).collect();
    rank.sort_by_key(|&i| std::cmp::Reverse(counts[i]));
    rank
}

/// The decision a vote has settled on (`decide_unseen`): of `counts` (how many draws made each
/// decision, in the order first made), the first by `vote_ranking`, once no decision (one made,
/// or a new one, which would come later) could pass it with `left` draws still to go
fn vote_settled(counts: &[usize], left: usize) -> Option<usize> {
    let lead = *vote_ranking(counts).first()?;
    let c = counts[lead];
    let safe = |i: usize| i == lead || if i < lead { counts[i] + left < c } else { counts[i] + left <= c };
    (left <= c && (0..counts.len()).all(safe)).then_some(lead)
}

/// Whether two decisions (`decide_step`) are the same: the same consumables used in the same
/// order, and the same cards played or thrown (in any order: the order played is the first
/// draw's)
fn same_step(x: &(Vec<usize>, Action), y: &(Vec<usize>, Action)) -> bool {
    let cards = |a: &Action| match a {
        Action::Play(v, _) => (0, { let mut v = v.clone(); v.sort_unstable(); v }),
        Action::Discard(v) => (1, { let mut v = v.clone(); v.sort_unstable(); v }),
    };
    x.0 == y.0 && cards(&x.1) == cards(&y.1)
}

/// `sim_round_uses`, with the board the round ends on (its jokers as its hands and discards
/// left them: `grown_over`; the cards played this ante, `Board::played_ante`, with the ones its
/// hands played, none after a boss beaten: `Board::won_round`). While a sale would lift the
/// blind (Verdant Leaf) the player weighs one before each hand (`leaf_sale`).
fn play_round(board: &Board, start: &RoundStart, rng: &mut Rng, uses: &[Use]) -> (RoundResult, Board) {
    let mut used = vec![false; uses.len()];
    let (mut b, mut start) = board.at_blind_set_drawn(start, set_seed(rng));
    // the face-down cards drawn anew for this round
    if let Some(s) = deal_hidden(&b, &start, rng) {
        start = s;
    }
    // the consumables held, on the cards this round dealt
    let uses = &uses_on(&b, uses, &start.hand);
    // the cards played this ante, set on the board as the round ends: not cloned with every board
    // its choices try
    let mut played_ante = std::mem::take(&mut b.played_ante);
    let mut deck = std::mem::take(&mut start.deck);
    shuffle(&mut deck, rng);
    let size = start.hand_size.max(1) as usize;
    let mut hand = std::mem::take(&mut start.hand);
    // the cards the boss turns down dealt face down (`Board::stays_flipped`); a hand short
    // since its last draw plays as it is, the draw after its next play or discard fills it
    // (`RoundStart::first_fill`)
    let mut flips = flip_stream(rng);
    let fill = start.first_fill(&hand, size);
    draw(&mut b, &mut hand, &mut deck, fill, &mut flips);
    let (mut hands, mut discards) = (start.hands, start.discards);
    let mut total = start.scored;
    let mut best_hand: f64 = 0.0;
    let mut plays = Vec::new();
    let mut money = 0.0;
    let mut grown = Grown::default();
    // Cards worth drawing (`RoundGoals::seen`), per consumable: the best gain drawn so far. A
    // consumable used during the round (on its own fixed targets) takes its gain with it.
    let mut seen_by: Vec<(String, f64)> = b.goals.as_ref().map_or(vec![], |g| g.carried.clone());
    note_seen(&b, &hand, &mut seen_by);
    while hands > 0 && !hand.is_empty() {
        b.hands_left = hands;
        b.discards_left = discards;
        b.deck_remaining = deck.len() as i64;
        if b.sale_lifts_blind() {
            let was = jokers_now(&b);
            money += leaf_sale(&mut b, &mut hand, &mut deck, size, hands);
            grown += grown_since(&b, &was);
        }
        // the player's decision, on the cards it sees (`decide_unseen` while one in hand is face down)
        let act = if hand.iter().any(|c| c.face_down) {
            decide_unseen(&mut b, &mut hand, &deck, &mut used, &mut seen_by, uses, hands, discards, total, start.target, size, rng)
        } else {
            decide_step(&mut b, &mut hand, &deck, &mut used, &mut seen_by, uses, hands, discards, total, start.target, size, rng).1
        };
        match act {
            Action::Play(mut idx, dig) => {
                if idx.is_empty() {
                    break;
                }
                idx.truncate(5);
                let played: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
                let mut held: Vec<Card> = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
                played_ante.extend(played.iter().filter(|c| !c.played_this_ante).map(|c| Card { played_this_ante: true, debuff: false, ..*c }));
                let was = jokers_now(&b);
                money += boss_takes_held(&mut b, &mut held, rng);
                let o = score::score_pressed(&b, &played, &held, rng, false);
                total += o.score;
                money += o.dollars;
                best_hand = best_hand.max(o.score);
                plays.push((o.hand, o.score, dig));
                let _ = b.after_hand(&o);
                grown += grown_since(&b, &was);
                hand = held;
                hands -= 1;
                if total >= start.target {
                    money += won_money(&b, &hand);
                    let seen = seen_by.iter().map(|e| e.1).fold(0.0, f64::max);
                    let planets = seal_planets(&b, &hand);
                    b.played_ante = played_ante;
                    // the round ends here: no draw owed (`BlindRules::prepped`)
                    b.blind.prepped = false;
                    b.won_round();
                    return (RoundResult { total, won: true, saved: false, best_hand, plays, money, hands_left: hands, discards_left: discards, planets, seen, grown }, b);
                }
                draw(&mut b, &mut hand, &mut deck, size, &mut flips);
                note_seen(&b, &hand, &mut seen_by);
            }
            Action::Discard(idx) => {
                let was = jokers_now(&b);
                money += b.discard(&picked(&hand, &idx));
                grown += grown_since(&b, &was);
                hand = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
                draw(&mut b, &mut hand, &mut deck, size, &mut flips);
                discards -= 1;
                note_seen(&b, &hand, &mut seen_by);
            }
        }
    }
    let won = total >= start.target;
    let seen = seen_by.iter().map(|e| e.1).fold(0.0, f64::max);
    b.played_ante = played_ante;
    // a lost round a joker saves (`Board::saves_lost_round`: Mr. Bones) ends for the board as a
    // won one does (`won_round`), and the joker that saved it is gone (state_events.lua
    // `end_round`, card.lua `start_dissolve`; a Blueprint or Brainstorm copying him spends the
    // same save). Not paid here: what the game's cash-out pays a saved round besides (Gold cards
    // held, the jokers' round-end money, Blue Seal planets, discards left)
    let saver = if won { None } else { b.saves_lost_round(total, start.target) };
    let saved = saver.is_some();
    if won || saved {
        b.won_round();
    }
    if let Some(i) = saver {
        // (a Negative one takes its slot with it: card.lua `Card:remove_from_deck`)
        if b.jokers[i].edition == Some(crate::model::Edition::Negative) {
            b.joker_slots -= 1;
        }
        b.jokers.remove(i);
    }
    (RoundResult { total, won, saved, best_hand, plays, money, hands_left: 0, discards_left: 0, planets: 0.0, seen, grown }, b)
}

/// What rounds played one after another leave of a joker a round changes (`grown_over`): its
/// key, its numbers (`Joker::lasting`) at the start and, at each checkpoint, their mean over the
/// runs (a used-up joker counted as none: Mult 0, ×1, Chips 0) and the share of runs it's used
/// up in by then.
#[derive(Debug, Clone)]
pub struct JokerGrowth {
    pub key: String,
    pub start: [f64; 3],
    pub at: Vec<[f64; 3]>,
    pub gone: Vec<f64>,
}

impl JokerGrowth {
    /// At checkpoint `k`, grown from `from` (its numbers, if not the measured start) by what the
    /// runs added to the start; `None`: used up in most runs. What a reset takes back (Ride the
    /// Bus, Obelisk) is in what was added, not in a difference at the start, which is kept whole.
    pub fn numbers(&self, k: usize, from: [f64; 3]) -> Option<[f64; 3]> {
        (self.gone[k] < 0.5).then(|| std::array::from_fn(|f| from[f] + self.at[k][f] - self.start[f]))
    }
}

/// Each joker on `b` whose growth rounds measure (`engine::grows_in_rounds`; `None` for the
/// others) as rounds played one after another leave it: `chains` runs, each round dealt anew
/// from `starts` in turn (an ante's blinds, fresh ones: each reset as it starts) and played by the simulated player, the board
/// carried from one to the next (the round targets drawn anew from the round's deck,
/// `Board::draw_round_targets`: Castle's suit; the round's reset as the next one starts,
/// `Board::at_blind_set`), read after
/// `checkpoints` rounds (ascending, from 1). The same seed gives the same runs. A lost round
/// doesn't end a run here; a save is spent in it as in the game (`play_round`: the Mr. Bones
/// that saved is gone for the rounds after, which the jokers that read the joker count see).
pub fn grown_over(b: &Board, starts: &[RoundStart], checkpoints: &[usize], chains: usize, seed: u64) -> Vec<Option<JokerGrowth>> {
    let counted: Vec<bool> = b.jokers.iter().map(|j| crate::engine::grows_in_rounds(&j.key)).collect();
    if !counted.contains(&true) || checkpoints.is_empty() || starts.is_empty() {
        return vec![None; b.jokers.len()];
    }
    let last = *checkpoints.last().expect("checkpoints");
    // each run's jokers at each checkpoint (`None`: used up), by place on the board
    let chain = |c: usize| -> Vec<Vec<Option<[f64; 3]>>> {
        let mut rng = Rng::new(seed.wrapping_add(c as u64 * 7919) ^ 0x6772_6f77);
        let mut board = b.clone();
        let mut out = vec![];
        for r in 1..=last {
            let start = &starts[(r - 1) % starts.len()];
            board = play_round(&board, start, &mut rng, &[]).1;
            board.draw_round_targets(&start.deck, &mut rng);
            if checkpoints.contains(&r) {
                out.push(b.jokers.iter().map(|j| board.jokers.iter().find(|n| n.id == j.id).map(Joker::lasting)).collect());
            }
        }
        out
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(chains).max(1);
    let per = chains.div_ceil(threads);
    let p = crate::progress::current();
    let runs: Vec<Vec<Vec<Option<[f64; 3]>>>> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                let (p, chain) = (p.clone(), &chain);
                sc.spawn(move || {
                    let _p = crate::progress::enter(p);
                    (t * per..((t + 1) * per).min(chains)).map(chain).collect::<Vec<_>>()
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| crate::progress::joined(h.join())).collect()
    });
    let none = |n: Option<[f64; 3]>| n.unwrap_or([0.0, 1.0, 0.0]);
    let n = runs.len().max(1) as f64;
    (0..b.jokers.len())
        .map(|i| {
            counted[i].then(|| {
                let at = (0..checkpoints.len()).map(|k| std::array::from_fn(|f| runs.iter().map(|c| none(c[k][i])[f]).sum::<f64>() / n)).collect();
                let gone = (0..checkpoints.len()).map(|k| runs.iter().filter(|c| c[k][i].is_none()).count() as f64 / n).collect();
                JokerGrowth { key: b.jokers[i].key.clone(), start: b.jokers[i].lasting(), at, gone }
            })
        })
        .collect()
}

/// Whether each of `sims` runs gets through `rounds` (fresh ones) one after another, each
/// from the board the one before ends with (its jokers' state as its hands and discards left
/// them; then the round targets drawn anew and the round's reset as it starts, as the game does
/// and `grown_over` carries it: `Board::at_blind_set`; the first from `b`), a
/// loss ending the run (a save not counted, as `round_odds`' chance on score: `chained_through`
/// counts it). Each round of each run is
/// dealt from its own stream (run, round), so the same run of two boards is dealt the same in
/// every round however many rolls an earlier round took. Not modelled between rounds: the
/// game's other round-end steps (card.lua `end_of_round`: Popcorn, Turtle Bean, Gros Michel, a
/// perishable's rounds, rent, the round's money) and what a round does to your deck (Vampire's
/// stripped cards are whole again; the Stone cards Marble Joker adds as a blind is set stay:
/// `Board::added_cards`).
pub fn chained_runs(b: &Board, rounds: &[RoundStart], sims: usize, seed: u64) -> Vec<bool> {
    chained(b, rounds, sims, seed, false).into_iter().map(|(through, _)| through == rounds.len()).collect()
}

/// `chained_runs` with saves: for each run, the rounds got through (a lost round a joker saves,
/// Mr. Bones, played on from the board it leaves, without the joker that saved it:
/// `play_round`) and which of them were saved (bit `k` for round `k`), on the same draws
pub fn chained_through(b: &Board, rounds: &[RoundStart], sims: usize, seed: u64) -> Vec<(usize, u64)> {
    chained(b, rounds, sims, seed, true)
}

/// The chained runs: a lost round ends a run unless `saves` and a joker saves it. A round in
/// progress keeps its round targets; a fresh one draws them anew.
fn chained(b: &Board, rounds: &[RoundStart], sims: usize, seed: u64, saves: bool) -> Vec<(usize, u64)> {
    let run_one = |i: usize| {
        let mut board = b.clone();
        let mut saved = 0u64;
        for (k, start) in rounds.iter().enumerate() {
            let mut rng = Rng::new(seed.wrapping_add(i as u64 * 7919).wrapping_add(k as u64 * 0x9e37_79b9) ^ 0x6168_6561);
            if start.fresh {
                board.draw_round_targets(&start.deck, &mut rng);
            }
            let (r, end) = play_round(&board, start, &mut rng, &[]);
            if !r.won && !(saves && r.saved) {
                return (k, saved);
            }
            if !r.won {
                saved |= 1 << k.min(63);
            }
            board = end;
        }
        (rounds.len(), saved)
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(sims / 8).max(1);
    if threads <= 1 {
        return (0..sims).map(run_one).collect();
    }
    let chunk = sims.div_ceil(threads);
    let p = crate::progress::current();
    std::thread::scope(|sc| {
        let hs: Vec<_> = (0..threads)
            .map(|t| {
                let (p, run_one) = (p.clone(), &run_one);
                sc.spawn(move || {
                    let _p = crate::progress::enter(p);
                    (t * chunk..((t + 1) * chunk).min(sims)).map(run_one).collect::<Vec<_>>()
                })
            })
            .collect();
        hs.into_iter().flat_map(|h| crate::progress::joined(h.join())).collect()
    })
}

/// Mean and quantiles of a sample.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Stats {
    pub mean: f64,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
    pub n: usize,
    /// Mean money earned while playing, per round (`RoundResult::money`; round simulations only).
    #[serde(skip)]
    pub money: f64,
    /// Mean hands left at the end, per round (0 for a round lost; round simulations only)
    #[serde(skip)]
    pub hands_left: f64,
    /// Chance of getting through with Mr. Bones' save counted (round simulations only).
    #[serde(skip)]
    pub p_saved: f64,
    /// Each simulated round, in order (`round_odds` only: round i draws the same cards on every
    /// board, so two boards' rounds pair up): what the shop ranking tells its options apart on
    /// (`advise` `Noise`)
    #[serde(skip)]
    pub rounds: Option<std::sync::Arc<Vec<Sampled>>>,
}

/// One simulated round of `round_odds`, as `Stats::rounds` keeps it
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Sampled {
    /// Beaten on score (`RoundResult::won`)
    pub won: bool,
    pub total: f64,
}

impl Stats {
    pub fn of(mut v: Vec<f64>) -> Stats {
        if v.is_empty() {
            return Stats::default();
        }
        v.sort_by(f64::total_cmp);
        let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        Stats { mean: v.iter().sum::<f64>() / v.len() as f64, p10: q(0.1), p50: q(0.5), p90: q(0.9), n: v.len(), money: 0.0, hands_left: 0.0, p_saved: 0.0, rounds: None }
    }
}

/// Best-hand score on fresh deals from `deck` (the "how strong is this board" number).
pub fn typical_hands(b: &Board, deck: &[Card], hand_size: usize, samples: usize, seed: u64) -> Vec<f64> {
    typical_hands_detail(b, deck, hand_size, samples, seed).into_iter().map(|(s, _)| s).collect()
}

/// Like `typical_hands`, with the hand type each deal's best play was.
pub fn typical_hands_detail(b: &Board, deck: &[Card], hand_size: usize, samples: usize, seed: u64) -> Vec<(f64, HandType)> {
    // Separate streams: deals must be identical across the boards being compared, and a
    // joker that adds a random roll (Lucky card, Bloodstone…) must not shift later deals.
    let mut rng = Rng::new(seed);
    let mut rolls = Rng::new(seed ^ 0xA5A5_5A5A_DEAD_BEEF);
    let mut d = deck.to_vec();
    let mut b = b.clone();
    b.deck_remaining = (deck.len().saturating_sub(hand_size)) as i64;
    (0..samples)
        .filter_map(|_| {
            shuffle(&mut d, &mut rng);
            let hand = &d[..hand_size.min(d.len())];
            best_play(&b, hand).map(|p| {
                let played: Vec<Card> = p.cards.iter().map(|&i| hand[i]).collect();
                let held: Vec<Card> = (0..hand.len()).filter(|i| !p.cards.contains(i)).map(|i| hand[i]).collect();
                (score::score(&b, &played, &held, &mut rolls, false).score, p.hand)
            })
        })
        .collect()
}

/// Rounds `range` one by one, each drawing the same cards as round i of `round_odds`.
pub fn round_results(b: &Board, start: &RoundStart, range: std::ops::Range<usize>, seed: u64) -> Vec<RoundResult> {
    range.map(|i| sim_round(b, start, &mut Rng::new(seed.wrapping_add(i as u64 * 7919)))).collect()
}

/// Chance of beating a round, from `sims` simulations (same seed → same deals across boards).
/// Simulations run in parallel; each has its own seed, so the result doesn't depend on
/// how they're split across threads.
pub fn round_odds(b: &Board, start: &RoundStart, sims: usize, seed: u64) -> (f64, Stats) {
    let run_one = |i: usize| {
        let mut rng = Rng::new(seed.wrapping_add(i as u64 * 7919));
        sim_round(b, start, &mut rng)
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(sims / 8).max(1);
    let results: Vec<RoundResult> = if threads <= 1 {
        (0..sims).map(run_one).collect()
    } else {
        let chunk = sims.div_ceil(threads);
        // the threads work for this thread's analysis (`progress`): a stop reaches them
        let p = crate::progress::current();
        std::thread::scope(|sc| {
            let hs: Vec<_> = (0..threads)
                .map(|t| {
                    let p = p.clone();
                    sc.spawn(move || {
                        let _p = crate::progress::enter(p);
                        (t * chunk..((t + 1) * chunk).min(sims)).map(run_one).collect::<Vec<_>>()
                    })
                })
                .collect();
            hs.into_iter().flat_map(|h| crate::progress::joined(h.join())).collect()
        })
    };
    // Win chance on score alone: Mr. Bones' one-off save is reported separately (p_saved),
    // so it doesn't make a weak board look strong.
    let wins = results.iter().filter(|r| r.won).count();
    let saved = results.iter().filter(|r| r.saved).count();
    let money = results.iter().map(|r| r.money).sum::<f64>() / results.len().max(1) as f64;
    let hands_left = results.iter().map(|r| if r.won { r.hands_left as f64 } else { 0.0 }).sum::<f64>() / results.len().max(1) as f64;
    let rounds: Vec<Sampled> = results.iter().map(|r| Sampled { won: r.won, total: r.total }).collect();
    let mut st = Stats::of(results.into_iter().map(|r| r.total).collect());
    st.money = money;
    st.hands_left = hands_left;
    st.p_saved = (wins + saved) as f64 / sims.max(1) as f64;
    st.rounds = Some(std::sync::Arc::new(rounds));
    (wins as f64 / sims.max(1) as f64, st)
}

/// Which hands score the points in simulated rounds: (hand, share of points, share of
/// hands played, mean score). Junk hands played to dig are left out. The rounds on every core,
/// added up in their order.
pub fn round_hand_mix(b: &Board, start: &RoundStart, sims: usize, seed: u64) -> Vec<(HandType, f64, f64, f64)> {
    let plays = crate::progress::par_map(&(0..sims).collect::<Vec<_>>(), |&i| {
        let mut rng = Rng::new(seed.wrapping_add(i as u64 * 7919));
        sim_round(b, start, &mut rng).plays
    });
    let mut pts = [0.0f64; 12];
    let mut cnt = [0usize; 12];
    for (h, s, dig) in plays.into_iter().flatten() {
        if !dig {
            pts[h as usize] += s;
            cnt[h as usize] += 1;
        }
    }
    let (tp, tc) = (pts.iter().sum::<f64>().max(1.0), cnt.iter().sum::<usize>().max(1) as f64);
    let mut out: Vec<_> = HandType::ALL
        .iter()
        .filter(|h| cnt[**h as usize] > 0)
        .map(|&h| (h, pts[h as usize] / tp, cnt[h as usize] as f64 / tc, pts[h as usize] / cnt[h as usize] as f64))
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

/// Rolls helper so callers don't need the trait in scope.
pub fn rng(seed: u64) -> Rng {
    let mut r = Rng::new(seed);
    let _ = r.unit();
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{sample_board, standard_deck};

    fn labels(cards: &[Card]) -> String {
        cards.iter().map(Card::label).collect::<Vec<_>>().join(" ")
    }

    /// A random hand of `n` cards (ranks and suits may repeat, as a changed deck's do), some
    /// Stone, Wild or Steel
    fn random_hand(rng: &mut Rng, n: usize) -> Vec<Card> {
        let ranks = ["2", "3", "4", "5", "6", "7", "8", "9", "T", "J", "Q", "K", "A"];
        let text: Vec<String> = (0..n)
            .map(|_| {
                let mods = ["", "", "", "", "", "", ":stone", ":wild", ":steel"];
                format!("{}{}{}", ranks[rng.below(13)], ["S", "H", "D", "C"][rng.below(4)], mods[rng.below(mods.len())])
            })
            .collect();
        Card::parse_list(&text.join(" ")).unwrap()
    }

    #[test]
    fn a_big_face_down_hands_screen_passes_the_blind_plays_worth_what_they_may_make() {
        // 16 cards, all face down, suited: a run of places in the sorts is worth its occasional
        // Flush or Straight, which a few draws rank only by luck. The plays passed hold one within
        // 3% of the best 5-card play (each valued on 512 draws nothing else uses); ranked on only
        // the structure's 4 draws (this seed) they didn't (gap 25 (c))
        let mut hand = Card::parse_list("AS KS QS JS TS 9H 8H 7H 6H 5D 4D 3D 2D AC KC QC").unwrap();
        hand.iter_mut().for_each(|c| c.face_down = true);
        let b = sample_board(&[]);
        let slots: Vec<usize> = (0..hand.len()).collect();
        let mut h = Hidden::new(&hand, &slots, slots.clone(), false);
        h.stand = hand.clone();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|x| x.same_kind(c))).collect();
        let s = RoundStart { hand: hand.clone(), deck, hand_size: 16, hands: 7, discards: 0, target: 18000.0, hidden: Some(std::sync::Arc::new(h)), ..Default::default() };
        let judge = hidden_hands(&b, &s, 55_000_009, 512);
        let value = |idx: &[usize]| {
            judge.iter().map(|h| {
                let played: Vec<Card> = idx.iter().map(|&i| h[i]).collect();
                let held: Vec<Card> = (0..h.len()).filter(|i| !idx.contains(i)).map(|i| h[i]).collect();
                score::score(&b, &played, &held, &mut Unlucky, false).score
            }).sum::<f64>() / judge.len() as f64
        };
        let best = (0u32..1 << 16).filter(|m| m.count_ones() == 5).map(|m| value(&(0..16).filter(|i| m & (1 << i) != 0).collect::<Vec<_>>())).fold(0.0, f64::max);
        let passed = |draws: &[Vec<Card>]| big_hand_moves(&b, draws, &s.deck, 0).iter().filter_map(|m| if let Move::Play(v) = m { Some(value(v)) } else { None }).fold(0.0, f64::max);
        let draws = hidden_hands_par(&b, &s, 77 + 1_000_000, BIG_HAND_RANK_DRAWS);
        assert_eq!(draws, hidden_hands(&b, &s, 77 + 1_000_000, BIG_HAND_RANK_DRAWS), "drawn on every core, the same hands");
        let (deep, shallow) = (passed(&draws), passed(&draws[..BIG_HAND_DRAWS]));
        assert!(deep >= 0.97 * best, "{deep} of {best}");
        assert!(shallow < 0.9 * best, "{shallow} of {best}: the measure no longer tells the ranking's depth");
    }

    #[test]
    fn a_vote_stops_only_once_no_draw_left_can_change_its_winner() {
        // every sequence of `UNSEEN_DRAWS` draws' decisions (as indices in the order first made): wherever
        // `vote_settled` stops, the whole vote's winner (the most made, the earliest on a tie) is
        // the one it named
        fn winner(seq: &[usize]) -> usize {
            let n = seq.iter().max().unwrap() + 1;
            let counts: Vec<usize> = (0..n).map(|i| seq.iter().filter(|&&x| x == i).count()).collect();
            vote_ranking(&counts)[0]
        }
        let u = UNSEEN_DRAWS;
        let mut stopped = 0;
        for code in 0..u.pow(u as u32) {
            // a decision index can be at most one past those made before it
            let seq: Vec<usize> = (0..u).map(|k| code / u.pow(k as u32) % u).collect();
            if (0..u).any(|k| seq[k] > seq[..k].iter().max().map_or(0, |m| m + 1)) {
                continue;
            }
            for k in 0..u {
                let n = seq[..=k].iter().max().unwrap() + 1;
                let counts: Vec<usize> = (0..n).map(|i| seq[..=k].iter().filter(|&&x| x == i).count()).collect();
                if let Some(l) = vote_settled(&counts, u - 1 - k) {
                    assert_eq!(l, winner(&seq), "{seq:?} stopped after {}", k + 1);
                    stopped += usize::from(k + 1 < u);
                    break;
                }
            }
        }
        // it does stop early: two draws alike settle a vote of four
        assert_eq!(vote_settled(&[2], 2), Some(0));
        assert_eq!(vote_settled(&[1, 1], 2), None);
        assert!(stopped > 0);
    }

    #[test]
    fn the_two_step_vote_stops_only_once_no_draw_left_can_change_what_it_decides() {
        // every sequence of `UNSEEN_DRAWS` draws' decisions from: no consumable and one play, no
        // consumable and another, a consumable first, another consumable first (each then a play):
        // wherever `unseen_settled` stops, the whole vote (`decide_unseen`) uses no consumable
        // and makes the action the draws so far make; it stops only on those
        let play = |i: usize| Action::Play(vec![i], false);
        let kinds: [(Vec<usize>, Action); 4] = [(vec![], play(0)), (vec![], play(1)), (vec![0], play(0)), (vec![1, 0], play(2))];
        // the vote's outcome: the consumable used first, else the action of those using none
        let outcome = |steps: &[(Vec<usize>, Action)]| -> (Vec<usize>, Option<Action>) {
            let (orders, made) = vote_tallies(steps);
            let first = orders[vote_ranking(&tally_counts(&orders))[0]].0.clone();
            let act = first.is_empty().then(|| made[vote_ranking(&tally_counts(&made))[0]].0.clone());
            (first, act)
        };
        let u = UNSEEN_DRAWS;
        let (mut stopped, mut ran) = (0, 0);
        for code in 0..kinds.len().pow(u as u32) {
            let seq: Vec<(Vec<usize>, Action)> = (0..u).map(|k| kinds[code / kinds.len().pow(k as u32) % kinds.len()].clone()).collect();
            let whole = outcome(&seq);
            for k in 0..u {
                if unseen_settled(&seq[..=k], u - 1 - k) {
                    let (first, act) = outcome(&seq[..=k]);
                    assert!(first.is_empty());
                    assert_eq!(format!("{whole:?}"), format!("{:?}", (first, act)), "{seq:?} stopped after {}", k + 1);
                    stopped += usize::from(k + 1 < u);
                    break;
                }
                ran += usize::from(k + 1 == u);
            }
        }
        assert!(stopped > 0 && ran > 0, "stopped early {stopped}, ran every draw {ran}");
    }

    #[test]
    fn a_draws_valuation_on_its_decisions_search_is_the_one_searched_afresh() {
        // `decide_unseen` values each draw on what its decision's search of that hand found
        // (`HandParts::with`): the same sums as the policy's estimates searched afresh, on hands
        // of 8 cards and of 14 (past `PARTS_MAX`: the shared table up to `SHARE_MAX`), on a
        // board where held cards score and one where they don't
        let mut checked = [0; 2];
        for (m, n) in [8, 14].into_iter().enumerate() {
            for (jokers, need) in [(&["j_droll", "j_tribe"][..], 2000.0), (&["j_blackboard", "j_baron"], 1500.0)] {
                let b = sample_board(jokers);
                for seed in 0..6u64 {
                    let mut deck = standard_deck();
                    shuffle(&mut deck, &mut Rng::new(seed + 100));
                    let mut hand: Vec<Card> = deck[..n].to_vec();
                    hand.iter_mut().for_each(|c| c.face_down = true);
                    let deck = deck[n..].to_vec();
                    UNSEEN_VALUED.with(|x| x.borrow_mut().clear());
                    decide_unseen(&mut b.clone(), &mut hand.clone(), &deck, &mut [], &mut vec![], &[], 4, 3, 0.0, need, n, &Rng::new(seed));
                    let Some(r) = UNSEEN_VALUED.with(|x| x.borrow().last().cloned()) else { continue };
                    let mut sum = vec![0.0; r.acts.len()];
                    for (h, d) in &r.draws {
                        for (s, v) in sum.iter_mut().zip(policy_estimates(&b, h, d, 4, 3, need, n, None, true, &r.acts)) {
                            *s += v;
                        }
                    }
                    assert_eq!(sum, r.sum, "{n} cards, {jokers:?}, seed {seed}");
                    checked[m] += 1;
                }
            }
        }
        assert!(checked.iter().all(|&k| k >= 3), "valued on 8 and 14 cards: {checked:?}");
    }

    #[test]
    fn the_draws_stop_only_once_they_agree_as_a_dissent_would_be_valued() {
        // a vote of four settled on two draws alike stops; one settled with a dissent among the
        // draws so far goes on, as that dissent is valued over every draw (`decide_unseen`)
        let play = |i: usize| (vec![], Action::Play(vec![i], false));
        assert!(unseen_done(&[play(0), play(0)], 2));
        assert!(unseen_settled(&[play(0), play(0), play(1)], 1));
        assert!(!unseen_done(&[play(0), play(0), play(1)], 1));
        assert!(!unseen_done(&[play(0)], 3));
        // a consumable first never stops it
        assert!(!unseen_done(&[(vec![0], Action::Play(vec![0], false)), (vec![0], Action::Play(vec![0], false))], 2));
    }

    #[test]
    fn a_face_down_hand_plays_the_decision_its_draws_value_most_not_the_first_draws() {
        // The House's first hand, every card face down: each draw imagines other cards, so the
        // draws mostly decide apart, and a vote played the first draw's pick. The decision is the
        // one the policy's own estimates value most over the draws (`policy_estimates`), the
        // vote's order only on a tie (gap 25 (a))
        let mut hand = Card::parse_list("AS KH QD 9C 7S 5H 4D 2C").unwrap();
        hand.iter_mut().for_each(|c| c.face_down = true);
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
        let cards = |a: &Action| match a {
            Action::Play(v, j) => (u8::from(*j), { let mut v = v.clone(); v.sort_unstable(); v }),
            Action::Discard(v) => (2, { let mut v = v.clone(); v.sort_unstable(); v }),
        };
        let (mut valued, mut moved) = (0, 0);
        for (jokers, need) in [(&["j_droll", "j_tribe"][..], 2000.0), (&["j_jolly", "j_duo"], 1500.0), (&["j_joker"], 900.0)] {
            let b = sample_board(jokers);
            for seed in 0..6 {
                UNSEEN_VALUED.with(|x| x.borrow_mut().clear());
                let act = decide_unseen(&mut b.clone(), &mut hand.clone(), &deck, &mut [], &mut vec![], &[], 4, 3, 0.0, need, 8, &Rng::new(seed));
                let Some(r) = UNSEEN_VALUED.with(|x| x.borrow().last().cloned()) else { continue };
                valued += 1;
                moved += usize::from(r.vote[0] != r.pick);
                // the action played is the one valued most
                assert_eq!(cards(&act), cards(&r.acts[r.pick]), "seed {seed}");
                // summed over every draw, each draw's hand and pile valued as the policy would
                let mut sum = vec![0.0; r.acts.len()];
                for (h, d) in &r.draws {
                    for (s, v) in sum.iter_mut().zip(policy_estimates(&b, h, d, 4, 3, need, 8, None, true, &r.acts)) {
                        *s += v;
                    }
                }
                assert_eq!(r.draws.len(), UNSEEN_DRAWS);
                assert_eq!(sum, r.sum);
                // the most, the vote's earliest on a tie
                let best = sum.iter().copied().fold(f64::MIN, f64::max);
                assert_eq!(r.pick, *r.vote.iter().find(|&&i| sum[i] == best).unwrap(), "{sum:?}");
            }
        }
        assert!(valued >= 6, "{valued}");
        // and it isn't the vote: some pick differs from the one the vote made
        assert!(moved > 0);
    }

    #[test]
    fn the_policys_estimates_value_its_own_decision_as_it_weighs_it() {
        // On a hand it sees, the policy's estimate of a play is its playing on (`playing_on`:
        // the play now, a later hand each hand after), and of the dig it makes for a plan at least
        // that (a chase is made only when worth more than playing on, `chase`); a dig of a card
        // the plan needs is worth less than the plan's own dig
        let b = sample_board(&["j_droll", "j_tribe"]);
        let hand = Card::parse_list("AH 9H 6H 3H KS QD 7C 2C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
        let (hands, discards, need) = (4, 3, 3000.0);
        let act = decide_cards(&b, &hand, &deck, hands, discards, need, 8, true);
        let Action::Discard(dig) = &act else { panic!("{act:?}: the four hearts aren't chased") };
        assert!((0..4).all(|i| !dig.contains(&i)), "{dig:?}");
        let best = best_play(&b, &hand).unwrap();
        let mut spoilt: Vec<usize> = dig.iter().copied().filter(|&i| i != dig[0]).collect();
        spoilt.push(0);
        let v = policy_estimates(&b, &hand, &deck, hands, discards, need, 8, None, true, &[Action::Play(best.cards.clone(), false), act.clone(), Action::Discard(spoilt)]);
        let on = playing_on(&b, &hand, &best, hands);
        assert!((v[0] - on).abs() <= 1e-6 * on, "{} against playing on {on}", v[0]);
        assert!(v[1] > v[0], "{v:?}");
        assert!(v[2] < v[1], "{v:?}");
    }

    #[test]
    fn the_policys_own_decision_is_valued_at_least_as_its_best_play() {
        // the estimates (`policy_estimates`) and the decision (`decide_cards`) weigh a dig
        // alike: on a hand it sees, what it decides is worth at least playing its best play, on
        // pace and off, on the last hand, and where the score reads the cards held
        let cases: [(&[&str], &str, i64, i64, f64); 8] = [
            (&["j_droll", "j_tribe"], "AH 9H 6H 3H KS QD 7C 2C", 4, 3, 3000.0),
            (&["j_droll", "j_tribe"], "AH 9H 6H KS KD 7C 4C 2D", 4, 3, 100.0),
            (&["j_droll"], "AH 9H 6H 3H KS QD 7C 2C", 1, 3, 900.0),
            (&["j_raised_fist"], "KS KD 9H 7C 5D 3S 2H 2C", 4, 3, 2500.0),
            (&["j_baron"], "KS KD KH 9H 7C 5D 3S 2C", 4, 2, 6000.0),
            (&["j_jolly", "j_duo"], "QS QD 9H 9C 6D 5S 3H 2C", 2, 1, 1500.0),
            (&["j_green_joker", "j_droll"], "AH 9H 6H 3H KS QD 7C 2C", 4, 3, 3000.0),
            (&["j_droll", "j_tribe"], "AH 9H 6H 3H KS QD 7C 2C", 3, 0, 2500.0),
        ];
        for (jokers, cards, hands, discards, need) in cases {
            let b = sample_board(jokers);
            let hand = Card::parse_list(cards).unwrap();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
            let act = decide_cards(&b, &hand, &deck, hands, discards, need, 8, true);
            let best = best_play(&b, &hand).unwrap();
            let acts = [act.clone(), Action::Play(best.cards.clone(), false)];
            let mut v = policy_estimates(&b, &hand, &deck, hands, discards, need, 8, None, true, &acts);
            if v.iter().all(|&x| x <= 0.0) {
                v = policy_estimates(&b, &hand, &deck, hands, discards, need, 8, None, false, &acts);
            }
            assert!(v[0] >= v[1] * (1.0 - 1e-9), "{cards} {jokers:?}: {act:?} {v:?}");
            // a play on its own cards with the rest held, as playing on counts it (Raised Fist,
            // Baron: the cards held score)
            if hands > 1 {
                let on = playing_on(&b, &hand, &best, hands);
                assert!((v[1] - on).abs() <= 1e-6 * on, "{cards} {jokers:?}: {} against playing on {on}", v[1]);
            }
        }
    }

    #[test]
    fn a_hand_of_only_face_down_cards_is_played() {
        // The House's first hand: every card face down. The game allows playing and discarding
        // them, so there are moves: every play and discard, as with any hand (gap 25), and the
        // round goes on past its first decision (it once played nothing, so every simulated
        // round from such a hand was lost).
        let mut hand = Card::parse_list("AS KH QD 9C 7S 5H 4D 2C").unwrap();
        hand.iter_mut().for_each(|c| c.face_down = true);
        // 8 cards: 8 + 28 + 56 + 70 + 56 plays of 1 to 5
        assert_eq!(all_plays(&hand).len(), 218);
        assert_eq!(all_discards(&hand, 1).len(), 218);
        assert!(all_discards(&hand, 0).is_empty());
        let b = sample_board(&["j_joker"]);
        let start = RoundStart { hand: hand.clone(), deck: standard_deck(), hand_size: 8, hands: 4, discards: 0, scored: 0.0, target: 300.0, fresh: false, ..Default::default() };
        let won = outcomes_after(&b, &start, &Move::Play(vec![0]), 0..16, 3, &[]).iter().filter(|o| o.won > 0.0).count();
        assert!(won > 8, "{won}");
        // the round's player too, from the first hand
        let mut rng = Rng::new(9);
        let r = sim_round_uses(&b, &RoundStart { deck: standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect(), ..start.clone() }, &mut rng, &[]);
        assert!(!r.plays.is_empty());
        // one face-up card: a face-down card is played like any other
        hand[0].face_down = false;
        assert_eq!(all_plays(&hand).len(), 218);
    }

    #[test]
    fn the_player_decides_on_what_a_face_down_card_could_be_not_what_it_is() {
        // A face-down 9♠ or 8♠: between A♠ and 2♠ in the suit sort, between the A and the 7s in
        // the rank sort, the other one in the pile. The sorts and the unseen cards are the same,
        // so every decision is, on boards that care about the rank or the suit (gap 25 (a))
        let mut nine = Card::parse_list("AS 2S 9S 7H 7D 3C 4C 6D").unwrap();
        nine[2].face_down = true;
        let mut eight = nine.clone();
        eight[2] = Card { face_down: true, ..Card::parse_list("8S").unwrap()[0] };
        let pile = |hand: &[Card]| -> Vec<Card> { standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect() };
        let known = |hand: &[Card]| {
            let h = Hidden::new(hand, &[2], vec![2], false);
            (h.bounds, h.place)
        };
        assert_eq!(known(&nine), known(&eight));
        let mut differs = 0;
        for jokers in [&["j_joker"][..], &["j_droll", "j_tribe"], &["j_fibonacci", "j_even_steven"]] {
            let b = sample_board(jokers);
            for (hands, discards, need) in [(4, 3, 600.0), (2, 1, 300.0), (1, 0, 150.0), (3, 0, 5000.0)] {
                let decide = |hand: &[Card]| {
                    let (mut b, mut hand, deck) = (b.clone(), hand.to_vec(), pile(hand));
                    (vec![], decide_unseen(&mut b, &mut hand, &deck, &mut [], &mut vec![], &[], hands, discards, 0.0, need, 8, &Rng::new(5)))
                };
                let (x, y) = (decide(&nine), decide(&eight));
                assert!(same_step(&x, &y), "{jokers:?} {hands} {discards}: {:?} against {:?}", x.1, y.1);
                // the decision on the cards as they really are, for the check below
                let read = |hand: &[Card]| {
                    let (mut b, mut hand, deck) = (b.clone(), hand.to_vec(), pile(hand));
                    decide_step(&mut b, &mut hand, &deck, &mut [], &mut vec![], &[], hands, discards, 0.0, need, 8, &mut Rng::new(5))
                };
                differs += !same_step(&read(&nine), &read(&eight)) as usize;
            }
        }
        // reading the card would have told them apart
        assert!(differs > 0);
    }

    #[test]
    fn a_consumable_held_for_a_face_down_place_goes_on_what_each_draw_put_there() {
        // A Strength held for the face-down card's place (`Use::at`), a 9♠ or an 8♠ that the
        // sorts and the unseen cards can't tell apart: each draw the player decides on puts it on
        // the card that draw has there, so whether it's used is the same whatever the card is,
        // and so is the action when it isn't; not only in the draws that put the card it really
        // is there (gap 25 (a)). Once used, the card is seen and the play decided on it.
        let mut nine = Card::parse_list("AS 2S 9S 7H 7D 3C 4C 6D").unwrap();
        nine[2].face_down = true;
        let mut eight = nine.clone();
        eight[2] = Card { face_down: true, ..Card::parse_list("8S").unwrap()[0] };
        let pile = |hand: &[Card]| -> Vec<Card> { standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect() };
        let strength = Use { key: "c_strength".into(), at: vec![(CardEffect::UpRank, vec![2])], ..Default::default() };
        let (mut uses_it, mut seen_after) = (0, 0);
        for jokers in [&["j_joker"][..], &["j_droll", "j_tribe"], &["j_fibonacci", "j_even_steven"], &["j_scholar", "j_walkie_talkie"]] {
            let b = sample_board(jokers);
            for (hands, discards, need) in [(4, 3, 600.0), (2, 1, 300.0), (1, 0, 150.0), (1, 0, 400.0), (3, 0, 5000.0)] {
                let decide = |hand: &[Card]| {
                    let (mut b, mut hand, deck) = (b.clone(), hand.to_vec(), pile(hand));
                    let uses = uses_on(&b, std::slice::from_ref(&strength), &hand);
                    let mut used = vec![false];
                    let act = decide_unseen(&mut b, &mut hand, &deck, &mut used, &mut vec![], &uses, hands, discards, 0.0, need, 8, &Rng::new(5));
                    (used[0], act, hand)
                };
                let (x, y) = (decide(&nine), decide(&eight));
                assert_eq!(x.0, y.0, "{jokers:?} {hands} {discards} {need}: used {} against {}", x.0, y.0);
                if x.0 {
                    uses_it += 1;
                    // the card it changed is seen, a 10♠ or a 9♠
                    assert!(!x.2[2].face_down && x.2[2].rank != y.2[2].rank);
                    seen_after += !same_step(&(vec![], x.1), &(vec![], y.1)) as usize;
                } else {
                    assert!(same_step(&(vec![], x.1.clone()), &(vec![], y.1.clone())), "{jokers:?} {hands} {discards} {need}: {:?} against {:?}", x.1, y.1);
                }
            }
        }
        // the decisions use it, and what it showed moves the play after it (decided again on the
        // card seen, not the draw's play)
        assert!(uses_it > 0 && seen_after > 0, "used {uses_it}, the play after it moved by the card {seen_after}");
    }

    #[test]
    fn a_place_effect_goes_back_to_its_places_while_its_card_is_face_down() {
        // Death held for a face-down left card (place 2) and a face-up right one (place 0): put on
        // the round's cards (`uses_on`), then back to places while the left one is face down, in
        // the order Death reads them (`Use::unplaced`)
        let mut hand = Card::parse_list("AS 2S 9S 7H 7D").unwrap();
        hand[2].face_down = true;
        let b = sample_board(&["j_joker"]);
        let death = Use { key: "c_death".into(), at: vec![(CardEffect::CopyLeftToRight, vec![2, 0])], ..Default::default() };
        let placed = uses_on(&b, std::slice::from_ref(&death), &hand).remove(0);
        assert!(placed.at.is_empty() && placed.placed.len() == 1);
        // the hand moved on: the 2♠ played, the cards one place to the left
        let moved: Vec<Card> = hand.iter().enumerate().filter(|&(i, _)| i != 1).map(|(_, c)| *c).collect();
        let back = placed.unplaced(&moved);
        assert_eq!(back.at, vec![(CardEffect::CopyLeftToRight, vec![1, 0])]);
        assert!(back.placed.is_empty());
        // seen: it stays on the cards (nothing to draw)
        let seen: Vec<Card> = moved.iter().map(|c| Card { face_down: false, ..*c }).collect();
        assert!(placed.unplaced(&seen).at.is_empty());
        // the right card played: Death can't be used (the game needs both selected), in a draw or not
        let gone: Vec<Card> = moved.iter().skip(1).copied().collect();
        let kept = placed.unplaced(&gone);
        assert!(kept.at.is_empty() && kept.placed.len() == 1);
        assert!(kept.apply(&b, &gone).is_none());
        assert!(placed.apply(&b, &moved).is_some());
    }

    #[test]
    fn the_oracles_futures_put_a_held_consumable_on_what_each_deals_there() {
        // The oracle's futures (`outcomes_after` with `RoundStart::hidden`) deal the face-down
        // card anew: a Strength held for its place goes on what each deals there once turned back
        // to places (`Use::unplaced`), so a 9♠ or an 8♠ meets the same rounds; left on the card it
        // really is, the rounds would tell them apart (gap 25 (a))
        let mut nine = Card::parse_list("AS 2S 9S 7H 7D 3C 4C 6D").unwrap();
        nine[2].face_down = true;
        let mut eight = nine.clone();
        eight[2] = Card { face_down: true, ..Card::parse_list("8S").unwrap()[0] };
        let pile = |hand: &[Card]| -> Vec<Card> { standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect() };
        let strength = Use { key: "c_strength".into(), at: vec![(CardEffect::UpRank, vec![2])], ..Default::default() };
        let b = sample_board(&["j_joker"]);
        let rounds = |hand: &[Card], back: bool| -> Vec<(u64, u64)> {
            let mut known = Hidden::new(hand, &[2], vec![2], false);
            known.stand = vec![hand[2]];
            let start = RoundStart { hand: hand.to_vec(), deck: pile(hand), hand_size: 8, hands: 2, discards: 1, target: 300.0, hidden: Some(std::sync::Arc::new(known)), ..Default::default() };
            let mut u = uses_on(&b, std::slice::from_ref(&strength), hand).remove(0);
            if back {
                u = u.unplaced(hand);
            }
            without_oracle(|| outcomes_after(&b, &start, &Move::Discard(vec![7]), 0..32, 9, &[u])).iter().map(|o| (o.won.to_bits(), o.total.to_bits())).collect()
        };
        assert_eq!(rounds(&nine, true), rounds(&eight, true));
        assert_ne!(rounds(&nine, false), rounds(&eight, false));
    }

    #[test]
    fn the_oracle_cant_see_a_face_down_card_either() {
        // The oracle (`set_oracle`) weighs the player's decision once, outside the draws, on
        // futures that draw the face-down card anew: never inside a draw, where it would see
        // that draw's card (gap 25 (a)). So a face-down 9♠ or 8♠ gets the same decision with
        // it on too, and at most one decision of the oracle's per decision with one in hand.
        let mut nine = Card::parse_list("AS 2S 9S 7H 7D 3C 4C 6D").unwrap();
        nine[2].face_down = true;
        let mut eight = nine.clone();
        eight[2] = Card { face_down: true, ..Card::parse_list("8S").unwrap()[0] };
        let pile = |hand: &[Card]| -> Vec<Card> { standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect() };
        let decisions = || ORACLE_DECISIONS.with(|x| x.get());
        set_oracle(8);
        let mut weighed = 0;
        for jokers in [&["j_joker"][..], &["j_droll", "j_tribe"]] {
            let b = sample_board(jokers);
            for (hands, discards, need) in [(4, 3, 600.0), (2, 1, 300.0)] {
                let mut decide = |hand: &[Card]| {
                    let (mut b, mut hand, deck) = (b.clone(), hand.to_vec(), pile(hand));
                    let before = decisions();
                    let act = decide_unseen(&mut b, &mut hand, &deck, &mut [], &mut vec![], &[], hands, discards, 0.0, need, 8, &Rng::new(5));
                    let n = decisions() - before;
                    assert!(n <= 1, "{jokers:?} {hands} {discards}: the oracle decided {n} times");
                    weighed += n;
                    (vec![], act)
                };
                let (x, y) = (decide(&nine), decide(&eight));
                assert!(same_step(&x, &y), "{jokers:?} {hands} {discards}: {:?} against {:?}", x.1, y.1);
            }
            // a win on the table whatever the card is (with the round's goals: without them the
            // policy decides even a win, `win_keeping_seals`): the oracle stays out, as on a
            // hand you see
            let (mut b, mut hand, deck) = (b.clone(), nine.clone(), pile(&nine));
            b.goals = Some(RoundGoals { no_lookahead: true, ..Default::default() });
            let before = decisions();
            decide_unseen(&mut b, &mut hand, &deck, &mut [], &mut vec![], &[], 4, 3, 0.0, 1.0, 8, &Rng::new(5));
            assert_eq!(decisions(), before, "{jokers:?}: the oracle weighed a sure win");
        }
        set_oracle(0);
        assert!(weighed > 0, "the oracle weighed none of the decisions");
    }

    #[test]
    fn a_consumable_on_a_face_down_card_changes_what_each_round_draws_there() {
        // the effect itself, done to whatever the cards are in each simulated round
        let c = |x: &str| Card::parse_list(x).unwrap()[0];
        let down = |x: &str| Card { face_down: true, ..c(x) };
        // Strength: one rank up from what it is (an Ace to a 2, as the game does), and seen (the
        // game flips it up to change it); a card it didn't go on stays face down
        assert_eq!(effect_on(&Board::empty(), &[down("AC"), down("9H")], CardEffect::UpRank, &[0]), vec![c("2C"), down("9H")]);
        // Death, the face-down card copied: both are seen
        assert_eq!(effect_on(&Board::empty(), &[c("4D"), down("KC")], CardEffect::CopyLeftToRight, &[0, 1]), vec![c("KC"), c("KC")]);
        // used first in a round: each round's draw changed there (The World on the face-down card)
        let hand = vec![c("KS"), down("QH")];
        let mut h = Hidden::new(&hand, &[1], vec![1], false);
        h.stand = vec![hand[1]];
        let deck: Vec<Card> = standard_deck().into_iter().filter(|x| !hand.iter().any(|y| y.same_kind(x))).collect();
        let start = RoundStart { hand, deck, hidden: Some(std::sync::Arc::new(h)), changes: vec![(CardEffect::Suit(Suit::Spades), vec![1])], ..Default::default() };
        for i in 0..20u64 {
            let x = deal_hidden(&Board::empty(), &start, &Rng::new(i)).unwrap().hand[1];
            assert!(!x.face_down && x.suit == Suit::Spades, "{x:?}");
        }
    }

    #[test]
    fn a_card_a_consumable_changes_takes_the_bosss_debuff_on_what_it_became() {
        // card.lua `Card:set_base`, `Card:set_ability` → blind.lua `Blind:debuff_card`; fails if a
        // changed card keeps the debuff it had
        use crate::model::Seal;
        let c = |x: &str| Card::parse_list(x).unwrap()[0];
        let under = |key: &str, cards: &str| {
            let mut b = Board::empty();
            b.blind.key = key.into();
            let mut h = Card::parse_list(cards).unwrap();
            b.debuff_cards(&mut h, &mut []);
            (b, h)
        };
        let debuffs = |h: &[Card]| h.iter().map(|c| c.debuff).collect::<Vec<_>>();
        // The Club: The Star turns two clubs into diamonds, which frees them; the club it
        // didn't go on stays debuffed
        let (club, h) = under("bl_club", "KC 9C 4C");
        assert_eq!(debuffs(&h), [true, true, true]);
        let star = effect_on(&club, &h, CardEffect::Suit(Suit::Diamonds), &[0, 1]);
        assert_eq!(debuffs(&star), [false, false, true]);
        // and The Moon turns a diamond into a club: debuffed now
        let (_, h2) = under("bl_club", "KD");
        assert!(effect_on(&club, &h2, CardEffect::Suit(Suit::Clubs), &[0])[0].debuff);
        // The Goad: The Lovers' Wild card is every suit, spades too
        let (goad, h) = under("bl_goad", "QH");
        assert!(!h[0].debuff && effect_on(&goad, &h, CardEffect::Enhance(Enhancement::Wild), &[0])[0].debuff);
        // The Plant: The Tower's Stone card is no face card; Strength makes a 10 a Jack
        let (plant, h) = under("bl_plant", "KS TH");
        assert_eq!(debuffs(&effect_on(&plant, &h, CardEffect::Enhance(Enhancement::Stone), &[0])), [false, false]);
        assert_eq!(debuffs(&effect_on(&plant, &h, CardEffect::UpRank, &[1])), [true, true]);
        // Death: the copy takes the copied card's (common_events.lua `copy_card`)
        let (_, h) = under("bl_club", "5H KC");
        assert_eq!(debuffs(&effect_on(&club, &h, CardEffect::CopyLeftToRight, &[0, 1])), [true, true]);
        // The Pillar: an enhancement set (The Empress, on a card already Mult too) makes a card
        // played this ante one no longer (card.lua `Card:set_ability` rebuilds `ability`); Death's
        // copy of a card played this ante is one too (common_events.lua `copy_card`)
        let mut pillar = Board::empty();
        pillar.blind.key = "bl_pillar".into();
        let mut h = Card::parse_list("KS:mult 5H").unwrap();
        h[0].played_this_ante = true;
        pillar.debuff_cards(&mut h, &mut []);
        assert_eq!(debuffs(&h), [true, false]);
        let freed = effect_on(&pillar, &h, CardEffect::Enhance(Enhancement::Mult), &[0]);
        assert!(!freed[0].debuff && !freed[0].played_this_ante);
        let mut h2 = vec![Card { played_this_ante: false, debuff: false, seal: Some(Seal::Red), ..h[0] }, h[0]];
        h2[1].played_this_ante = true;
        assert_eq!(debuffs(&effect_on(&pillar, &h2, CardEffect::CopyLeftToRight, &[0, 1])), [true, true]);
        // a held consumable on one of two K♠, the other played this ante: the one it names
        let mut two = Card::parse_list("KS KS").unwrap();
        two[0].played_this_ante = true;
        pillar.debuff_cards(&mut two, &mut []);
        let deja = Use { key: "c_deja_vu".into(), swap: vec![(two[1], Card { seal: Some(Seal::Red), ..two[1] })], ..Default::default() };
        let (_, after) = deja.apply(&pillar, &two).unwrap();
        assert_eq!((after[0].debuff, after[0].seal, after[1].debuff, after[1].seal), (true, None, false, Some(Seal::Red)));
        // a seal alone leaves the debuff as it was; a boss disabled debuffs nothing it changes
        let (_, h) = under("bl_club", "KC");
        assert!(effect_on(&club, &h, CardEffect::Seal(Seal::Red), &[0])[0].debuff);
        let mut off = club.clone();
        off.blind.disabled = true;
        assert!(!effect_on(&off, &h, CardEffect::Enhance(Enhancement::Wild), &[0])[0].debuff);
        // a held consumable the simulated player uses (`Use::apply`'s swap): The Star frees it
        let (_, h) = under("bl_club", "KC 2S");
        let star = Use { key: "c_star".into(), swap: vec![(c("KC"), c("KD"))], ..Default::default() };
        let (_, after) = star.apply(&club, &h).unwrap();
        assert_eq!((after[0].label(), after[0].debuff), ("K♦".to_string(), false));
        // used first on a face-down card: each round's draw changed there and debuffed by what
        // it became (The Moon under The Club)
        let mut hand = Card::parse_list("KS QH").unwrap();
        hand[1].face_down = true;
        let mut hd = Hidden::new(&hand, &[1], vec![1], false);
        hd.stand = vec![hand[1]];
        // the draw pile debuffed as the blind set it (blind.lua `Blind:set_blind`, as a save has it)
        let mut deck: Vec<Card> = standard_deck().into_iter().filter(|x| !hand.iter().any(|y| y.same_kind(x))).collect();
        club.debuff_cards(&mut [], &mut deck);
        let start = RoundStart { hand, deck, hidden: Some(std::sync::Arc::new(hd)), changes: vec![(CardEffect::Suit(Suit::Clubs), vec![1])], ..Default::default() };
        for i in 0..20u64 {
            let x = deal_hidden(&club, &start, &Rng::new(i)).unwrap().hand[1];
            assert!(x.suit == Suit::Clubs && x.debuff, "{x:?}");
        }
    }

    #[test]
    fn face_down_cards_are_drawn_anew_each_round_as_the_sorts_and_the_boss_allow() {
        // Run 1RVL49LQ's House hand, sorted by suit: K♠ ? ? 6♠ 5♠ 4♠ 3♥ 5♦ 2♦. Both face-down
        // cards are spades between 6♠ and K♠ (here Q♠ and 9♠), the higher one first in both
        // sorts.
        let mut hand = Card::parse_list("KS QS 9S 6S 5S 4S 3H 5D 2D").unwrap();
        hand[1].face_down = true;
        hand[2].face_down = true;
        let mut h = Hidden::new(&hand, &[1, 2], vec![1, 2], false);
        h.stand = vec![hand[1], hand[2]];
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|x| x.same_kind(c))).collect();
        let start = RoundStart { hand: hand.clone(), deck, hand_size: 9, hands: 3, target: 1e12, hidden: Some(std::sync::Arc::new(h)), ..Default::default() };
        let mut seen = std::collections::HashSet::new();
        for i in 0..200u64 {
            let rng = Rng::new(i);
            let s = deal_hidden(&Board::empty(), &start, &rng).unwrap();
            let (a, c) = (s.hand[1], s.hand[2]);
            assert!(a.face_down && c.face_down);
            for x in [a, c] {
                assert!(x.suit == Suit::Spades && (7..=12).contains(&x.rank.0), "{x:?}");
            }
            // the order between them kept in both sorts
            assert!(a.sort_nominal(false) >= c.sort_nominal(false) && a.sort_nominal(true) >= c.sort_nominal(true), "{a:?} {c:?}");
            // every card once: the hand and the deck still the 52
            assert_eq!(s.hand.len() + s.deck.len(), 52);
            // the same draw for the same round
            assert_eq!(deal_hidden(&Board::empty(), &start, &rng).unwrap().hand, s.hand);
            seen.insert((a.rank.0, c.rank.0));
        }
        // a fresh draw for each round: the pairs of the six spades from 7 to Q come up
        assert!(seen.len() >= 10, "{seen:?}");
        // The House's first hand, all eight face down: nothing bounds them but their order among
        // themselves, which every draw keeps in both sorts (the draw never falls back)
        let mut hand = Card::parse_list("AS KH QD 9C 7S 5H 4D 2C").unwrap();
        hand.iter_mut().for_each(|c| c.face_down = true);
        let all: Vec<usize> = (0..8).collect();
        let mut h = Hidden::new(&hand, &all, all.clone(), false);
        h.stand = hand.clone();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|x| x.same_kind(c))).collect();
        let place = h.place.clone();
        let start = RoundStart { hand: hand.clone(), deck, hidden: Some(std::sync::Arc::new(h)), ..Default::default() };
        for i in 0..200u64 {
            let d = deal_hidden(&Board::empty(), &start, &Rng::new(i)).unwrap().hand;
            for (a, b) in (0..8).flat_map(|a| (0..8).map(move |b| (a, b))) {
                for (s, by_suit) in [false, true].into_iter().enumerate() {
                    if place[a][s] < place[b][s] {
                        assert!(d[a].sort_nominal(by_suit) >= d[b].sort_nominal(by_suit), "round {i}: {:?} {:?}", d[a], d[b]);
                    }
                }
            }
        }
        // The Mark turns down only face cards: the face-down card sorts below 8♠ by suit and
        // below A♠ by rank, so it's a J, Q or K of hearts, clubs or diamonds
        let mut hand = Card::parse_list("AS 9S 8S KH").unwrap();
        hand[3].face_down = true;
        let mut h = Hidden::new(&hand, &[3], vec![3], true);
        h.stand = vec![hand[3]];
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|x| x.same_kind(c))).collect();
        let start = RoundStart { hand, deck, hidden: Some(std::sync::Arc::new(h)), ..Default::default() };
        for i in 0..50u64 {
            let c = deal_hidden(&Board::empty(), &start, &Rng::new(i)).unwrap().hand[3];
            assert!((11..=13).contains(&c.rank.0) && c.suit != Suit::Spades, "{c:?}");
        }
    }

    /// `Hidden::draw` as it was written before each pool card's sort values and order window were
    /// worked out once (2026-11-09): the plain form the draw must pick exactly as
    fn plain_draw(h: &Hidden, live: &[bool], pool: &[Card], rng: &mut Rng) -> Vec<usize> {
        type Ok<'a> = dyn Fn(usize, &Card, &[Option<Card>]) -> bool + 'a;
        let in_order = |k: usize, c: &Card, at: &[Option<Card>]| {
            (0..2).all(|s| {
                let x = c.sort_nominal(s == 1);
                at.iter().enumerate().all(|(o, d)| match d {
                    Some(d) if o != k => {
                        let y = d.sort_nominal(s == 1);
                        if h.place[o][s] < h.place[k][s] { y >= x } else { x >= y }
                    }
                    _ => true,
                })
            })
        };
        let mut seq: Vec<usize> = (0..h.pos.len()).filter(|&k| live[k]).collect();
        seq.sort_by_key(|&k| h.place[k][0]);
        for level in 0..4 {
            if level == 1 && !h.face_only {
                continue;
            }
            let ok = |k: usize, c: &Card, at: &[Option<Card>]| match level {
                0 => h.fits(k, c) && in_order(k, c, at) && h.boss_allows(c),
                1 => h.fits(k, c) && in_order(k, c, at),
                2 => h.fits(k, c),
                _ => true,
            };
            let mut at: Vec<Option<Card>> = vec![None; h.pos.len()];
            let mut pick = vec![usize::MAX; h.pos.len()];
            let mut used = vec![false; pool.len()];
            let mut budget = HIDDEN_TRIES;
            #[allow(clippy::too_many_arguments)]
            fn dfs(i: usize, seq: &[usize], pool: &[Card], ok: &Ok, at: &mut [Option<Card>], pick: &mut [usize], used: &mut [bool], budget: &mut usize, rng: &mut Rng) -> bool {
                let Some(&k) = seq.get(i) else { return true };
                let mut cands: Vec<usize> = (0..pool.len()).filter(|&j| !used[j] && ok(k, &pool[j], at)).collect();
                shuffle(&mut cands, rng);
                let mut tried: Vec<Card> = vec![];
                for j in cands {
                    if *budget == 0 {
                        return false;
                    }
                    if tried.iter().any(|t| t.order_eq(&pool[j])) {
                        continue;
                    }
                    tried.push(pool[j]);
                    *budget -= 1;
                    (used[j], at[k], pick[k]) = (true, Some(pool[j]), j);
                    let open = seq[i + 1..].iter().all(|&r| (0..pool.len()).any(|q| !used[q] && ok(r, &pool[q], at)));
                    if open && dfs(i + 1, seq, pool, ok, at, pick, used, budget, rng) {
                        return true;
                    }
                    (used[j], at[k]) = (false, None);
                }
                false
            }
            if !dfs(0, &seq, pool, &ok, &mut at, &mut pick, &mut used, &mut budget, rng) {
                continue;
            }
            for _ in 0..HIDDEN_SWEEPS {
                for &k in &seq {
                    let cur = pick[k];
                    let cands: Vec<usize> = (0..pool.len()).filter(|&j| (j == cur || !used[j]) && ok(k, &pool[j], &at)).collect();
                    let j = cands[rng.below(cands.len())];
                    (used[cur], used[j], pick[k], at[k]) = (false, true, j, Some(pool[j]));
                }
            }
            return pick;
        }
        unreachable!()
    }

    #[test]
    fn the_draw_of_face_down_cards_picks_as_its_plain_form_does() {
        // `Hidden::draw` works each pool card's sort values, bounds and the boss's rule out once,
        // and a card's order with the others as one window per sort: the same cards drawn, and
        // the same rolls taken, as its plain form (`plain_draw`), on random hands of 4 to 14
        // cards (in either sort or as dealt), some or all face down, some redrawn only in part,
        // under a boss that turns down only face cards or not, and where nothing fits a bound or
        // the order among themselves (each fallback level)
        let mut rng = Rng::new(17);
        let mut cases = 0;
        // how many draws came out at each fallback level (`Hidden::fell_back`)
        let mut levels = [0; 4];
        for t in 0..400 {
            let mut deck = standard_deck();
            shuffle(&mut deck, &mut rng);
            let n = 4 + t % 11;
            let mut hand: Vec<Card> = deck[..n].to_vec();
            match t % 3 {
                0 => hand.sort_by(|a, b| b.sort_nominal(false).total_cmp(&a.sort_nominal(false))),
                1 => hand.sort_by(|a, b| b.sort_nominal(true).total_cmp(&a.sort_nominal(true))),
                _ => {}
            }
            let slots: Vec<usize> = (0..n).filter(|_| t % 9 == 0 || rng.below(2) == 0).collect();
            if slots.is_empty() {
                continue;
            }
            for &s in &slots {
                hand[s].face_down = true;
            }
            let mut h = Hidden::new(&hand, &slots, slots.clone(), t % 4 == 0);
            if t % 7 == 0 {
                h.bounds[0] = [(100.0, 101.0), (100.0, 101.0)];
            }
            if t % 5 == 0 && slots.len() > 2 {
                h.place.swap(0, slots.len() - 1);
            }
            let live: Vec<bool> = slots.iter().map(|_| t % 6 != 0 || rng.below(3) != 0).collect();
            if !live.contains(&true) {
                continue;
            }
            // the pool as `Hidden::deal` makes it: the pile and the cards redrawn, in a fixed order
            let mut pool: Vec<Card> = deck[n..].to_vec();
            pool.extend(slots.iter().zip(&live).filter(|(_, l)| **l).map(|(&s, _)| Card { face_down: false, ..hand[s] }));
            pool.sort_by(Card::order_cmp);
            let seed = rng.next_u64();
            let (mut a, mut b) = (Rng::new(seed), Rng::new(seed));
            assert_eq!(h.draw(&live, &pool, &mut a), plain_draw(&h, &live, &pool, &mut b), "case {t}");
            assert_eq!(a.next_u64(), b.next_u64(), "case {t}: the rolls taken");
            levels[h.fell_back.load(std::sync::atomic::Ordering::Relaxed) as usize] += 1;
            cases += 1;
        }
        assert!(cases > 300, "{cases}");
        assert!(levels.iter().all(|&n| n >= 3), "draws at each fallback level: {levels:?}");
    }

    #[test]
    fn the_hook_takes_two_held_cards_before_the_hand_scores() {
        // blind.lua `Blind:press_play`: once the played cards leave the hand, 2 held cards are
        // discarded, before scoring, so held Steel cards count two fewer
        let hand = Card::parse_list("AS KH:steel KD:steel KC:steel KS:steel QH:steel QD:steel QC:steel").unwrap();
        let start = RoundStart { hand: hand.clone(), deck: standard_deck(), hand_size: 8, hands: 1, discards: 0, scored: 0.0, target: 1e12, fresh: false, ..Default::default() };
        let first = |key: &str| {
            let mut b = Board::empty();
            b.blind.key = key.into();
            outcomes_after(&b, &start, &Move::Play(vec![0]), 0..8, 3, &[]).iter().map(|o| o.total).collect::<Vec<_>>()
        };
        // High Card: 5 + 11 Chips, ×1.5 a held Steel card, floored
        let plain = first("bl_small");
        assert!(plain.iter().all(|&s| s == (16.0 * 1.5f64.powi(7)).floor()), "{plain:?}");
        let hook = first("bl_hook");
        assert!(hook.iter().all(|&s| s == (16.0 * 1.5f64.powi(5)).floor()), "{hook:?}");
        // disabled (Chicot, Luchador): held as they are
        let mut b = Board::empty();
        b.blind.key = "bl_hook".into();
        b.blind.disabled = true;
        assert_eq!(outcomes_after(&b, &start, &Move::Play(vec![0]), 0..8, 3, &[]).iter().map(|o| o.total).collect::<Vec<_>>(), plain);
    }

    #[test]
    fn under_the_hook_a_play_is_chosen_by_its_average_and_wins_surely_by_its_worst() {
        // gap 15: Baron (×1.5 a King held), a Steel King. Played, the Kings score once; held, the
        // Hook takes 2 of the 6 cards held, so they stay most of the time: the player holds them.
        // With every pair against you (the old floor) holding them looked worth nothing
        let mut b = crate::bench::sample_board(&["j_baron"]);
        b.blind.key = "bl_hook".into();
        let hand = Card::parse_list("7S 7C KH KD:steel 2H 3D 4C 5S").unwrap();
        let p = best_play(&b, &hand).unwrap();
        let played: Vec<String> = p.cards.iter().map(|&i| hand[i].label()).collect();
        assert!(!played.iter().any(|c| c.starts_with('K')), "{played:?} {} {}", p.floor, p.sure);
        assert!(p.sure < p.floor, "{} {}", p.sure, p.floor);
        // and the Kings add while held, so they're never thrown away
        let kings: Vec<usize> = (0..hand.len()).filter(|&i| hand[i].rank.0 == 13).collect();
        let held = held_value(&b, &hand, &p.cards, &standard_deck());
        assert!(kings.iter().all(|k| held.contains(k)), "{held:?}");
        // but a round it can win surely now it wins surely: the last hand, 78 to go, where the
        // Pair of 7s wins only when The Hook spares the Steel King (10 pairs of 15)
        assert!(p.surest >= 78.0 && p.sure < 78.0, "{} {}", p.surest, p.sure);
        let Action::Play(idx, _) = decide(&b, &hand, &[], 1, 0, 78.0, 8) else { panic!("discarded") };
        let played: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
        let held: Vec<Card> = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
        let o = score::score(&b, &played, &held, &mut Unlucky, false);
        assert!(o.sure >= 78.0, "{:?} surely {}", played.iter().map(Card::label).collect::<Vec<_>>(), o.sure);
    }

    #[test]
    fn under_the_hook_a_held_card_is_measured_against_a_card_drawn() {
        let pile = |hand: &[Card]| -> Vec<Card> { standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect() };
        // The Hook takes 2 of the cards held: a plain card beside a Steel King isn't worth
        // holding (left out instead, it would be, as a card the Hook might take in the King's
        // place), the King is
        let mut b = sample_board(&[]);
        b.blind.key = "bl_hook".into();
        let hand = Card::parse_list("7S 7C KD:steel 2H 3D 4C 9S JH").unwrap();
        assert_eq!(held_value(&b, &hand, &[0, 1], &pile(&hand)), [2]);
        // and with a ×Mult that isn't a short binary fraction, a plain card that adds no more
        // than a card drawn still isn't (a tie compared as differences, not sums)
        for x in [1.1, 1.3, 1.7, 1.97] {
            let mut b = sample_board(&["j_constellation"]);
            b.jokers[0].x_mult = x;
            b.blind.key = "bl_hook".into();
            assert_eq!(held_value(&b, &hand, &[0, 1], &pile(&hand)), [2], "×{x}");
        }
        // Raised Fist reads the lowest card held: with 3 held and 2 taken, the one that stays is
        // the one it reads, so the 9 and the King, above a card drawn on average, are worth
        // holding and the 2 isn't (against a card that adds nothing, all three would be).
        // Without The Hook none is (left out, the 2 lets the Fist read the 9)
        let mut b = sample_board(&["j_raised_fist"]);
        let hand = Card::parse_list("7S 7C 2H 9C KS").unwrap();
        assert!(held_value(&b, &hand, &[0, 1], &pile(&hand)).is_empty());
        b.blind.key = "bl_hook".into();
        assert_eq!(held_value(&b, &hand, &[0, 1], &pile(&hand)), [3, 4]);
        // with Baron and only two Kings held, the Hook takes both before the hand scores
        let mut b = sample_board(&["j_baron"]);
        b.blind.key = "bl_hook".into();
        let hand = Card::parse_list("7S 7C KD KH").unwrap();
        assert!(held_value(&b, &hand, &[0, 1], &pile(&hand)).is_empty());
    }

    #[test]
    fn the_hook_plays_in_every_hand_of_the_round_and_leaves_its_draws_alone() {
        let hook = |b: &mut Board| b.blind.key = "bl_hook".into();
        // the rest of the round (`sim_round`) too: Four of a Kind played, the Queens held are
        // Steel, and the Hook takes at least one of them every time (2 of AS QH QD QC)
        let hand = Card::parse_list("KH KD KC KS AS QH:steel QD:steel QC:steel").unwrap();
        let start = RoundStart { hand, deck: vec![], hand_size: 8, hands: 1, discards: 0, scored: 0.0, target: 1e12, fresh: false, ..Default::default() };
        for seed in 0..8 {
            let plain = sim_round(&Board::empty(), &start, &mut Rng::new(seed));
            let mut b = Board::empty();
            hook(&mut b);
            let hooked = sim_round(&b, &start, &mut Rng::new(seed));
            assert_eq!(plain.plays[0].0, hooked.plays[0].0, "the same hand played");
            assert!(hooked.total < plain.total, "seed {seed}: {} vs {}", hooked.total, plain.total);
        }
        // Burnt Joker doesn't level the Hook's discard on the way (a High Card of 16, not 42)
        let mut b = crate::bench::sample_board(&["j_burnt"]);
        hook(&mut b);
        let start = RoundStart { hand: Card::parse_list("AS KH QD JC 9S 7H 5D 3C").unwrap(), deck: standard_deck(), hand_size: 8, hands: 1, discards: 0, scored: 0.0, target: 1e12, fresh: false, ..Default::default() };
        assert!(outcomes_after(&b, &start, &Move::Play(vec![0]), 0..8, 3, &[]).iter().all(|o| o.total == 16.0));
        // what goes depends on the cards, not on how they sit; a move holding one more card that
        // stays loses the same two; and over many rounds every card goes about as often
        let mut b = Board::empty();
        hook(&mut b);
        let held = Card::parse_list("AS KH QD JC 9S 7H 5D").unwrap();
        let taken = |cards: &[Card], seed: u64| {
            let mut left = cards.to_vec();
            boss_takes_held(&mut b.clone(), &mut left, &Rng::new(seed));
            let mut gone: Vec<String> = cards.iter().filter(|c| !left.contains(c)).map(Card::label).collect();
            gone.sort();
            gone
        };
        let mut count = std::collections::HashMap::new();
        for seed in 0..2000 {
            let gone = taken(&held, seed);
            assert_eq!(gone.len(), 2);
            assert_eq!(gone, taken(&held.iter().rev().copied().collect::<Vec<_>>(), seed));
            let fewer: Vec<Card> = held.iter().filter(|c| gone.contains(&c.label()) || c.label() != held[6].label()).copied().collect();
            if fewer.len() == 6 {
                assert_eq!(taken(&fewer, seed), gone, "seed {seed}");
            }
            for g in gone {
                *count.entry(g).or_insert(0) += 1;
            }
        }
        // 2 of 7: each card about 571 times in 2000
        assert!(count.values().all(|&n| (480..=660).contains(&n)), "{count:?}");
    }

    const RULE_BOARDS: &[&[&str]] = &[&["j_joker"], &["j_four_fingers"], &["j_shortcut"], &["j_smeared"], &["j_four_fingers", "j_shortcut", "j_smeared"]];

    #[test]
    fn the_plays_a_hands_structure_gives_are_every_play_whose_cards_all_score() {
        // against every subset, on hands small enough to try them all, with the rules that
        // change what scores
        let mut rng = Rng::new(20);
        let mut checked = 0;
        for keys in RULE_BOARDS {
            let f = sample_board(keys).rule_flags();
            for _ in 0..60 {
                let n = 5 + rng.below(8);
                let hand = random_hand(&mut rng, n);
                let given: std::collections::HashSet<u64> = scoring_plays(&hand, f).into_iter().collect();
                for m in 1u64..(1 << n) {
                    if m.count_ones() > 5 {
                        continue;
                    }
                    let played: Vec<Card> = mask_cards(m).iter().map(|&i| hand[i]).collect();
                    if hand::detect(&played, f).scoring.len() == played.len() {
                        assert!(given.contains(&m), "{keys:?}: {} missing from {}", labels(&played), labels(&hand));
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 10_000, "{checked}");
    }

    #[test]
    fn a_big_hands_moves_come_from_every_card() {
        // 40 cards, the five 8s (two 8♠: Five of a Kind, the best play) last, at bits 34-38,
        // behind 24 higher cards and 9 Stone cards, where the first-12 cut never looked: it leads the plays, the best few of
        // each kind are in, and a discard keeps it, throwing the lowest cards outside it
        let mut text = vec![];
        for r in ["A", "K", "Q", "J", "T", "9"] {
            for s in ["S", "H", "D", "C"] {
                text.push(format!("{r}{s}"));
            }
        }
        text.extend(std::iter::repeat_n("AS:stone".to_string(), 9));
        text.extend(["7S", "8S", "8H", "8C", "8D", "8S", "3H"].map(String::from));
        let hand = Card::parse_list(&text.join(" ")).unwrap();
        assert_eq!(hand.len(), 40);
        let b = sample_board(&["j_joker"]);
        let moves = big_hand_moves(&b, std::slice::from_ref(&hand), &standard_deck(), 3);
        let plays: Vec<&Vec<usize>> = moves.iter().filter_map(|m| if let Move::Play(v) = m { Some(v) } else { None }).collect();
        assert!(plays.len() <= BIG_HAND_PLAYS);
        let mut eights: Vec<usize> = (0..hand.len()).filter(|&i| hand[i].rank.0 == 8).collect();
        let mut best = plays[0].clone();
        best.sort();
        assert_eq!(best, eights, "the best play");
        let kinds: std::collections::HashSet<HandType> = plays.iter().map(|v| hand::detect(&v.iter().map(|&i| hand[i]).collect::<Vec<_>>(), b.rule_flags()).hand).collect();
        assert!(kinds.len() >= 6, "{kinds:?}");
        // keeping the five 8s, the 5 lowest outside them: 3♥ 7♠ and three 9s
        eights.sort();
        let lowest = |v: &Vec<usize>| v.len() == 5 && v.iter().all(|i| !eights.contains(i)) && v.iter().filter(|&&i| hand[i].rank.0 < 8).count() == 2;
        assert!(moves.iter().any(|m| matches!(m, Move::Discard(v) if lowest(v))), "{moves:?}");
    }

    #[test]
    fn a_big_hands_moves_read_its_face_down_cards_as_drawn_not_as_their_stand_ins() {
        // 14 cards (Juggle Tags), a face-down 9♠ or 8♠ the sorts can't tell apart (between K♠ and
        // 7♠ by suit, between the Q and the 7s by rank), the other in the pile: the moves come
        // from the draws, so they're the same either way, and some hold that card (gap 25 (c))
        let mut nine = Card::parse_list("AS KS 9S 7S 2S AH KH QH 7H 7D 6D 5C 4C 3C").unwrap();
        nine[2].face_down = true;
        let mut eight = nine.clone();
        eight[2] = Card { face_down: true, ..Card::parse_list("8S").unwrap()[0] };
        let b = sample_board(&["j_joker"]);
        let start = |hand: &[Card], slots: &[usize]| {
            let mut h = Hidden::new(hand, slots, slots.to_vec(), false);
            h.stand = slots.iter().map(|&i| hand[i]).collect();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|x| x.same_kind(c))).collect();
            RoundStart { hand: hand.to_vec(), deck, hand_size: hand.len() as i64, hands: 4, discards: 3, target: 5000.0, hidden: Some(std::sync::Arc::new(h)), ..Default::default() }
        };
        let moves = |hand: &[Card], slots: &[usize]| {
            let s = start(hand, slots);
            big_hand_moves(&b, &hidden_hands(&b, &s, 42, BIG_HAND_RANK_DRAWS), &s.deck, s.discards)
        };
        let (x, y) = (moves(&nine, &[2]), moves(&eight, &[2]));
        assert_eq!(x, y);
        assert!(x.iter().any(|m| matches!(m, Move::Play(v) if v.contains(&2))));
        assert!(x.iter().any(|m| matches!(m, Move::Discard(v) if v.contains(&2))));
        // reading the cards as they are (the old way, a stand-in read as real) told them apart
        assert_ne!(big_hand_moves(&b, std::slice::from_ref(&nine), &[], 3), big_hand_moves(&b, std::slice::from_ref(&eight), &[], 3));
        // The House with 14 cards, all face down: moves from the draws over every card, the two
        // lowest by the rank sort too (no cut to the 12 highest), plays and discards
        let mut all = nine.clone();
        all.iter_mut().for_each(|c| c.face_down = true);
        let slots: Vec<usize> = (0..all.len()).collect();
        let m = moves(&all, &slots);
        assert!(m.iter().filter(|m| matches!(m, Move::Play(_))).count() > 20, "{m:?}");
        for i in 0..all.len() {
            assert!(m.iter().any(|m| matches!(m, Move::Play(v) if v.contains(&i))), "card {i} in no play");
            assert!(m.iter().any(|m| matches!(m, Move::Discard(v) if v.contains(&i))), "card {i} in no discard");
        }
    }

    #[test]
    fn the_simulated_player_burns_discards_only_when_the_board_scores_more_without_them() {
        // On pace with a Pair, so without a reason to discard the player plays it. With
        // Mystic Summit the best play scores more with no discards left: discard first.
        let hand = Card::parse_list("AS AH KD 9C 7S 5H 3D 2C").unwrap();
        // no card of a rank in hand left to draw: no Three of a Kind or Two Pair to dig for
        // while on pace
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank)).collect();
        let act = |keys: &[&str]| {
            let mut b = sample_board(keys);
            b.discards_left = 2;
            b.hands_left = 3;
            let pair = best_play(&b, &hand).unwrap().floor;
            decide(&b, &hand, &deck, 3, 2, pair * 2.0, 8)
        };
        assert!(matches!(act(&["j_mystic_summit"]), Action::Discard(_)), "Mystic Summit: burn first");
        assert!(matches!(act(&["j_joker"]), Action::Play(..)), "a plain joker: play the Pair");
    }

    #[test]
    fn the_last_hand_digs_for_what_reaches_the_score_needed() {
        // One hand left with 3 discards, 316 needed, no jokers (the bench's plain_1h3d). Four
        // clubs make a flush that scores 288-320 by the club drawn (a 9 or higher reaches it, 3
        // of the 9 clubs left), so most complete short;
        // A K Q J make a straight of 324 with any 10. In points the flush's plan scores more, but
        // only the straight surely reaches: the last hand weighs reaching it.
        let hand = Card::parse_list("KC JS 2H AC 8H KS QC 4C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.contains(c)).collect();
        let b = sample_board(&[]);
        let thrown = |reach: bool| match decide_cards_with(&b, &hand, &deck, 1, 3, 316.0, 8, true, None, reach) {
            Action::Discard(v) => v,
            a => panic!("{a:?}"),
        };
        let keeps_straight = |v: &[usize]| [1, 3, 6].iter().all(|i| !v.contains(i)) && !(v.contains(&0) && v.contains(&5));
        assert!(keeps_straight(&thrown(true)), "{:?}", thrown(true));
        assert!(!keeps_straight(&thrown(false)), "in points it digs for the flush: {:?}", thrown(false));
    }

    #[test]
    fn playing_now_on_the_last_hand_counts_its_rolls_that_reach() {
        // Misprint's Mult is rolled: a Pair of Aces reaches a score between its floor and its
        // top on some rolls only, so playing it now is worth that share of the score needed;
        // with no roll asked, all or nothing
        let hand = Card::parse_list("AS AH KD 9C 7S 5H 3D 2C").unwrap();
        let b = sample_board(&["j_misprint"]);
        let play = [0, 1];
        let played: Vec<Card> = play.iter().map(|&i| hand[i]).collect();
        let held: Vec<Card> = hand[2..].to_vec();
        let floor = score::score(&b, &played, &held, &mut Unlucky, false).score;
        let top = score::score(&b, &played, &held, &mut crate::engine::Lucky, false).score;
        let need = (floor + top) / 2.0;
        let v = play_reach(&b, &hand, &play, need, false);
        assert!(v > 0.0 && v < need, "{floor} {top} {v}");
        let plain = sample_board(&["j_joker"]);
        let x = score::score(&plain, &played, &held, &mut Unlucky, false).score;
        assert_eq!(play_reach(&plain, &hand, &play, x, false), x);
        assert_eq!(play_reach(&plain, &hand, &play, x + 1.0, false), 0.0);
    }

    #[test]
    fn a_made_flush_is_a_plan_on_the_last_hand_only() {
        // Q 10 8 6 3 of diamonds: a flush made, short of what's needed only on the last hand
        let hand = Card::parse_list("QD TD 8D 6D 3D 4S TH 2C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.contains(c)).collect();
        let b = sample_board(&[]);
        let made = |last: bool| aims(&b, &hand, &deck, last).into_iter().any(|a| a.hand == HandType::Flush && a.keep == [0, 1, 2, 3, 4] && a.groups[0].1 == 1);
        assert!(made(true));
        assert!(!made(false));
    }

    #[test]
    fn on_the_last_hand_a_completion_is_scored_after_the_burn() {
        // Mystic Summit's Mult with no discards left: the policy burns them before playing the
        // made hand when that scores more, so the plan's completions are scored so
        let hand = Card::parse_list("AH KH 9H 5H 2C 3S 7D 8C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.contains(c)).collect();
        let mut b = sample_board(&["j_mystic_summit"]);
        b.discards_left = 2;
        let a = aims(&b, &hand, &deck, true).into_iter().find(|a| a.hand == HandType::Flush).unwrap();
        assert!(aim_score(&b, &a, &hand, &[], None, None, true) > aim_score(&b, &a, &hand, &[], None, None, false));
        // and only reaching counts there: a score needed above every completion is worth nothing
        assert_eq!(aim_score(&b, &a, &hand, &[], None, Some(1e9), true), 0.0);
    }

    #[test]
    fn a_save_is_spent_once_for_each_copy() {
        // Rounds the board loses but reaches a quarter of: the first Mr. Bones saves one and is
        // gone (state_events.lua `end_round`, card.lua `start_dissolve`), a second copy is left
        // for the next lost round
        let start = |target: f64| RoundStart { deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, target, fresh: true, ..Default::default() };
        let one = sample_board(&["j_mr_bones"]);
        let two = sample_board(&["j_mr_bones", "j_mr_bones"]);
        let mean = round_odds(&one, &start(1e12), 200, 3).1.mean;
        let rounds = vec![start(mean * 2.0); 4];
        let through = |b: &Board| {
            let runs = chained_through(b, &rounds, 200, 5);
            let n = runs.len() as f64;
            // (rounds got through on score: up to the first saved one)
            let on_score = |r: &(usize, u64)| if r.1 == 0 { r.0 } else { r.1.trailing_zeros() as usize };
            (runs.iter().map(|r| on_score(r) as f64).sum::<f64>() / n, runs.iter().map(|r| r.0 as f64).sum::<f64>() / n)
        };
        let ((one_score, one_saved), (two_score, two_saved)) = (through(&one), through(&two));
        assert!(one_saved > one_score + 0.8 && two_saved > one_saved + 0.6, "one copy {one_score} → {one_saved}, two copies {two_score} → {two_saved}");
        // the round a save ends leaves the board without the copy that saved it
        let mut rng = Rng::new(9);
        let (r, end) = play_round(&two, &start(mean * 2.0), &mut rng, &[]);
        assert!(r.saved, "this round is lost above a quarter of the blind");
        assert_eq!(end.round_saves(), 1);
        assert_eq!(two.saves_lost_round(0.2, 1.0), None, "under a quarter of the blind nothing saves");
    }

    #[test]
    fn the_best_play_shortcut_changes_nothing() {
        // `could_all_score` only skips plays hand detection would rule out: the same best
        // play with it and without, on random hands (Stone and Wild cards among them, some
        // debuffed; every other hand from two suits, so flushes are common) and boards with
        // the rules that change hands
        let mut rng = Rng::new(5);
        let boards: [&[&str]; 7] = [&[], &["j_four_fingers"], &["j_shortcut", "j_smeared"], &["j_jolly", "j_droll"], &["j_four_fingers", "j_shortcut"], &["j_pareidolia", "j_four_fingers", "j_smeared"], &["j_smeared", "j_droll"]];
        for keys in boards {
            let b = sample_board(keys);
            for t in 0..600 {
                let mut deck = standard_deck();
                if t % 2 == 1 {
                    deck.retain(|c| matches!(c.suit, Suit::Hearts | Suit::Spades) || (t % 4 == 1 && c.suit == Suit::Diamonds));
                }
                shuffle(&mut deck, &mut rng);
                let mut hand: Vec<Card> = deck[..8].to_vec();
                if rng.below(3) == 0 {
                    hand[rng.below(8)].enhancement = Some(Enhancement::Stone);
                }
                if rng.below(3) == 0 {
                    hand[rng.below(8)].enhancement = Some(Enhancement::Wild);
                }
                if rng.below(4) == 0 {
                    hand[rng.below(8)].debuff = true;
                }
                let fast = best_play_with(&b, &hand, true).map(|p| (p.cards, p.floor));
                let full = best_play_with(&b, &hand, false).map(|p| (p.cards, p.floor));
                assert_eq!(fast, full, "{keys:?} {hand:?}");
            }
        }
    }

    #[test]
    fn pace_counts_a_random_jokers_average() {
        // Misprint adds +0 to +23 Mult: its floor (+0) says the Pair is behind, its average
        // says the Pair keeps pace. The player plays it, as it would with a fixed +12 Mult.
        let hand = Card::parse_list("AS AH KD 9C 7S 5H 3D 2C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
        let b = sample_board(&["j_misprint"]);
        let p = best_play(&b, &hand).unwrap();
        assert!(p.mean > 2.0 * p.floor, "the average counts the rolls: {} vs {}", p.mean, p.floor);
        let need = 3.0 * (p.floor + p.mean) / 2.0;
        assert!(matches!(decide(&b, &hand, &deck, 3, 2, need, 8), Action::Play(..)));
    }

    #[test]
    fn the_last_hand_digs_with_a_discard_left_when_it_cant_win_yet() {
        // One hand left, one discard, a Pair of Aces short of the target: discarding the cards
        // outside the Pair keeps it and may improve it, so the player discards first; with no
        // discard left it plays; one that already wins is played. Fails if the last hand is
        // always played at once.
        let hand = Card::parse_list("AS AH KD 9C 7S 5H 3D 2C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
        let b = sample_board(&["j_joker"]);
        let pair = best_play(&b, &hand).unwrap();
        let Action::Discard(v) = decide(&b, &hand, &deck, 1, 1, pair.floor * 3.0, 8) else { panic!("a discard is left: dig first") };
        assert!(!v.iter().any(|&i| hand[i].rank.0 == 14), "the Pair stays: {v:?}");
        assert!(matches!(decide(&b, &hand, &deck, 1, 0, pair.floor * 3.0, 8), Action::Play(..)));
        assert!(matches!(decide(&b, &hand, &deck, 1, 1, pair.floor, 8), Action::Play(..)));
    }

    #[test]
    fn cards_that_score_while_held_arent_discarded() {
        // Behind with a Pair of Aces: the player digs with what's outside it, but Baron's
        // Kings score while held (×1.5 each), so they stay; the rest goes.
        // (two cards of each suit and one discard: no flush worth chasing)
        let hand = Card::parse_list("AS AH KD KC 9H 7S 3D 2C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
        let b = sample_board(&["j_baron"]);
        let pair = best_play(&b, &hand).unwrap();
        let Action::Discard(v) = decide(&b, &hand, &deck, 2, 1, pair.mean * 10.0, 8) else { panic!("behind: dig") };
        let tossed: Vec<String> = v.iter().map(|&i| hand[i].label()).collect();
        assert!(!tossed.iter().any(|c| c.starts_with('K')), "{tossed:?}");
        assert_eq!(tossed.len(), 4, "{tossed:?}");
    }

    #[test]
    fn a_pair_is_dug_toward_two_pair_and_three_of_a_kind_toward_a_full_house() {
        // one more of a kept single's rank beside the group (misc_functions.lua `get_X_same`):
        // with one or two singles, those with the most cards of their rank left first; not when
        // the hand is already made. Fails without these plans.
        let plans = |cards: &str, extra: &[&str], want: HandType| -> Vec<(Vec<usize>, usize)> {
            let hand = Card::parse_list(cards).unwrap();
            let mut deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
            deck.retain(|c| !extra.iter().any(|e| Card::parse(e).unwrap().same_kind(c)));
            aims(&sample_board(&[]), &hand, &deck, false).into_iter().filter(|a| a.hand == want).map(|a| (a.keep, a.groups[0].0.len())).collect()
        };
        // a Pair of 9s, singles K Q 7 4 3 2: all have 3 left, so the higher first; the plans keep
        // the Pair and one or two singles (`AIM_SINGLES`), 3 more cards of each
        let two = plans("9S 9H KD QC 7S 4H 2D 3C", &[], HandType::TwoPair);
        assert_eq!(two, [(vec![0, 1, 2], 3), (vec![0, 1, 2, 3], 6)], "{two:?}");
        // with two Kings gone from the deck the Queen (3 left) comes first
        let fewer = plans("9S 9H KD QC 7S 4H 2D 3C", &["KS", "KH"], HandType::TwoPair);
        assert_eq!(fewer[0], (vec![0, 1, 3], 3), "{fewer:?}");
        // Three of a Kind: a Full House from one more of a single
        let full = plans("9S 9H 9D QC 7S 4H 2D 3C", &[], HandType::FullHouse);
        assert_eq!(full[0], (vec![0, 1, 2, 3], 3), "{full:?}");
        // made already: Two Pair isn't dug for with two Pairs, nor a Full House with one
        assert!(plans("9S 9H KD KC 7S 4H 2D 3C", &[], HandType::TwoPair).is_empty());
        assert!(plans("9S 9H 9D KC KS 4H 2D 3C", &[], HandType::FullHouse).is_empty());
    }

    #[test]
    fn every_completion_of_a_plan_makes_its_hand() {
        // A plan's completions are scored as its hand: a plan keeping five or more cards (Two
        // Pair with three singles, a Full House with two) is played as its best five, which hold
        // the card that completes it. Fails if a completion is cut to the kept cards.
        let b = sample_board(&[]);
        for cards in ["9S 9H KD QC 7S 4H 2D 3C", "9S 9H 9D QC 7S 4H 2D 3C", "5H 6H 7H 8H KS QC 3D 2C", "AS AH KS KH 9C 9D 2S 3S", "2H 5H 9H JH KD QC 7S 3C"] {
            let hand = Card::parse_list(cards).unwrap();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
            for a in aims(&b, &hand, &deck, false) {
                for (c, _) in aim_completions(&b, &a, &hand) {
                    assert!(c.len() <= 5 && hand::detect(&c, b.rule_flags()).contains(a.hand), "{cards}: {:?} keep {:?}: {:?}", a.hand, a.keep, c.iter().map(|x| x.label()).collect::<Vec<_>>());
                }
            }
        }
    }

    #[test]
    fn playing_on_counts_what_the_play_grows() {
        // Playing on is the best play now, then the best play of the same cards again each later
        // hand on the board the hand before leaves (the game's own step): two Spare Trousers grow
        // +2 Mult each from every Two Pair, so three hands are worth more than three times this
        // one; Ice Cream loses 5 Chips a hand, so less; a plain Joker exactly three times. The
        // round's memory counts too: Card Sharp's ×3 for a hand type played this round, Acrobat's
        // on the last hand, and The Arm lowers the hand's level each time; The Eye's ban on the
        // hands played since isn't carried (the cards held stand in for new ones, which make other
        // hands). Fails if later
        // hands are this one repeated, or if the board doesn't move on with them.
        let hand = Card::parse_list("AS AH KD KC 9C 7S 3D 2C").unwrap();
        let on = |keys: &[&str], blind: &str| {
            let mut b = sample_board(keys);
            b.new_round(blind);
            b.hands_left = 3;
            let best = best_play(&b, &hand).unwrap();
            (best.mean, playing_on(&b, &hand, &best, 3))
        };
        let (now, three) = on(&["j_trousers", "j_trousers"], "");
        assert!(three > now * 3.1, "Trousers grow: {now} -> {three}");
        // The Arm lowers a played hand's level: a Two Pair at level 3 scores less each time
        let levelled = |blind: &str| {
            let mut b = sample_board(&["j_trousers", "j_trousers"]);
            b.new_round(blind);
            b.hands_left = 3;
            let tp = HandType::TwoPair as usize;
            b.levels[tp] = b.levels[tp].with_level(3);
            let best = best_play(&b, &hand).unwrap();
            playing_on(&b, &hand, &best, 3)
        };
        assert!(levelled("bl_arm") < levelled(""), "The Arm lowers the Two Pair");
        let (_, eye) = on(&["j_trousers", "j_trousers"], "bl_eye");
        assert_eq!(eye, three, "The Eye's ban isn't carried");
        // a ban already in place stays: with Two Pair played, the Pair is played on
        let mut b = sample_board(&["j_joker"]);
        b.new_round("bl_eye");
        b.hands_left = 3;
        b.blind.eye_seen = HandType::TwoPair.bit();
        let best = best_play(&b, &hand).unwrap();
        assert_eq!(best.hand, HandType::Pair);
        for shortcut in [true, false] {
            assert!((playing_on_with(&b, &hand, &best, 3, shortcut, false, None) - 3.0 * best.mean).abs() < 1e-9 * best.mean, "{shortcut}");
        }
        let (now, three) = on(&["j_ice_cream"], "");
        assert!(three < now * 3.0, "Ice Cream fades: {now} -> {three}");
        for keys in [&["j_card_sharp"][..], &["j_acrobat"]] {
            let (now, three) = on(keys, "");
            assert!(three > now * 3.1, "{keys:?}: the round's memory pays: {now} -> {three}");
        }
        let (now, three) = on(&["j_joker"], "");
        assert!((three - now * 3.0).abs() < 1e-9 * now, "a plain Joker: {now} -> {three}");
    }

    #[test]
    fn the_playing_on_shortcut_changes_nothing() {
        // `playing_on` skips the search for a later hand when its jokers and its play's score
        // didn't move: the same total as searching every hand, for every joker in the game data
        // under no boss and the bosses that remember the hand (The Eye, The Mouth, The Arm on
        // hands at level 2), on random hands
        let mut rng = Rng::new(9);
        let keys: Vec<String> = crate::data::GameData::bundled().centers.iter().filter(|c| c.key.starts_with("j_")).map(|c| c.key.clone()).collect();
        for key in &keys {
            for blind in ["", "bl_eye", "bl_mouth", "bl_arm"] {
                let mut b = sample_board(&[key.as_str(), "j_joker"]);
                b.new_round(blind);
                for l in b.levels.iter_mut() {
                    *l = l.with_level(2);
                }
                for _ in 0..4 {
                    let mut deck = standard_deck();
                    shuffle(&mut deck, &mut rng);
                    let hand = deck[..8].to_vec();
                    let Some(best) = best_play(&b, &hand) else { continue };
                    let (fast, full) = (playing_on_with(&b, &hand, &best, 4, true, false, None), playing_on_with(&b, &hand, &best, 4, false, false, None));
                    assert!((fast - full).abs() <= 1e-9 * full.abs(), "{key} {blind} {hand:?}: {fast} vs {full}");
                }
            }
        }
    }

    #[test]
    fn the_later_hands_front_finds_what_searching_them_all_finds() {
        // `playing_on`'s later hands search only the plays the last full search left unbeaten
        // (`later_best`), from the decision's own search of the hand (`HandParts::shortlist`) or
        // the first later hand's: for every joker in the game data beside one that grows, fades or
        // retriggers in the round (after it over 4 hands, before it over 6), under no boss, The
        // Arm and The Mouth, with and without the Plasma Deck, on random hands of enhanced cards
        // with editions and random levels, the total as searching every hand nearly always, and
        // never far off
        let mut rng = Rng::new(7);
        let keys: Vec<String> = crate::data::GameData::bundled().centers.iter().filter(|c| c.key.starts_with("j_")).map(|c| c.key.clone()).collect();
        let movers = ["j_dusk", "j_ice_cream", "j_selzer", "j_bull", "j_ride_the_bus", "j_supernova", "j_green_joker", "j_runner", "j_square", "j_trousers"];
        let enh = [None, None, None, Some(Enhancement::Bonus), Some(Enhancement::Mult), Some(Enhancement::Glass), Some(Enhancement::Steel), Some(Enhancement::Lucky)];
        let edi = [None, None, None, None, Some(Edition::Foil), Some(Edition::Holo), Some(Edition::Polychrome)];
        let (mut cases, mut differ, mut worst) = (0, [0, 0], 1.0f64);
        for mover in movers {
            for key in &keys {
                for (blind, plasma, first) in ["", "bl_arm", "bl_mouth"].into_iter().flat_map(|bl| [(bl, false, false), (bl, true, false), (bl, false, true)]) {
                    let (mut b, hands) = if first { (sample_board(&[mover, key.as_str()]), 6) } else { (sample_board(&[key.as_str(), mover]), 4) };
                    if b.jokers.len() < 2 {
                        continue;
                    }
                    b.new_round(blind);
                    b.plasma = plasma;
                    b.hands_left = hands;
                    // Ice Cream and Seltzer run out within the round
                    for j in b.jokers.iter_mut() {
                        if j.kind == Kind::IceCream {
                            j.extra.chips = 15.0;
                        }
                        if j.kind == Kind::Seltzer {
                            j.extra.n = 2.0;
                        }
                    }
                    for l in b.levels.iter_mut() {
                        *l = l.with_level(1 + rng.below(4) as i64);
                    }
                    for _ in 0..2 {
                        let mut deck = standard_deck();
                        for c in deck.iter_mut() {
                            c.enhancement = enh[rng.below(enh.len())];
                            c.edition = edi[rng.below(edi.len())];
                        }
                        shuffle(&mut deck, &mut rng);
                        let hand = deck[..8].to_vec();
                        let Some(best) = best_play(&b, &hand) else { continue };
                        let start = {
                            let parts = HandParts::new(&b, &hand);
                            parts.best(&(0..hand.len()).collect::<Vec<_>>());
                            parts.shortlist()
                        };
                        let full = playing_on_with(&b, &hand, &best, hands, true, false, None);
                        let shorts = [playing_on_with(&b, &hand, &best, hands, true, true, None), playing_on_with(&b, &hand, &best, hands, true, true, start)];
                        cases += 1;
                        for (k, short) in shorts.into_iter().enumerate() {
                            if (short - full).abs() > 1e-9 * full.abs() {
                                differ[k] += 1;
                                worst = worst.min(short / full);
                            }
                        }
                    }
                }
            }
        }
        eprintln!("front: {differ:?} of {cases} differ (from the first later hand, from the hand's own search), worst {worst:.4}");
        assert!(differ.iter().all(|&d| d * 1000 <= cases) && worst >= 0.9, "{differ:?} of {cases}, worst {worst}");
    }

    #[test]
    fn a_growing_board_keeps_pace_by_what_it_grows_to() {
        // Two Spare Trousers and a Two Pair with a flush draw beside it (no Ace or King left to
        // draw: no Full House to dig for with a free discard): the target is more than the Two
        // Pair three times over but less than playing it on while the Trousers grow, so the
        // player plays it (on pace) instead of throwing cards for the flush. Fails if pace is the
        // best play times the hands.
        let hand = Card::parse_list("AS AH KD KC 9H 7H 3H 2H").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| c.rank.0 < 13 && !hand.iter().any(|h| h.same_kind(c))).collect();
        let mut b = sample_board(&["j_trousers", "j_trousers"]);
        b.hands_left = 3;
        let best = best_play(&b, &hand).unwrap();
        let on = playing_on(&b, &hand, &best, 3);
        let need = (best.mean * 3.0 + on) / 2.0;
        assert!(best.mean * 3.0 < need && need < on, "{} {need} {on}", best.mean);
        let a = decide(&b, &hand, &deck, 3, 2, need, 8);
        assert!(matches!(a, Action::Play(ref v, false) if v.len() == 4), "{a:?}");
    }

    #[test]
    fn a_straight_flush_held_whole_leaves_no_room_for_its_flush_part() {
        // Five cards of a suit in a row are the straight part and the flush part at once: no room
        // for the suit's other cards, nothing to draw, so no plan (already made); the suit's other
        // runs still are. Panics in a debug build (overflow checks: `cargo test`, not `--release`)
        // if the flush part's room goes below 0.
        for (cards, keys) in [("5H 6H 7H 8H 9H KH 2H 3C", &[][..]), ("5H 6D 7H 8D 9H KH 2D 3C", &["j_smeared"][..]), ("AH 2H 3H 4H 5H KH QH 3C", &[][..])] {
            let hand = Card::parse_list(cards).unwrap();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
            let all = aims(&sample_board(keys), &hand, &deck, false);
            assert!(all.iter().all(|a| a.keep.len() <= 5), "{cards}: {:?}", all.iter().map(|a| (a.hand, a.keep.clone())).collect::<Vec<_>>());
            // the run held (cards 0-4) is no plan; the run one rank up from its top four is
            let sf: Vec<&Vec<usize>> = all.iter().filter(|a| a.hand == HandType::StraightFlush).map(|a| &a.keep).collect();
            assert!(!sf.iter().any(|k| (0..5).all(|i| k.contains(&i))), "{cards}: made already {sf:?}");
            assert!(sf.contains(&&vec![1, 2, 3, 4]), "{cards}: {sf:?}");
        }
    }

    #[test]
    fn a_straight_flush_is_a_plan_of_its_own() {
        // A straight flush is its own hand (scored at its own level): the game finds its flush
        // part and straight part apart (misc_functions.lua `evaluate_poker_hand`)
        let plans = |cards: &str, keys: &[&str]| -> Vec<(Vec<usize>, Vec<String>)> {
            let hand = Card::parse_list(cards).unwrap();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
            aims(&sample_board(keys), &hand, &deck, false)
                .into_iter()
                .filter(|a| a.hand == HandType::StraightFlush && a.groups.len() == 1)
                .map(|a| {
                    let mut v: Vec<String> = a.groups[0].0.iter().map(|c| c.label()).collect();
                    v.sort();
                    (a.keep, v)
                })
                .collect()
        };
        // four Hearts in a row: the Hearts at either end complete it
        assert_eq!(plans("5H 6H 7H 8H KS QC 3D 2C", &[]), [(vec![0, 1, 2, 3], vec!["4♥".to_string(), "9♥".into()])]);
        // a Wild card is every suit
        assert_eq!(plans("5H 6H 7S:wild 8H KS QC 3D 2C", &[]), [(vec![0, 1, 2, 3], vec!["4♥".to_string(), "9♥".into()])]);
        // Smeared Joker: Diamonds are Hearts too, each plan once (not once per suit); the 3♦
        // makes another (3♦ 5♥ 6♥ 7♥ and a red 4)
        let smeared = plans("5H 6H 7H 8H KS QC 3D 2C", &["j_smeared"]);
        assert_eq!(smeared.iter().filter(|p| p.0 == [0, 1, 2, 3]).collect::<Vec<_>>(), [&(vec![0, 1, 2, 3], vec!["4♥".to_string(), "4♦".into(), "9♥".into(), "9♦".into()])]);
        assert_eq!(smeared.len(), 2, "{smeared:?}");
        // Four Fingers: three Hearts in a row and another Heart are the flush part, so a 4 or an
        // 8 of any suit makes the straight part (5♥ 6♥ 7♥ 8♠ 2♥ is a straight flush)
        // a made straight on a straight flush draw: the off-suit 8 goes, the 8♥ is drawn
        // (without Four Fingers every card of the play is of the suit)
        assert_eq!(plans("5H 6H 7H 8S 9H KS QC 2D", &[]), [(vec![0, 1, 2, 4], vec!["8♥".to_string()])]);
        // with Four Fingers a made straight with three Hearts in it needs any Heart (or an 8♥
        // for the 8♠, the same draw)
        let made = plans("5H 6S 7H 8H KS QC JD 2C", &["j_four_fingers"]);
        assert!(made.iter().any(|p| p.0 == [0, 1, 2, 3] && p.1.len() == 13 - 3), "{made:?}");
        let ff = plans("5H 6H 7H 2H KS QC JD 9C", &["j_four_fingers"]);
        let any: Vec<String> = ["4♠", "4♥", "4♣", "4♦", "8♠", "8♥", "8♣", "8♦"].iter().map(|s| s.to_string()).collect();
        let mut any = any;
        any.sort();
        assert!(ff.contains(&(vec![0, 1, 2, 3], any)), "{ff:?}");
    }

    #[test]
    fn a_plan_is_scored_with_what_its_hand_holds_then() {
        // A flush plan's completion is played beside the 3 cards the hand holds then, drawn
        // ones here (the dig throws every other card): Raised Fist reads the lowest of them, and
        // Blackboard's ×3 needs every one a Spade or Club. Scored with the cards kept beside it
        // alone (none), the Fist read nothing and Blackboard always paid.
        let hand = Card::parse_list("2H 5H 9H JH KS 3C 4D 6C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
        let mut pile = deck.clone();
        pile.sort_by(|p, q| p.order_cmp(q));
        let ends = Some(Ends { pile: &pile, size: 8 });
        let flush = |b: &Board| aims(b, &hand, &deck, false).into_iter().find(|a| a.hand == HandType::Flush && a.keep == [0, 1, 2, 3]).unwrap();
        let fist = sample_board(&["j_raised_fist"]);
        let a = flush(&fist);
        assert!(aim_score(&fist, &a, &hand, &[], ends, None, false) > aim_score(&fist, &a, &hand, &[], None, None, false));
        let black = sample_board(&["j_blackboard"]);
        let a = flush(&black);
        assert!(aim_score(&black, &a, &hand, &[], ends, None, false) < aim_score(&black, &a, &hand, &[], None, None, false));
        // a completion that can't be made beside the cards held back (2 Hearts kept, 4 Steel
        // cards worth holding, 2 slots a dig refills, 3 Hearts missing) has no odds, and what it
        // holds beside it never takes the hand past its size, the first cards held back first
        let hand = Card::parse_list("2H 5H KS:steel KC:steel KD:steel QS:steel 3C 4D").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
        let mut pile = deck.clone();
        pile.sort_by(|p, q| p.order_cmp(q));
        let ends = Some(Ends { pile: &pile, size: 8 });
        let b = sample_board(&[]);
        let a = aims(&b, &hand, &deck, false).into_iter().find(|a| a.hand == HandType::Flush && a.keep == [0, 1]).unwrap();
        let kept: Vec<Card> = hand[2..].to_vec();
        for (cards, _, rest) in aim_completions_held(&b, &a, &hand) {
            let held = held_beside(&a, &hand, &cards, &rest, &kept, ends, 0);
            assert_eq!(cards.len() + held.len(), 8, "{cards:?} {held:?}");
            assert_eq!(held[..3], hand[2..5]);
        }
    }

    #[test]
    fn held_draws_are_shared_and_a_plan_skips_only_its_fits() {
        let pile: Vec<Card> = standard_deck();
        for s in 0..4 {
            let all = held_draws(&pile, &[], 6, s);
            // a draw of fewer cards is the start of one of more
            assert_eq!(held_draws(&pile, &[], 3, s), all[..3]);
            // a plan whose fit falls among them draws the next card in its place, the rest the same
            let fit = all[2];
            let mut without: Vec<Card> = all.clone();
            without.remove(2);
            assert_eq!(held_draws(&pile, &[fit], 5, s), without);
        }
    }

    #[test]
    fn a_completion_of_more_than_five_cards_keeps_the_rest_in_hand() {
        // Three Kings and two singles kept for a Full House: a completion is 6 cards, its best 5
        // played and the other single held
        let hand = Card::parse_list("KS KH KD 5C 9D 2H 3S 4C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
        let b = sample_board(&[]);
        let a = aims(&b, &hand, &deck, false).into_iter().find(|a| a.hand == HandType::FullHouse && a.keep.len() == 5).unwrap();
        let kept: Vec<Card> = a.keep.iter().map(|&i| hand[i]).collect();
        for (cards, _, rest) in aim_completions_held(&b, &a, &hand) {
            assert_eq!((cards.len(), rest.len()), (5, 1), "{cards:?} {rest:?}");
            assert!(kept.iter().all(|k| cards.contains(k) || rest.contains(k)));
            assert!(!cards.contains(&rest[0]));
        }
    }

    #[test]
    fn a_chase_is_worked_out_dig_by_dig_against_playing_on() {
        // a plan whose fits are `fits` groups of `n` cards in a 44-card pile, 4 cards kept of 8
        let aim = |groups: &[usize]| Aim {
            family: None,
            hand: HandType::StraightFlush,
            keep: vec![0, 1, 2, 3],
            groups: groups.iter().map(|&n| (standard_deck()[..n].to_vec(), 1)).collect(),
        };
        let round = |best: f64, need: f64, discards: i64| ChaseRound { pile: 44, size: 8, hands: 4, discards, need, best, later: best, on_pace: best * 4.0 >= need, takes: 0, stays: 0, lift: 1.0, spent: 1.0, last: None, now: 0.0 };
        // one card away, 5 fits: one dig makes it 39% of the time, too little on its own to
        // beat playing on, but the policy digs again after a miss, so six digs make it nearly
        // always: worth chasing (a chase decided on one dig's odds would never start)
        let one = aim(&[5]);
        assert!((aim_odds(&one, 44, 8, 1) - 0.394).abs() < 0.01);
        let c = chase(&one, 8652.0, 74.0, &[], &round(3312.0, 21360.0, 3));
        assert!(c.go && c.value > 4.0 * 3312.0, "{}", c.value);
        // with no discard every dig is a junk hand, a hand the board doesn't grow on: chased
        // when a later hand is worth what this one is, not when the board grows (a later hand
        // played on worth 2.5 times this one)
        let junk = |later: f64| chase(&one, 20000.0, 74.0, &[], &ChaseRound { later, ..round(3312.0, 21360.0, 0) }).go;
        assert!(junk(3312.0) && !junk(2.5 * 3312.0));
        // on the round's last hand, on pace (its best play reaches the score needed on half its
        // rolls): three digs make it about 78% of the time, which beats playing now's 50%; the
        // on-pace margin, half a best play in points, isn't asked there
        let last = ChaseRound { hands: 1, on_pace: true, last: Some(1000.0), now: 500.0, ..round(1200.0, 1000.0, 3) };
        let c = chase(&one, 1000.0, 0.0, &[], &last);
        assert!(c.go && c.value > 700.0 && c.value < 500.0 + ON_PACE_MARGIN * 1200.0, "{}", c.value);
        // a junk hand is a played hand: The Hook then discards 2 of the 4 cards held, the plan's
        // own, so with no discard it isn't chased; with discards, the digs after them count less
        let hook = |discards: i64| chase(&one, 20000.0, 74.0, &[], &ChaseRound { takes: 2, ..round(3312.0, 21360.0, discards) });
        assert!(!hook(0).go, "{}", hook(0).value);
        // with 2 cards kept for their held value the junk hand plays 2, so 6 are held and the
        // plan's 4 are spared 1 time in 15 (and a dig draws 2 cards, not 4: they take its slots)
        let kept = chase(&one, 20000.0, 74.0, &[], &ChaseRound { takes: 2, stays: 2, ..round(3312.0, 21360.0, 0) });
        assert!(kept.value > hook(0).value, "{} {}", kept.value, hook(0).value);
        let plain = chase(&one, 20000.0, 74.0, &[], &round(3312.0, 21360.0, 3));
        assert!(hook(3).go && hook(3).value < plain.value, "{} {}", hook(3).value, plain.value);
        // cards held back for their held value take slots a dig would refill: with 2 of them a
        // dig draws 2 cards, not 4, and the plan is worth less (without The Hook too)
        let held_back = chase(&one, 20000.0, 74.0, &[], &ChaseRound { stays: 2, ..round(3312.0, 21360.0, 3) });
        assert!(held_back.value < plain.value, "{} {}", held_back.value, plain.value);
        // two cards away, 3 fits each, with 1 discard: the rule before took every dig left
        // (the discard and 3 junk hands) as spent on it (56%, × score × 2 against the best × 4)
        // and chased it; dig by dig, each junk hand costs a hand and a failed chase leaves only
        // what it kept (here High Card), so it isn't worth more than playing on
        let two = aim(&[3, 3]);
        let p = aim_odds(&two, 44, 8, 4);
        // 4 cards kept and 2 missing need 6 slots: with 3 cards held back beside them (5 slots) it
        // can't be made, however many digs
        assert_eq!(aim_odds(&two, 44, 5, 4), 0.0);
        assert!(aim_odds(&two, 44, 6, 4) > 0.0);
        assert!(p * 1000.0 * 2.0 > 180.0 * 4.0, "{p}");
        let c = chase(&two, 1000.0, 20.0, &[], &round(180.0, 876.0, 1));
        assert!(!c.go && c.value < 180.0 * 4.0, "{}", c.value);
        // the lesser hand its cards make on the way (a Flush on pace) is where it often ends,
        // and is valued there
        let flush = Aim { family: None, hand: HandType::Flush, keep: vec![0, 1, 2, 3], groups: vec![(standard_deck()[10..19].to_vec(), 1)] };
        let with = chase(&two, 1000.0, 20.0, &[(&flush, 300.0)], &round(180.0, 876.0, 1));
        assert!(with.value > c.value, "{} {}", with.value, c.value);
        // a later hand played on after the first dig, on the board its discard leaves (`lift`):
        // one that lowers it (Green Joker's Mult) makes the one-away chase worth less, one that
        // raises it (Hit the Road's Jack) the two-away one worth more; playing on now stays
        let lifted = |a: &Aim, lift: f64, sc: f64, left: f64, r: ChaseRound| chase(a, sc, left, &[], &ChaseRound { lift, ..r }).value;
        let base = round(3312.0, 21360.0, 3);
        assert!(lifted(&one, 0.5, 8652.0, 74.0, base) < lifted(&one, 1.0, 8652.0, 74.0, round(3312.0, 21360.0, 3)));
        assert!(lifted(&two, 2.0, 1000.0, 20.0, round(180.0, 876.0, 1)) > c.value);
        // a discard's count (Banner's Chips) on both sides: a chase a dig's ×0.8 stops goes
        // when playing on is charged the same discard (`spent`)
        let counted = |spent: f64| chase(&one, 0.8 * 5200.0, 74.0, &[], &ChaseRound { lift: 0.8, spent, ..round(3312.0, 21360.0, 3) }).go;
        assert!(!counted(1.0) && counted(0.8));
    }

    #[test]
    fn a_dig_throws_first_the_cards_whose_discard_lifts_the_hands_after_it() {
        // an effect every card has alike (Green Joker's Mult) leaves the lowest first: a pair of
        // 3s among the six thrown from is thrown as on a plain board
        let pairs = Card::parse_list("KH KS 3C 3D 4S 6C 7D 9S").unwrap();
        let mut green = sample_board(&["j_green_joker"]);
        green.jokers[0].mult = 5.0;
        let toss = |b: &Board| dig_for(&HandParts::new(b, &pairs), &[0, 1], &[], 1).unwrap().0;
        assert_eq!(toss(&green), toss(&sample_board(&["j_joker"])));
        // beside a joker whose effect depends on the card (Hit the Road), Green Joker's still
        // cancels: the same cards as with Hit the Road alone
        let mut both = sample_board(&["j_green_joker", "j_hit_the_road"]);
        both.jokers[0].mult = 5.0;
        let jacks = Card::parse_list("KH KS JC 3D 4S 6C 7D 9S").unwrap();
        let toss = |b: &Board| dig_for(&HandParts::new(b, &jacks), &[0, 1], &[], 1).unwrap().0;
        assert_eq!(toss(&both)[0], 2);
        assert_eq!(toss(&both), toss(&sample_board(&["j_hit_the_road"])));
        // Hit the Road: ×0.5 Mult for each Jack discarded, for the rest of the round (card.lua
        // `Card:calculate_joker`, the `discard` context). Six cards outside the Hearts kept, five
        // thrown: the Jack before the lower cards with it; without it, the lowest five
        let hand = Card::parse_list("2H 5H 9H JS 3C 4D 6S 7C").unwrap();
        let dig = |b: &Board| dig_for(&HandParts::new(b, &hand), &[0, 1], &[], 1).unwrap().0;
        assert_eq!(dig(&sample_board(&["j_hit_the_road"]))[0], 3);
        assert!(!dig(&sample_board(&["j_joker"])).contains(&3));
        // as a junk hand (no discard left) nothing is discarded: the same cards as without it
        let junk = |b: &Board| dig_for(&HandParts::new(b, &hand), &[0, 1], &[], 0).unwrap().0;
        assert_eq!(junk(&sample_board(&["j_hit_the_road"])), junk(&sample_board(&["j_joker"])));
    }

    #[test]
    fn a_dig_is_scored_on_the_board_its_cards_leave() {
        let hand = Card::parse_list("KH KS JS 3C 4D").unwrap();
        let kings = [0, 1];
        let lift = |b: &Board, thrown: &[usize]| dig_board(b, &hand, thrown, &kings).map_or(1.0, |a| ratio(b, &a, &picked(&hand, &kings), &picked(&hand, &[2, 3, 4])));
        // Hit the Road: a Jack thrown, the Pair of Kings after it at ×1.5; no Jack, no board
        let road = sample_board(&["j_hit_the_road"]);
        assert!((lift(&road, &[2]) - 1.5).abs() < 0.01, "{}", lift(&road, &[2]));
        assert!(dig_board(&road, &hand, &[3, 4], &kings).is_none());
        // Green Joker loses a Mult for each discard, whatever is thrown
        let mut green = sample_board(&["j_green_joker"]);
        green.jokers[0].mult = 5.0;
        assert!(lift(&green, &[3]) < 1.0, "{}", lift(&green, &[3]));
        // the count of discards left is the engine's step too: with Banner (Chips for each
        // discard left) beside Hit the Road, a dig without a Jack leaves a board one discard
        // lower, and one with a Jack the Jack's lift and the count's
        let mut banner = sample_board(&["j_banner", "j_hit_the_road"]);
        banner.discards_left = 3;
        let after = dig_board(&banner, &hand, &[3, 4], &kings).unwrap();
        assert_eq!((after.discards_left, after.discards_used), (2, banner.discards_used + 1));
        let count = lift(&banner, &[3, 4]);
        assert!(count < 1.0, "{count}");
        assert!((lift(&banner, &[2]) - 1.5 * count).abs() < 0.01, "{}", lift(&banner, &[2]));
        // Burnt Joker levels the first discard's hand: a board of its own
        assert!(dig_board(&sample_board(&["j_burnt"]), &hand, &[3], &kings).is_some());
        // a joker no discard changes
        assert!(dig_board(&sample_board(&["j_joker"]), &hand, &[2], &kings).is_none());
    }

    #[test]
    fn playing_on_is_charged_the_discard_the_policy_makes_instead() {
        // `spent_by`: the policy's fallback discard on the engine's whole step, as a dig's is
        // charged to its chase. Nothing to throw: it plays and keeps its discards
        let hand = Card::parse_list("KH KS JS 3C 4D").unwrap();
        let kings = [0, 1];
        let mut banner = sample_board(&["j_banner"]);
        banner.discards_left = 3;
        assert_eq!(spent_by(&banner, &hand, &[], &kings), 1.0);
        // Banner's Chips for the discard it spends; Green Joker's Mult; Hit the Road's Jack
        // thrown lifts the hands after it; a joker no discard changes, nothing
        assert!(spent_by(&banner, &hand, &[3, 4], &kings) < 1.0);
        let mut green = sample_board(&["j_green_joker"]);
        green.jokers[0].mult = 5.0;
        assert!(spent_by(&green, &hand, &[3, 4], &kings) < 1.0);
        assert!((spent_by(&sample_board(&["j_hit_the_road"]), &hand, &[2, 3], &kings) - 1.5).abs() < 0.01);
        assert_eq!(spent_by(&sample_board(&["j_joker"]), &hand, &[2, 3, 4], &kings), 1.0);
        // the discard charged is the policy's own fallback: the cards outside the best play and
        // those worth holding, lowest first; with every other card worth holding, none
        let parts = HandParts::new(&banner, &hand);
        assert_eq!(fallback_toss(&parts, &kings, &[]), vec![3, 4, 2]);
        assert_eq!(fallback_toss(&parts, &kings, &[4]), vec![3, 2]);
        assert!(fallback_toss(&parts, &kings, &[2, 3, 4]).is_empty());
    }

    #[test]
    fn a_dig_leaves_the_cards_worth_holding() {
        // Baron: the Kings score while held, so a dig for the Hearts never throws them, with a
        // discard or as a junk hand, and the hand a failed chase is left with still has them
        let b = sample_board(&["j_baron"]);
        let hand = Card::parse_list("KS KC 2H 5H 9H 3C 4D 8S").unwrap();
        let hearts = [2, 4, 5];
        let held = held_value(&b, &hand, &[], &[]);
        assert!(held.contains(&0) && held.contains(&1), "{held:?}");
        for discards in [1, 0] {
            let (dig, junk) = dig_for(&HandParts::new(&b, &hand), &hearts, &held, discards).unwrap();
            assert_eq!(junk, discards == 0);
            assert!(!dig.contains(&0) && !dig.contains(&1) && dig.iter().all(|i| !hearts.contains(i)), "{dig:?}");
        }
        // with nothing but cards worth holding outside it, there's no dig
        assert!(dig_for(&HandParts::new(&b, &hand[..2]), &[], &[0, 1], 1).is_none());
    }

    #[test]
    fn a_floor_that_rolls_nothing_is_the_average() {
        // `best_play` takes a floor that asked for no roll as its average: the same as
        // `mean_score`, on boards with random jokers and hands with Lucky cards, and without
        let mut rng = Rng::new(11);
        let boards: [&[&str]; 4] = [&[], &["j_misprint"], &["j_bloodstone", "j_jolly"], &["j_baron", "j_joker"]];
        for keys in boards {
            let b = sample_board(keys);
            for _ in 0..150 {
                let mut deck = standard_deck();
                shuffle(&mut deck, &mut rng);
                let mut hand: Vec<Card> = deck[..8].to_vec();
                if rng.below(2) == 0 {
                    hand[rng.below(8)].enhancement = Some(Enhancement::Lucky);
                }
                let p = best_play(&b, &hand).unwrap();
                assert_eq!(p.mean, mean_score(&b, &hand, &p.cards, p.floor), "{keys:?} {hand:?}");
            }
        }
    }

    #[test]
    fn the_best_plays_of_a_hands_parts_are_best_play_of_their_cards() {
        // `HandParts` shares hand detection, and floors no held card takes part in, between the
        // parts of a hand and remembers each part: the same best play, average included, as
        // `best_play` of the part's cards, on random hands and parts (the whole hand first or
        // not), Stone, Wild and Steel cards among them, and on boards where held cards and
        // random effects score; on hands of 6 to 10 cards and of 13 to 17 (past `SUBSETS_MAX`,
        // each part's own search remembered), and again once what it found is handed on
        // (`HandParts::with`)
        let mut rng = Rng::new(9);
        let boards: [&[&str]; 8] = [
            &[],
            &["j_baron", "j_raised_fist"],
            &["j_four_fingers", "j_shortcut"],
            &["j_misprint", "j_blackboard"],
            &["j_smeared", "j_half"],
            &["j_reserved_parking", "j_mime"],
            &["j_shoot_the_moon", "j_blackboard"],
            &["j_bloodstone", "j_jolly"],
        ];
        for keys in boards {
            let b = sample_board(keys);
            for t in 0..116 {
                let mut deck = standard_deck();
                shuffle(&mut deck, &mut rng);
                let mut hand: Vec<Card> = deck[..if t < 100 { 6 + t % 5 } else { 13 + t % 5 }].to_vec();
                if rng.below(3) == 0 {
                    hand[rng.below(6)].enhancement = Some(Enhancement::Stone);
                }
                if rng.below(3) == 0 {
                    hand[rng.below(6)].enhancement = Some(Enhancement::Wild);
                }
                if rng.below(3) == 0 {
                    hand[rng.below(6)].enhancement = Some(Enhancement::Steel);
                }
                let parts = HandParts::new(&b, &hand);
                if t % 2 == 0 {
                    let whole: Vec<usize> = (0..hand.len()).collect();
                    assert_eq!(parts.best(&whole).map(|p| (p.cards, p.floor, p.mean)), best_play(&b, &hand).map(|p| (p.cards, p.floor, p.mean)));
                    // the whole hand's plays as searched are the plays `best_play` tries, up to
                    // `SHARE_MAX` cards; the shortlist from them only up to `PARTS_MAX`
                    if hand.len() <= SHARE_MAX {
                        assert!(parts.plays() == plays_tried(&b, &hand), "{keys:?} {hand:?}");
                    }
                    assert_eq!(parts.shortlist().is_some(), hand.len() <= PARTS_MAX && front_holds(&b, &hand));
                }
                let mut asked: Vec<Vec<usize>> = vec![];
                let check = |parts: &HandParts, idx: &[usize]| {
                    let cards: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
                    let want = best_play(&b, &cards).map(|p| (p.cards.iter().map(|&k| idx[k]).collect::<Vec<_>>(), p.hand, p.floor, p.mean));
                    let got = parts.best(idx).map(|p| (p.cards, p.hand, p.floor, p.mean));
                    assert_eq!(got, want, "{keys:?} {hand:?} {idx:?}");
                };
                for k in 0..12 {
                    // (some asked twice: what a part found is remembered)
                    let idx: Vec<usize> = if k % 4 == 3 {
                        asked[rng.below(asked.len())].clone()
                    } else {
                        let m = rng.below(1 << hand.len()) as u32 | 1;
                        (0..hand.len()).filter(|i| m & (1 << i) != 0).collect()
                    };
                    check(&parts, &idx);
                    asked.push(idx);
                }
                // the whole hand's plays as `best_play` tries them, its parts searched first or not
                let whole: Vec<usize> = (0..hand.len()).collect();
                check(&parts, &whole);
                if hand.len() <= SHARE_MAX {
                    assert!(parts.plays() == plays_tried(&b, &hand), "{keys:?} {hand:?}");
                }
                // handed on: the parts asked before and new ones, and the whole hand's plays; to
                // another hand, nothing
                let again = HandParts::with(&b, &hand, parts.searched());
                for k in 0..4 {
                    let idx = if k % 2 == 0 { asked[rng.below(asked.len())].clone() } else { (0..hand.len()).filter(|i| (rng.below(2) == 0) || *i == 0).collect() };
                    check(&again, &idx);
                }
                assert!(again.plays() == plays_tried(&b, &hand), "{keys:?} {hand:?}");
                let mut other = hand.clone();
                other[0] = Card { suit: if other[0].suit == Suit::Spades { Suit::Hearts } else { Suit::Spades }, ..other[0] };
                let elsewhere = HandParts::with(&b, &other, again.searched());
                let cards: Vec<Card> = whole.iter().map(|&i| other[i]).collect();
                assert_eq!(elsewhere.best(&whole).map(|p| (p.cards, p.floor, p.mean)), best_play(&b, &cards).map(|p| (p.cards, p.floor, p.mean)), "{keys:?} {other:?}");
            }
        }
    }

    #[test]
    fn the_oracle_keeps_sets_by_the_games_hand_rules() {
        // the cards each oracle discard keeps
        let kept = |cards: &str, keys: &[&str]| -> Vec<Vec<String>> {
            let hand = Card::parse_list(cards).unwrap();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
            oracle_moves(&sample_board(keys), &hand, &deck, 3)
                .into_iter()
                .filter_map(|m| match m {
                    Move::Discard(v) => Some((0..hand.len()).filter(|i| !v.contains(i)).map(|i| hand[i].label()).collect()),
                    Move::Play(_) => None,
                })
                .collect()
        };
        let has = |v: &[Vec<String>], want: &[&str]| v.iter().any(|k| k.len() == want.len() && want.iter().all(|w| k.contains(&w.to_string())));
        // Smeared Joker: Hearts and Diamonds are one suit; a Wild card is every suit
        let smeared = kept("5H 9D KH 2S 7C JC 4S 8S:wild", &["j_smeared"]);
        assert!(has(&smeared, &["5♥", "9♦", "K♥", "8♠ [wild]"]), "{smeared:?}");
        assert!(!has(&kept("5H 9D KH 2S 7C JC 4S 8S:wild", &[]), &["5♥", "9♦", "K♥", "8♠ [wild]"]));
        // Four Fingers: a run of 4 you hold 2 of; Shortcut: a run with a gap
        let ff = kept("5H 6S KD QC 2C 9D 9S 3H", &["j_four_fingers"]);
        assert!(has(&ff, &["2♣", "3♥", "5♥", "6♠"]) || has(&ff, &["3♥", "5♥", "6♠"]), "{ff:?}");
        let sc = kept("2H 5S 8D KC KD QS QC 3C", &["j_shortcut"]);
        assert!(has(&sc, &["2♥", "5♠", "8♦"]), "{sc:?}");
        assert!(!has(&kept("2H 5S 8D KC KD QS QC 3C", &[]), &["2♥", "5♠", "8♦"]));
    }

    #[test]
    fn discards_that_pay_are_cashed_while_safe() {
        // Money for discards comes from the engine (`discard_money`), the policy names no
        // joker: Mail-In pays per card of its rank, Faceless Joker for 3+ faces at once.
        // no card of a rank in hand left to draw: the Pair of Aces has no Three of a Kind or
        // Two Pair to dig for, so only the money decides
        let decide_on = |keys: &[&str], hand: &str, discards: i64, need_mult: f64| {
            let hand = Card::parse_list(hand).unwrap();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank)).collect();
            let mut b = sample_board(keys);
            b.mail_rank = Some(4);
            b.discards_left = discards;
            b.hands_left = 3;
            let pair = best_play(&b, &hand).unwrap().floor;
            let act = decide(&b, &hand, &deck, 3, discards, pair * need_mult, 8);
            match act {
                Action::Discard(v) => Some(v.iter().map(|&i| hand[i].label()).collect::<Vec<_>>()),
                Action::Play(..) => None,
            }
        };
        // safe (the Pair of Aces twice beats the target): cash the 4 the play doesn't need
        assert_eq!(decide_on(&["j_mail"], "AS AH 4C 9D 8S 7H 3D 2C", 2, 2.0), Some(vec!["4♣".to_string()]));
        // behind (the Pair can't get there): discards are for digging, not cashing the 4 alone
        assert_ne!(decide_on(&["j_mail"], "AS AH 4C 9D 8S 7H 3D 2C", 2, 10.0), Some(vec!["4♣".to_string()]));
        // the last discard waits for the hand that wins
        assert_eq!(decide_on(&["j_mail"], "AS AH 4C 9D 8S 7H 3D 2C", 1, 2.0), None);
        // Faceless Joker: three faces at once pay $5
        let faces = decide_on(&["j_faceless"], "AS AH KC QD JS 7H 3D 2C", 2, 2.0).expect("discard three faces");
        assert_eq!(faces.len(), 3);
        assert!(faces.iter().all(|l| l.starts_with('K') || l.starts_with('Q') || l.starts_with('J')), "{faces:?}");
        // the last discard is cashed when the play wins now
        assert_eq!(decide_on(&["j_mail"], "AS AH 4C 9D 8S 7H 3D 2C", 1, 1.0), Some(vec!["4♣".to_string()]));
        // a plain joker: no discard pays, play the Pair
        assert_eq!(decide_on(&["j_joker"], "AS AH 4C 9D 8S 7H 3D 2C", 2, 2.0), None);
    }

    #[test]
    fn a_smaller_paying_discard_is_tried_when_the_biggest_breaks_the_pace() {
        // Mail-In on Kings, three Kings in hand: cashing all three ($15) or two ($10) leaves
        // no Pair; cashing one ($5) keeps the Pair of Kings, which keeps pace. Discards padded
        // with cards that pay nothing must not crowd out the smaller one.
        let hand = Card::parse_list("KS KH KD 9C 7D 3S 2C 5H").unwrap();
        let deck = standard_deck();
        let mut b = sample_board(&["j_mail"]);
        b.mail_rank = Some(13);
        b.discards_left = 2;
        b.hands_left = 3;
        let pair = best_play(&b, &Card::parse_list("KS KH 9C 7D 3S 2C 5H").unwrap()).unwrap().floor;
        match decide(&b, &hand, &deck, 3, 2, pair * 3.0, 8) {
            Action::Discard(v) => assert_eq!(v.iter().filter(|&&i| hand[i].rank.0 == 13).count(), 1, "cash exactly one King: {:?}", v.iter().map(|&i| hand[i].label()).collect::<Vec<_>>()),
            other => panic!("expected a discard, got {other:?}"),
        }
    }

    #[test]
    fn a_paying_card_in_the_play_is_cashed_only_if_the_rest_keeps_pace() {
        // Mail-In on Aces, Two Pair (Aces and Kings) in hand: discarding the Aces pays $10 and
        // leaves a Pair of Kings. Cash them only when the Kings alone still keep you on pace.
        let hand = Card::parse_list("AS AH KS KH 9C 7D 3S 2C").unwrap();
        let deck = standard_deck();
        let mut b = sample_board(&["j_mail"]);
        b.mail_rank = Some(14);
        b.discards_left = 2;
        b.hands_left = 3;
        let two_pair = best_play(&b, &hand).unwrap().floor;
        let kings = best_play(&b, &hand[2..]).unwrap().floor;
        let aces = |v: &[usize]| v.iter().any(|&i| hand[i].rank.0 == 14);
        match decide(&b, &hand, &deck, 3, 2, kings * 3.0, 8) {
            Action::Discard(v) => assert!(aces(&v), "the Kings keep pace: cash the Aces"),
            other => panic!("expected a discard, got {other:?}"),
        }
        let need = (kings * 3.0 + two_pair * 3.0) / 2.0;
        if let Action::Discard(v) = decide(&b, &hand, &deck, 3, 2, need, 8) {
            assert!(!aces(&v), "the Kings alone don't keep pace: keep the Aces");
        }
    }

    #[test]
    fn drawing_a_card_worth_having_adds_its_gain_to_the_round() {
        // A held consumable would be worth 0.5 more on the 3♠ with a Blue Seal (e.g. Cryptid's
        // copies): a simulated round that draws it is worth that much more; one that doesn't,
        // nothing extra. No rule names the consumable or the card.
        let seal = Card::parse_list("3S:blue").unwrap()[0];
        let hand = Card::parse_list("AS AH KD 9C 7S 5H 4D 2C").unwrap();
        let mut deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit) && !(c.rank == seal.rank && c.suit == seal.suit)).collect();
        deck.push(seal);
        let mut b = sample_board(&["j_joker"]);
        b.goals = Some(RoundGoals { seen: vec![("c_cryptid".into(), seal, 0.5)], ..Default::default() });
        let start = RoundStart { hand: hand.clone(), deck, hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1.0, fresh: false, ..Default::default() };
        let outs = outcomes_after(&b, &start, &Move::Discard(vec![4, 5, 6, 7]), 0..400, 7, &[]);
        let g = b.goals.clone().unwrap();
        let drew = outs.iter().filter(|o| o.seen > 0.0).count();
        assert!(drew > 0 && drew < outs.len(), "some rounds draw it, some don't: {drew}");
        for o in &outs {
            assert!(o.seen == 0.0 || o.seen == 0.5);
            assert_eq!(g.value(o), o.won * (1.0 + o.seen));
        }
    }

    #[test]
    fn a_round_counts_what_its_hands_leave_the_jokers() {
        // Square Joker grows +4 Chips on a hand of exactly 4 cards (card.lua, before context),
        // for the run. Two Pair wins the round either way: with a fifth card that doesn't score
        // it's worth the same now, and 4 Chips' worth less (0.01 a Chip here). No rule names
        // the joker: what one more Chip is worth is the round's goal (`RoundGoals::jokers`).
        let hand = Card::parse_list("AS AH 10S 10H KD 8C 7S 5C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit)).collect();
        let start = RoundStart { hand, deck, hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1.0, fresh: false, ..Default::default() };
        let mut b = sample_board(&["j_square"]);
        b.goals = Some(worths(&mut b, &[([0.0, 0.0, 0.01], -1.0)]));
        let g = b.goals.clone().unwrap();
        let mean = |m: Move| {
            let outs = outcomes_after(&b, &start, &m, 0..20, 7, &[]);
            assert!(outs.iter().all(|o| o.won == 1.0));
            outs.iter().map(|o| g.value(o)).sum::<f64>() / outs.len() as f64
        };
        let (four, five) = (mean(Move::Play(vec![0, 1, 2, 3])), mean(Move::Play(vec![0, 1, 2, 3, 4])));
        assert!((four - five - 0.04).abs() < 1e-9, "{four} vs {five}");
        // nothing counted without the goal: the same value
        b.goals = Some(RoundGoals::default());
        let plain = |m: Move| outcomes_after(&b, &start, &m, 0..20, 7, &[]).iter().map(|o| o.grown.long.abs() + o.grown.ahead.abs()).sum::<f64>();
        assert_eq!((plain(Move::Play(vec![0, 1, 2, 3])), plain(Move::Play(vec![0, 1, 2, 3, 4]))), (0.0, 0.0));
    }

    #[test]
    fn the_finish_is_the_winning_play_worth_most() {
        // A Flush and Two Pair both win the round now. Scored, the Flush is the best play;
        // valued, Two Pair's 4 cards grow Square Joker (+4 Chips for the run, 0.01 a Chip here),
        // so the finish is a 4-card play. No rule names the joker: the round's goals decide.
        let hand = Card::parse_list("AS KS QS JS 9S AH KH 2C").unwrap();
        let mut b = sample_board(&["j_square"]);
        assert_eq!(best_play(&b, &hand).map(|p| p.hand), Some(HandType::Flush));
        let g = worths(&mut b, &[([0.0, 0.0, 0.01], -1.0)]);
        assert_eq!(finish_play(&b, &g, &hand, 1.0, 3).map(|v| v.len()), Some(4));
        // nothing growing: the best-scoring win
        assert_eq!(finish_play(&b, &RoundGoals::default(), &hand, 1.0, 3).map(|v| v.len()), Some(5));
        // nothing wins: no finish
        assert_eq!(finish_play(&b, &g, &hand, 1e9, 3), None);
    }

    #[test]
    fn playing_on_grows_the_jokers_when_the_growth_pays_for_the_hand() {
        // Only the Flush wins: a 4-card play that doesn't win yet grows Square Joker. It's the
        // play to look ahead at when 4 Chips are worth more than a spare hand's money by the
        // look-ahead's margin, and none when they aren't.
        let hand = Card::parse_list("AS KS QS JS 9S AH KH 2C").unwrap();
        let mut b = sample_board(&["j_square"]);
        let flush = best_play(&b, &hand).unwrap();
        assert_eq!(flush.hand, HandType::Flush);
        let need = flush.floor - 1.0;
        let mut g = worths(&mut b, &[([0.0, 0.0, 0.01], -1.0)]);
        let win = finish_play(&b, &g, &hand, need, 3).unwrap();
        assert_eq!(win.len(), 5);
        let grow = growth_play(&b, &g, &hand, need, &win, 0.0).expect("a growing play");
        assert_eq!(grow.len(), 4);
        // no 4-card play leaves the Flush whole: the highest-scoring one, Two Pair
        let cards: Vec<Card> = grow.iter().map(|&i| hand[i]).collect();
        assert_eq!(score::score(&b, &cards, &[], &mut Unlucky, false).hand, HandType::TwoPair);
        // a spare hand worth as much as the growth: no look-ahead for it
        g.dollar = 0.01;
        g.per_hand = 4.0;
        assert_eq!(growth_play(&b, &g, &hand, need, &win, 0.0), None);
        // unless winning now takes something off the jokers (a reset) that playing on spares
        assert!(growth_play(&b, &g, &hand, need, &win, 0.02).is_some());
        // nor without growing jokers
        assert_eq!(growth_play(&b, &RoundGoals::default(), &hand, need, &win, 0.0), None);
    }

    #[test]
    fn the_finish_keeps_a_blue_seal_when_a_win_without_it_is_worth_more() {
        // Any play wins; the 3♠ Blue Seal's planet (the last hand's) counts only while it's
        // held at the end, so the finish leaves it in hand. With the planet worth nothing, the
        // best-scoring win, which plays it in the Flush.
        let hand = Card::parse_list("AS KS QS JS 3S:blue AH 2C").unwrap();
        let mut b = sample_board(&[]);
        b.planet_slots = 1;
        let seal = hand.iter().position(|c| c.seal.is_some()).unwrap();
        let g = RoundGoals { planet: [0.05; 12], ..Default::default() };
        let v = finish_play(&b, &g, &hand, 1.0, 3).unwrap();
        assert!(!v.contains(&seal), "{v:?}");
        let v = finish_play(&b, &RoundGoals::default(), &hand, 1.0, 3).unwrap();
        assert!(v.contains(&seal), "{v:?}");
    }

    #[test]
    fn the_growth_play_leaves_the_winning_cards_in_hand_when_one_grows_as_much() {
        // Every 4-card play grows Square Joker alike; one of them (the four low off-suit cards, a
        // 5-high High Card) leaves the Flush that wins in hand, so it's the one to look ahead at,
        // though a 4-card play with the Ace scores more.
        let hand = Card::parse_list("AS KS QS JS 9S 2H 5D 3C 4C").unwrap();
        let mut b = sample_board(&["j_square"]);
        let need = best_play(&b, &hand).unwrap().floor - 1.0;
        let g = worths(&mut b, &[([0.0, 0.0, 0.01], -1.0)]);
        let win = finish_play(&b, &g, &hand, need, 3).unwrap();
        let grow = growth_play(&b, &g, &hand, need, &win, 0.0).unwrap();
        assert_eq!(grow.len(), 4);
        let low: Vec<Card> = grow.iter().map(|&i| hand[i]).collect();
        let ace = Card::parse_list("AS 2H 5D 3C").unwrap();
        assert!(score::score(&b, &ace, &[], &mut Unlucky, false).score > score::score(&b, &low, &[], &mut Unlucky, false).score);
        assert!(grow.iter().all(|i| !win.contains(i)), "{grow:?} against {win:?}");
    }

    #[test]
    fn growth_over_rounds_is_measured_and_faders_only_fade() {
        // Green Joker (+1 a hand, −1 a discard) at +10 over 6 rounds at targets out of reach
        // (every round plays all its hands) grows, and from a start 5 higher ends 5 higher; Ice Cream (−5 Chips a hand),
        // Ramen (−×0.01 a discarded card) and Seltzer (10 hands) only fade or run out; the plain
        // Joker isn't one a round changes, nor Vampire (it takes from the cards it grows on)
        let mut b = sample_board(&["j_green_joker", "j_ice_cream", "j_ramen", "j_selzer", "j_joker", "j_vampire"]);
        for (i, j) in b.jokers.iter_mut().enumerate() {
            j.id = i as u32 + 1;
        }
        b.jokers[0].mult = 10.0;
        let start = |target| RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target, fresh: true, ..Default::default() };
        let starts = [start(20000.0), start(30000.0), start(40000.0)];
        let g = grown_over(&b, &starts, &[3, 6], 16, 1);
        assert!(g[4].is_none() && g[5].is_none());
        let green = g[0].as_ref().unwrap();
        assert_eq!((green.key.as_str(), green.start[0], green.at.len()), ("j_green_joker", 10.0, 2));
        assert!(green.at[1][0] > green.at[0][0] && green.at[0][0] > 10.0, "{:?}", green.at);
        let up = green.numbers(1, [15.0, 1.0, 0.0]).unwrap();
        assert!((up[0] - green.at[1][0] - 5.0).abs() < 1e-9);
        for f in &g[1..4] {
            let f = f.as_ref().unwrap();
            for k in 0..2 {
                assert!((0..3).all(|n| f.at[k][n] <= f.start[n] + 1e-9), "{} {:?}", f.key, f.at);
            }
            assert!(f.gone[1] >= f.gone[0], "{} {:?}", f.key, f.gone);
        }
        // Seltzer's 10 hands are gone in 6 rounds of 4 hands
        assert!(g[3].as_ref().unwrap().gone[1] == 1.0 && g[3].as_ref().unwrap().numbers(1, [0.0, 1.0, 0.0]).is_none(), "{:?}", g[1..4].iter().map(|x| x.as_ref().map(|x| (x.at.clone(), x.gone.clone()))).collect::<Vec<_>>());
        // the same seed, the same runs
        assert_eq!(grown_over(&b, &starts, &[3, 6], 16, 1)[0].as_ref().unwrap().at, green.at);
    }

    #[test]
    fn a_round_end_draws_the_round_targets_from_the_deck_as_the_game_does() {
        // common_events.lua `reset_castle_card` and the like: a random card that isn't a Stone
        // card; Ancient Joker's suit is never the one it had
        let deck = Card::parse_list("7H 7H:stone 2S:stone").unwrap();
        let mut b = Board::empty();
        let mut rng = Rng::new(3);
        for _ in 0..20 {
            let was = b.ancient_suit;
            b.draw_round_targets(&deck, &mut rng);
            assert_eq!((b.castle_suit, b.mail_rank, b.idol), (Some(Suit::Hearts), Some(7), Some((7, Suit::Hearts))));
            assert!(b.ancient_suit.is_some() && b.ancient_suit != was);
        }
        // no card that isn't Stone: the game's defaults
        b.draw_round_targets(&Card::parse_list("2S:stone").unwrap(), &mut rng);
        assert_eq!((b.castle_suit, b.mail_rank, b.idol), (Some(Suit::Spades), Some(14), Some((14, Suit::Spades))));
    }

    /// Worth entries for `b`'s jokers as they are now, each given its place as its id
    fn worths(b: &mut Board, each: &[([f64; 3], f64)]) -> RoundGoals {
        for (i, j) in b.jokers.iter_mut().enumerate() {
            j.id = i as u32 + 1;
        }
        let jokers = b.jokers.iter().zip(each).map(|(j, &(per, gone))| JokerWorth { id: j.id, key: j.key.clone(), start: j.lasting(), per, gone, floor: true, ahead: Default::default() }).collect();
        RoundGoals { jokers: std::sync::Arc::new(jokers), ..Default::default() }
    }

    /// Plays `cards` on `b` and returns what the step left the jokers (`RoundGoals::grown`)
    fn play_grown(g: &RoundGoals, b: &mut Board, cards: &str) -> Grown {
        let was = g.jokers_now(b).unwrap();
        let o = score::score(b, &Card::parse_list(cards).unwrap(), &[], &mut Unlucky, false);
        b.after_hand(&o);
        g.grown(&was, b)
    }

    #[test]
    fn a_joker_used_up_counts_as_gone_and_a_reset_one_as_unchanged() {
        // Ice Cream on its last 5 Chips melts after the hand (card.lua, after context): the
        // round counts what losing it is worth, and the jokers beside it stay matched to their
        // own worth. Hit the Road's ×Mult from a discarded Jack is back to ×1 at the round's end
        // (`Joker::lasting`): nothing left to count. Green Joker loses 1 Mult on a discard.
        let mut b = sample_board(&["j_ice_cream", "j_green_joker", "j_hit_the_road"]);
        b.jokers[0].extra.chips = 5.0;
        b.jokers[1].mult = 5.0;
        let g = worths(&mut b, &[([0.0, 0.0, 0.01], -0.3), ([0.02, 0.0, 0.0], -0.5), ([1.0, 1.0, 1.0], -1.0)]);
        // Green Joker +1 for the hand, Ice Cream gone: the run's value by Ante 8 at 1.02 × 0.7
        assert!((play_grown(&g, &mut b, "2S").long - (1.02f64 * 0.7).ln()).abs() < 1e-12);
        assert_eq!(b.jokers.len(), 2);
        let was = g.jokers_now(&b).unwrap();
        b.discard(&Card::parse_list("JS JH").unwrap());
        assert!(b.jokers[1].x_mult > 1.0);
        assert!((g.grown(&was, &b).long + 1.02f64.ln()).abs() < 1e-12, "the discard's −1 Green Joker Mult only");
    }

    #[test]
    fn a_used_up_copy_is_the_one_counted_gone_and_a_fall_costs_at_most_losing_it() {
        // Two Ice Creams: the first on its last 5 Chips melts, the second loses 5 of its 30
        // (card.lua, after context). The first counts gone, the second its 5 Chips: copies are
        // told apart by their place on your board (`Joker::id`), not by where they sit after it.
        let mut b = sample_board(&["j_ice_cream", "j_ice_cream"]);
        b.jokers[0].extra.chips = 5.0;
        b.jokers[1].extra.chips = 30.0;
        let g = worths(&mut b, &[([0.0, 0.0, 0.01], -0.3), ([0.0, 0.0, 0.01], -0.4)]);
        assert!((play_grown(&g, &mut b, "2S").long - (0.7f64 * 0.95).ln()).abs() < 1e-12);
        // and in the hands after it the survivor is still itself: 5 more hands take its other
        // 25 Chips, −0.25, then it melts: in all its own −0.4 (never below losing it)
        let rest: f64 = (0..6).map(|_| play_grown(&g, &mut b, "2S").long).sum();
        assert!(b.jokers.is_empty() && (rest - (0.6f64 / 0.95).ln()).abs() < 1e-12, "{rest}");
        // An Ice Cream that fades over a round and then melts adds up to losing it, once
        let mut b = sample_board(&["j_ice_cream"]);
        b.jokers[0].extra.chips = 15.0;
        let g = worths(&mut b, &[([0.0, 0.0, 0.01], -0.3)]);
        let total: f64 = (0..3).map(|_| play_grown(&g, &mut b, "2S").long).sum();
        assert!(b.jokers.is_empty() && (total - 0.7f64.ln()).abs() < 1e-12, "{total}");
        // Ride the Bus at +20 Mult reset by a face card: worth 0.01 a Mult, but losing it all
        // costs no more than losing the joker (0.1)
        let mut b = sample_board(&["j_ride_the_bus"]);
        b.jokers[0].mult = 20.0;
        let g = worths(&mut b, &[([0.01, 0.0, 0.0], -0.1)]);
        assert!((play_grown(&g, &mut b, "KS").long - 0.9f64.ln()).abs() < 1e-12);
    }

    #[test]
    fn in_the_blinds_ahead_a_fall_is_priced_near_and_far_and_the_parts_multiply() {
        // Ride the Bus at +7, worth 0.001 a Mult by Ante 8, losing it −0.05 there; in the blinds
        // ahead (80% to get through as it is) one more Mult shifts nothing, the step below (to
        // +5) nothing, past it to none −0.3 of the log-odds a Mult (its reset loses the boss's
        // flushes: `value::Ahead::joker`), losing it −2.
        let mut b = sample_board(&["j_ride_the_bus"]);
        b.jokers[0].mult = 7.0;
        let mut g = worths(&mut b, &[([0.001, 0.0, 0.0], -0.05)]);
        g.ahead = log_odds(0.8, 300);
        std::sync::Arc::make_mut(&mut g.jokers)[0].ahead = AheadWorth { far: [0.3, 0.0, 0.0], knot: [5.0, 1.0, 0.0], gone: -2.0, ..Default::default() };
        // a hand without a face card grows it: Ante 8's ×1.001 only
        let up = play_grown(&g, &mut b, "2S");
        assert!((up.long - 1.001f64.ln()).abs() < 1e-12 && up.ahead == 0.0);
        // a face card's reset: ×0.993 by Ante 8, and the log-odds 5 × 0.3 lower
        let mut b = sample_board(&["j_ride_the_bus"]);
        b.jokers[0].mult = 7.0;
        b.jokers[0].id = g.jokers[0].id;
        let reset = play_grown(&g, &mut b, "KS");
        assert!((reset.long - 0.993f64.ln()).abs() < 1e-12 && (reset.ahead + 1.5).abs() < 1e-12);
        // the round's value: the two parts multiplied, the chance ahead from 80% to σ(ln 4 − 1.5)
        let o = Outcome { won: 1.0, grown: reset, ..Default::default() };
        let ahead = chance_at(4f64.ln() - 1.5) / 0.8;
        assert!((g.value(&o) - 0.993 * ahead).abs() < 1e-9, "{}", g.value(&o));
        // losing it in both parts costs at most a round
        let w = &mut std::sync::Arc::make_mut(&mut g.jokers)[0];
        w.gone = -0.9;
        let gone = w.of(None);
        assert!((gone.long - 0.1f64.ln()).abs() < 1e-12 && gone.ahead == -2.0);
    }

    #[test]
    fn losing_a_joker_ahead_costs_at_least_its_numbers_at_none() {
        // Ride the Bus at +7: nothing a step down, −0.3 of the log-odds a Mult past +5. Where its
        // loss alone wasn't shown (`gone` 0) the reset still counts, floored at its numbers at
        // none (`value::Ahead::joker` takes the lower of the two)
        let mut w = AheadWorth { far: [0.3, 0.0, 0.0], knot: [5.0, 1.0, 0.0], ..Default::default() };
        let start = [7.0, 1.0, 0.0];
        assert_eq!(w.of(&start, Some([0.0, 1.0, 0.0])), 0.0);
        w.gone = w.loss(0.0, &start);
        assert!((w.at_none(&start) + 1.5).abs() < 1e-12);
        assert!((w.of(&start, Some([0.0, 1.0, 0.0])) + 1.5).abs() < 1e-12 && (w.of(&start, None) + 1.5).abs() < 1e-12);
    }

    #[test]
    fn several_falls_multiply_and_growth_never_passes_a_sure_pass() {
        // Three jokers each reset: by Ante 8 each leaves the run at ×0.5, in the blinds ahead
        // (60% as it is) each shifts the log-odds by −1. Added as shares they'd cost more than a
        // round (−1.5 by Ante 8 alone), floored at a lost round's nothing; as the parts combine,
        // the won round keeps ×0.125 of its value by Ante 8 and the chance σ(ln 1.5 − 3) / 0.6
        let g = RoundGoals { ahead: log_odds(0.6, 300), ..Default::default() };
        let o = Outcome { won: 1.0, grown: Grown { long: 3.0 * 0.5f64.ln(), ahead: -3.0 }, ..Default::default() };
        let v = g.value(&o);
        assert!(v > 0.0 && (v - 0.125 * chance_at(1.5f64.ln() - 3.0) / 0.6).abs() < 1e-9, "{v}");
        // however much the jokers grow, getting through counts at most as a sure pass: 1 / 0.6
        let big = Outcome { won: 1.0, grown: Grown { long: 0.0, ahead: 50.0 }, ..Default::default() };
        assert!(g.value(&big) <= 1.0 / 0.6 + 1e-12 && g.value(&big) > 1.0 / 0.6 - 1e-9);
        // and a lost round is nothing
        assert_eq!(g.value(&Outcome { won: 0.0, ..big }), 0.0);
    }

    #[test]
    fn an_outcome_from_mid_round_is_valued_against_the_analysis_start() {
        // Ride the Bus at +7 (0.001 a Mult by Ante 8; in the blinds ahead, 80% as it is, 0.2 of
        // the log-odds a Mult up, 0.3 down past +5), reset by a King earlier in the round. An
        // outcome from that board starts where the reset left the jokers (`at`), so a later
        // hand's +1 is valued where the chance now is, not as if from the start
        let mut b = sample_board(&["j_ride_the_bus"]);
        b.jokers[0].mult = 7.0;
        let mut g = worths(&mut b, &[([0.001, 0.0, 0.0], -0.05)]);
        g.ahead = log_odds(0.8, 300);
        std::sync::Arc::make_mut(&mut g.jokers)[0].ahead = AheadWorth { up: [0.2, 0.0, 0.0], far: [0.3, 0.0, 0.0], knot: [5.0, 1.0, 0.0], gone: -2.0, ..Default::default() };
        let reset = play_grown(&g, &mut b, "KS");
        assert_eq!(g.at(&b), reset);
        // a hand of no face card from there: +1 Mult, from none
        let was = g.jokers_now(&b).unwrap();
        let mut after = b.clone();
        let o = score::score(&after, &Card::parse_list("2S").unwrap(), &[], &mut Unlucky, false);
        after.after_hand(&o);
        let step = g.grown(&was, &after);
        assert!((step.ahead - 0.3).abs() < 1e-12, "{step:?}");
        let v = |gr: Grown| g.value(&Outcome { won: 1.0, grown: gr, ..Default::default() });
        assert!((g.grown_worth(&was, &after) - (v(reset + step) - v(reset))).abs() < 1e-12);
        // the same step from the start would be worth another amount: the chance isn't linear
        assert!((g.grown_worth(&was, &after) - (v(step) - v(Grown::default()))).abs() > 1e-3);
        // and an outcome played on from that board starts at it
        let deck: Vec<Card> = standard_deck();
        let start = RoundStart { hand: Card::parse_list("2S 3H").unwrap(), deck, hand_size: 2, hands: 1, discards: 0, scored: 0.0, target: 1.0, fresh: false, ..Default::default() };
        b.goals = Some(g.clone());
        let outs = outcomes_after(&b, &start, &Move::Play(vec![0]), 0..1, 7, &[]);
        assert!(outs[0].won == 1.0 && (outs[0].grown.ahead - (reset.ahead + 0.3)).abs() < 1e-12, "{:?}", outs[0].grown);
    }

    #[test]
    fn without_the_floor_a_fall_is_linear_and_using_up_a_fallen_joker_earns_nothing() {
        // The growth runs' second pass (`JokerWorth::floor` off): an Ice Cream from 100 Chips that
        // fell to 5 in earlier rounds melts: used up counts as its numbers at none (0 Chips), so
        // the melt costs its last 5 Chips, not a gain back up to `gone`
        let mut b = sample_board(&["j_ice_cream"]);
        b.jokers[0].extra.chips = 100.0;
        let mut g = worths(&mut b, &[([0.0, 0.0, 0.001], -0.3)]);
        let free = |g: &mut RoundGoals| std::sync::Arc::make_mut(&mut g.jokers).iter_mut().for_each(|w| w.floor = false);
        free(&mut g);
        b.jokers[0].extra.chips = 5.0;
        assert!((play_grown(&g, &mut b, "2S").long - (0.9f64 / 0.905).ln()).abs() < 1e-12);
        assert!(b.jokers.is_empty());
        // Ride the Bus at +20 Mult reset: all 20 Mult, past what losing the joker is worth
        let mut b = sample_board(&["j_ride_the_bus"]);
        b.jokers[0].mult = 20.0;
        let mut g = worths(&mut b, &[([0.01, 0.0, 0.0], -0.1)]);
        free(&mut g);
        assert!((play_grown(&g, &mut b, "KS").long - 0.8f64.ln()).abs() < 1e-12);
    }

    #[test]
    fn what_a_play_takes_off_the_jokers_is_counted_joker_by_joker() {
        // Square Joker at 0.01 a Chip and Ride the Bus at +10, 0.001 a Mult: a 4-card play with
        // a scored King grows Square by 4 Chips and resets Ride the Bus, a net gain that hides
        // the reset; the loss is the reset alone
        let mut b = sample_board(&["j_square", "j_ride_the_bus"]);
        b.jokers[1].mult = 10.0;
        let g = worths(&mut b, &[([0.0, 0.0, 0.01], -1.0), ([0.001, 0.0, 0.0], -0.1)]);
        let was = g.jokers_now(&b).unwrap();
        let mut after = b.clone();
        let o = score::score(&b, &Card::parse_list("KS KH 9S 9D").unwrap(), &[], &mut Unlucky, false);
        let _ = after.after_hand(&o);
        assert!(g.grown_worth(&was, &after) > 0.0);
        let mut reset = b.clone();
        reset.jokers[1].mult = 0.0;
        let lost = g.lost_worth(&was, &after);
        assert!(lost < 0.0 && (lost - g.grown_worth(&was, &reset)).abs() < 1e-12, "{lost}");
        // a play that only grows them takes nothing off
        let mut grown = b.clone();
        let o = score::score(&b, &Card::parse_list("9S 9D 2C 3H").unwrap(), &[], &mut Unlucky, false);
        let _ = grown.after_hand(&o);
        assert_eq!(g.lost_worth(&was, &grown), 0.0);
    }

    #[test]
    fn a_line_is_the_play_once_then_the_best_play_on_the_board_it_leaves() {
        // Ride the Bus at +1: the Pair of 2s spares it, and the hands after it are played as the
        // policy plays them (the Kings, which still score most), not as the 2s again
        let mut b = sample_board(&["j_ride_the_bus"]);
        b.jokers[0].mult = 1.0;
        let hand = Card::parse_list("KS KH KD 2C 2H 5D").unwrap();
        let twos = [3, 4];
        let mut after = b.clone();
        after.hands_left = 3;
        let o = score::score(&after, &[hand[3], hand[4]], &[hand[0], hand[1], hand[2], hand[5]], &mut Unlucky, false);
        let _ = after.after_hand(&o);
        let best = best_play(&after, &hand).unwrap();
        assert!(best.cards.contains(&0), "the Kings: {:?}", best.cards);
        let line = line_pace(&b, &hand, &twos, 3);
        assert!((line - o.score - playing_on(&after, &hand, &best, 2)).abs() < 1e-6, "{line}");
        // more than the 2s three times over
        assert!(line > 3.0 * o.score * 1.5, "{line} against {}", o.score);
        // a junk hand that moves no joker (a plain Joker) is followed by the best play too
        let plain = sample_board(&["j_joker"]);
        let junk = [5];
        let o = score::score(&plain, &[hand[5]], &[hand[0], hand[1], hand[2], hand[3], hand[4]], &mut Unlucky, false);
        let best = best_play(&plain, &hand).unwrap();
        let mut after = plain.clone();
        after.hands_left = 3;
        let _ = after.after_hand(&o);
        let want = o.score + playing_on(&after, &hand, &best_play(&after, &hand).unwrap(), 2);
        assert!((line_pace(&plain, &hand, &junk, 3) - want).abs() < 1e-6);
        assert!(line_pace(&plain, &hand, &junk, 3) > o.score + 2.0 * best.floor - 1e-6);
    }

    #[test]
    fn the_sparing_play_is_the_best_one_that_grows_the_jokers_clearly_more() {
        // Ride the Bus at +10 (0.01 a Mult): the policy's Pair of Kings scores face cards and
        // resets it; of the plays that don't, the Pair of Nines scores most, and the lone 2 and
        // 3 it can spare go with it (Ride the Bus reads only the cards that score)
        let mut b = sample_board(&["j_ride_the_bus"]);
        b.jokers[0].mult = 10.0;
        let g = worths(&mut b, &[([0.01, 0.0, 0.0], -0.1)]);
        let hand = Card::parse_list("KS KH 9S 9D 2C 3H").unwrap();
        let was = g.jokers_now(&b).unwrap();
        let (idx, spare) = sparing_play(&b, &g, &was, &hand, &plays_tried(&b, &hand), &[0, 1], 3).unwrap();
        let named = |v: &[usize]| {
            let mut v: Vec<String> = v.iter().map(|&i| hand[i].label()).collect();
            v.sort();
            v
        };
        assert_eq!(named(&idx), named(&[2, 3]));
        assert!(spare.len() > idx.len() && spare.iter().all(|&i| i >= 2), "{:?}", named(&spare));
        // a play that already grows it has nothing clearly better beside it
        assert!(sparing_play(&b, &g, &was, &hand, &plays_tried(&b, &hand), &[2, 3], 3).is_none());
    }

    #[test]
    fn the_round_plays_on_the_board_a_discard_leaves() {
        // Hit the Road grows ×0.5 for each Jack discarded (card.lua, discard context): the
        // hands after that discard score with it, on the same draws
        let hand = Card::parse_list("JS JH JD JC 2S 3H 5D 7C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit)).collect();
        let start = RoundStart { hand, deck, hand_size: 8, hands: 2, discards: 1, scored: 0.0, target: 1e12, fresh: false, ..Default::default() };
        let b = sample_board(&["j_hit_the_road"]);
        let mut off = b.clone();
        off.jokers[0].debuff = true;
        let first = Move::Discard(vec![0, 1, 2, 3]);
        let with = outcomes_after(&b, &start, &first, 0..50, 3, &[]);
        let without = outcomes_after(&off, &start, &first, 0..50, 3, &[]);
        for (w, o) in with.iter().zip(&without) {
            assert!(w.total >= 3.0 * o.total - 1.0, "×3 after four Jacks: {} vs {}", w.total, o.total);
        }
    }

    #[test]
    fn using_a_consumable_drops_its_cards_worth_drawing() {
        let seal = Card::parse_list("3S:blue").unwrap()[0];
        let mut b = sample_board(&["j_joker"]);
        b.goals = Some(RoundGoals { seen: vec![("c_cryptid".into(), seal, 0.5), ("c_talisman".into(), seal, 0.2)], ..Default::default() });
        let hand = Card::parse_list("3S:blue AH").unwrap();
        let used = Use { key: "c_cryptid".into(), swap: vec![(seal, seal)], add: vec![seal, seal], ..Default::default() };
        let (after, _) = used.apply(&b, &hand).unwrap();
        let g = after.goals.unwrap();
        assert_eq!(g.seen.len(), 1);
        assert_eq!(g.seen_gain(&[seal]), 0.2, "only the Talisman entry is left");
    }

    #[test]
    fn a_consumable_used_mid_round_stops_counting_its_cards() {
        // The simulated player uses the consumable at its first decision (it lifts every hand a
        // level): drawing its "worth drawing" card later in the round then counts for nothing.
        let seal = Card::parse_list("3S:blue").unwrap()[0];
        let hand = Card::parse_list("AS AH KD 9C 7S 5H 4D 2C").unwrap();
        let mut deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit) && !(c.rank == seal.rank && c.suit == seal.suit)).collect();
        deck.push(seal);
        let mut b = sample_board(&["j_joker"]);
        b.goals = Some(RoundGoals { seen: vec![("c_test".into(), seal, 0.5)], ..Default::default() });
        let used = Use { key: "c_test".into(), levels: [1; 12], ..Default::default() };
        let start = RoundStart { hand, deck, hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1e9, fresh: false, ..Default::default() };
        let held = outcomes_after(&b, &start, &Move::Discard(vec![4, 5, 6, 7]), 0..200, 7, &[]);
        let spent = outcomes_after(&b, &start, &Move::Discard(vec![4, 5, 6, 7]), 0..200, 7, &[used]);
        assert!(held.iter().any(|o| o.seen > 0.0), "held: the card counts when drawn");
        assert!(spent.iter().all(|o| o.seen == 0.0), "used at the first decision: it never counts");
    }

    #[test]
    fn rounds_one_by_one_match_the_odds() {
        // Round i draws the same cards either way, so a slice of rounds compared one by one
        // (the target search's race) is the same measure as the projection's mean.
        let b = sample_board(&["j_joker", "j_greedy_joker"]);
        let start = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 900.0, fresh: true, ..Default::default() };
        let (_, st) = round_odds(&b, &start, 64, 11u64.wrapping_add(16 * 7919));
        let one = round_results(&b, &start, 16..80, 11);
        assert!((one.iter().map(|r| r.total).sum::<f64>() / 64.0 - st.mean).abs() < 1e-6);
    }

    #[test]
    fn each_card_drawn_is_dealt_face_down_where_the_boss_turns_it_down() {
        // blind.lua `Blind:stay_flipped`, asked for every card drawn to hand
        let drawn = |b: &Board, seed: u64| {
            let (mut hand, mut deck) = (vec![], standard_deck());
            let mut b = b.clone();
            draw(&mut b, &mut hand, &mut deck, 52, &mut Rng::new(seed));
            // `Blind:drawn_to_hand`: the draw done, the next isn't The Fish's to turn down
            assert!(!b.blind.prepped);
            hand
        };
        let under = |keys: &[&str], blind: &str, disabled: bool| {
            let mut b = sample_board(keys);
            b.blind = score::BlindRules { key: blind.into(), disabled, ..Default::default() };
            b
        };
        let down = |h: &[Card]| h.iter().filter(|c| c.face_down).count();
        // The House: the round's first deal only, before a hand is played or a discard used
        let house = under(&[], "bl_house", false);
        assert_eq!(down(&drawn(&house, 1)), 52);
        let mut played = house.clone();
        played.levels[HandType::Pair as usize].played_this_round = 1;
        assert_eq!(down(&drawn(&played, 1)), 0);
        assert_eq!(down(&drawn(&Board { discards_used: 1, ..house.clone() }, 1)), 0);
        // The Mark: the face cards, every card with a Pareidolia that isn't debuffed
        let m = drawn(&under(&[], "bl_mark", false), 1);
        assert!(m.iter().all(|c| c.face_down == (11..=13).contains(&c.rank.0)));
        assert_eq!(down(&m), 12);
        let mut pareidolia = under(&["j_pareidolia"], "bl_mark", false);
        assert_eq!(down(&drawn(&pareidolia, 1)), 52);
        pareidolia.jokers[0].debuff = true;
        assert_eq!(down(&drawn(&pareidolia, 1)), 12);
        // The Fish: the draw after a hand played (`Board::after_hand` sets `prepped`), not a
        // discard's or the round's first
        let fish = under(&[], "bl_fish", false);
        assert_eq!(down(&drawn(&fish, 1)), 0);
        let mut pressed = fish.clone();
        let _ = pressed.after_hand(&score::score(&fish, &Card::parse_list("AS").unwrap(), &[], &mut Unlucky, false));
        assert_eq!(down(&drawn(&pressed, 1)), 52);
        let mut thrown = fish.clone();
        let _ = thrown.discard(&Card::parse_list("AS").unwrap());
        assert_eq!(down(&drawn(&thrown, 1)), 0);
        // The Wheel: 1 in 7 of the cards (a share's standard error on 10,400 cards about
        // 0.0034), 2 in 7 with Oops! All 6s (`probabilities.normal` 2)
        let mut wheel = under(&[], "bl_wheel", false);
        let share = |b: &Board| (0..200).map(|s| down(&drawn(b, s))).sum::<usize>() as f64 / (200.0 * 52.0);
        assert!((share(&wheel) - 1.0 / 7.0).abs() < 0.015, "{}", share(&wheel));
        wheel.probability = 2.0;
        assert!((share(&wheel) - 2.0 / 7.0).abs() < 0.015, "{}", share(&wheel));
        // disabled, or another blind: none
        for boss in ["bl_house", "bl_mark", "bl_fish", "bl_wheel"] {
            let mut b = under(&["j_pareidolia"], boss, true);
            b.blind.prepped = true;
            assert_eq!(down(&drawn(&b, 1)), 0, "{boss}");
        }
        let mut hook = under(&[], "bl_hook", false);
        hook.blind.prepped = true;
        assert_eq!(down(&drawn(&hook, 1)), 0);
    }

    #[test]
    fn a_simulated_round_decides_unseen_on_the_cards_its_boss_deals_face_down() {
        // one decision each (1 hand, no discard): the player decides without seeing a card the
        // boss dealt face down (`decide_unseen`)
        let unseen = |b: &Board, start: &RoundStart| {
            UNSEEN_DECISIONS.with(|x| x.set(0));
            sim_round(b, start, &mut Rng::new(3));
            UNSEEN_DECISIONS.with(|x| x.get())
        };
        let fresh = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 1, discards: 0, scored: 0.0, target: 1e12, fresh: true, ..Default::default() };
        let on = |blind: &str| RoundStart { blind: blind.into(), ..fresh.clone() };
        // a fresh round: The House's first hand; none without a boss that turns cards down
        assert_eq!(unseen(&Board::empty(), &on("bl_house")), 1);
        assert_eq!(unseen(&Board::empty(), &on("")), 0);
        assert_eq!(unseen(&Board::empty(), &on("bl_fish")), 0);
        // a round in progress (its blind on the board): its first cards still to deal under The
        // House while nothing is played or thrown, not once a hand was played or a discard used
        let mut house = Board::empty();
        house.blind = score::BlindRules { key: "bl_house".into(), ..Default::default() };
        let dealing = RoundStart { fresh: false, ..fresh.clone() };
        assert_eq!(unseen(&house, &dealing), 1);
        let mut played = house.clone();
        played.levels[HandType::Pair as usize].played_this_round = 1;
        assert_eq!(unseen(&played, &dealing), 0);
        assert_eq!(unseen(&Board { discards_used: 1, ..house.clone() }, &dealing), 0);
        // The Mark: its face cards, a fresh round's first hand too
        assert_eq!(unseen(&Board::empty(), &on("bl_mark")), 1);
        // The Fish: the cards drawn right after a play (`outcomes_after`'s first move sets it)
        let mut fish = Board::empty();
        fish.blind = score::BlindRules { key: "bl_fish".into(), ..Default::default() };
        let held = Card::parse_list("AS AH 2C").unwrap();
        let rest: Vec<Card> = standard_deck().into_iter().filter(|c| !held.iter().any(|h| h.same_kind(c))).collect();
        let short = RoundStart { hand: held.clone(), deck: rest.clone(), fresh: false, scored: 40.0, ..fresh.clone() };
        let mut pressed = fish.clone();
        pressed.blind.prepped = true;
        assert_eq!(unseen(&pressed, &short), 1);
        assert_eq!(unseen(&fish, &short), 0);
        // inside one round under The Fish: every draw after a play turned down (each refills at
        // least the card played), the round's first hand seen: 3 hands, no discard, the 2nd and
        // 3rd decisions unseen
        assert_eq!(unseen(&Board::empty(), &RoundStart { hands: 3, ..on("bl_fish") }), 2);
        // a first move, then the round's one decision left
        let start = RoundStart { hand: held.clone(), deck: rest, hand_size: 3, ..short.clone() };
        let after = |m: Move, hands: i64, discards: i64| {
            UNSEEN_DECISIONS.with(|x| x.set(0));
            outcomes_after(&fish, &RoundStart { hands, discards, ..start.clone() }, &m, 0..1, 3, &[]);
            UNSEEN_DECISIONS.with(|x| x.get())
        };
        assert_eq!(after(Move::Play(vec![2]), 2, 0), 1);
        assert_eq!(after(Move::Discard(vec![2]), 1, 1), 0);
    }

    #[test]
    fn a_hand_short_since_its_last_draw_plays_as_it_is_until_its_next_move() {
        // the game draws only after a play or a discard (game.lua `Game:update_hand_played`,
        // state_events.lua `G.FUNCS.discard_cards_from_highlighted`, then
        // `G.FUNCS.draw_from_deck_to_hand`), so a hand a consumable took cards from makes its
        // next move as it is: A♠ alone, held
        // beside nothing, though the pile is Steel cards (×1.5 Mult each held)
        let ace = Card::parse_list("AS").unwrap();
        let steel = Card::parse_list("KS:steel KH:steel KD:steel KC:steel QS:steel QH:steel QD:steel QC:steel").unwrap();
        let b = Board::empty();
        let start = RoundStart { hand: ace.clone(), deck: steel, hand_size: 8, hands: 1, discards: 0, scored: 0.0, target: 1e12, fresh: false, ..Default::default() };
        let dealt = start.clone().dealt();
        assert_eq!(dealt.short, 7);
        let alone = score::score(&b, &ace, &[], &mut Unlucky, false).score;
        // Best play's line: its first move on the hand as it is
        let first = |s: &RoundStart| outcomes_after(&b, s, &Move::Play(vec![0]), 0..1, 3, &[])[0].total;
        assert_eq!(first(&dealt), alone);
        // the round simulated from there, its one hand the same
        assert_eq!(sim_round(&b, &dealt, &mut Rng::new(3)).total, alone);
        // a hand whose draw is still owed (the rest of a round after a move) is filled first:
        // the Steel cards held beside it
        assert!(first(&start) > alone);
        // with a hand more, the draw after the first play fills it: the second hand plays the
        // Steel cards
        let two = RoundStart { hands: 2, ..dealt.clone() };
        assert!(sim_round(&b, &two, &mut Rng::new(3)).total > 2.0 * alone);
        // an empty hand is filled at once (game.lua `Game:update`, the hand-selecting state), and
        // a fresh round's first deal fills it whatever it holds
        assert_eq!(RoundStart { hand: vec![], ..start.clone() }.dealt().short, 0);
        assert_eq!(RoundStart { fresh: true, ..start.clone() }.dealt().short, 0);
        // a decision's futures (the oracle's) start from the hand as it is too
        assert_eq!(RoundStart::at_decision(&ace, &start.deck, 8, 1, 0, 0.0, 1e12).short, 7);
    }

    #[test]
    fn a_fresh_round_is_played_as_its_blind_sets_it_and_one_in_progress_as_it_stands() {
        // Burglar (card.lua `calculate_joker`, `setting_blind`): a fresh round from 4 hands and
        // 3 discards is played with 7 hands and none; a round in progress keeps its own, even
        // while its first cards aren't dealt yet (empty hand, nothing scored).
        let b = sample_board(&["j_burglar"]);
        let fresh = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1e12, fresh: true, ..Default::default() };
        let r = sim_round(&b, &fresh, &mut Rng::new(3));
        assert_eq!(r.plays.len(), 7);
        let dealt = RoundStart { fresh: false, ..fresh.clone() };
        assert_eq!(sim_round(&b, &dealt, &mut Rng::new(3)).plays.len(), 4);
        // a first move's outcomes: the rest of the round on what the blind set, not set again
        let pair = Card::parse_list("AS AH").unwrap();
        let start = RoundStart { hand: pair.clone(), deck: standard_deck().into_iter().filter(|c| !pair.iter().any(|p| p.same_kind(c))).collect(), hand_size: 2, target: 1.0, ..fresh };
        let o = &outcomes_after(&b, &start, &Move::Play(vec![0, 1]), 0..1, 3, &[])[0];
        assert_eq!((o.won, o.spare), (1.0, 6.0));
    }

    #[test]
    fn a_fresh_round_takes_its_bosss_rule_on_hands_and_discards_before_the_jokers() {
        // state_events.lua `new_round`: blind.lua `Blind:set_blind` first (The Needle leaves one
        // hand, The Water no discards, The Manacle -1 hand size, suit/face debuffs), then each
        // joker's `setting_blind` (Burglar: +3 hands, no discards); a disabled boss takes nothing,
        // and a round in progress keeps what it was set with.
        let fresh = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1e12, fresh: true, ..Default::default() };
        let for_blind = |keys: &[&str], blind: &str| {
            let b = sample_board(keys);
            let start = RoundStart { blind: blind.to_string(), ..fresh.clone() };
            (b, start)
        };
        let (b, start) = for_blind(&[], "bl_needle");
        assert_eq!(start.hands_on(&b), (1, 3));
        let (b, start) = for_blind(&[], "bl_water");
        assert_eq!(start.hands_on(&b), (4, 0));
        let (b, start) = for_blind(&[], "bl_big");
        assert_eq!(start.hands_on(&b), (4, 3));
        let (b, start) = for_blind(&["j_burglar"], "bl_needle");
        assert_eq!(start.hands_on(&b), (4, 0));
        let (b, start) = for_blind(&["j_burglar"], "bl_water");
        assert_eq!(start.hands_on(&b), (7, 0));

        // disabled blind takes nothing
        let (b, mut start) = for_blind(&[], "bl_needle");
        start.disabled = true;
        assert_eq!(start.hands_on(&b), (4, 3));

        // round in progress keeps what it had
        let (b, needle_start) = for_blind(&[], "bl_needle");
        assert_eq!(RoundStart { fresh: false, ..needle_start.clone() }.hands_on(&b), (4, 3));
        assert_eq!(sim_round(&b, &needle_start, &mut Rng::new(3)).plays.len(), 1);

        // Manacle reduces hand size by 1 (unless disabled)
        let (b, manacle) = for_blind(&[], "bl_manacle");
        let (_, s) = b.at_blind_set(&manacle);
        assert_eq!(s.hand_size, 7);
        let (_, s) = b.at_blind_set(&RoundStart { disabled: true, ..manacle });
        assert_eq!(s.hand_size, 8);

        // The Club debuffs the Clubs, The Plant the face cards (every card with Pareidolia)
        let (b, club) = for_blind(&[], "bl_club");
        let club = RoundStart { hand: Card::parse_list("2C 2H").unwrap(), ..club };
        let (_, s) = b.at_blind_set(&club);
        assert!(s.hand.iter().chain(&s.deck).all(|c| c.debuff == (c.suit == Suit::Clubs)) && s.hand[0].debuff);
        let (b, plant) = for_blind(&[], "bl_plant");
        assert_eq!(b.at_blind_set(&plant).1.deck.iter().filter(|c| c.debuff).count(), 12);
        let (b, plant) = for_blind(&["j_pareidolia"], "bl_plant");
        assert!(b.at_blind_set(&plant).1.deck.iter().all(|c| c.debuff));
        // and every card's debuff is the new blind's (`Blind:debuff_card`), dealt face up: the last one's go
        let (b, mut big) = for_blind(&[], "bl_big");
        big.deck.iter_mut().for_each(|c| (c.debuff, c.face_down) = (true, true));
        assert!(!b.at_blind_set(&big).1.deck.iter().any(|c| c.debuff || c.face_down));

        // from no hands a round (Black Deck, Hieroglyph, two Troubadours) the game's max(1, 0)
        // less −1: two
        let (b, needle_zero) = for_blind(&[], "bl_needle");
        assert_eq!(RoundStart { hands: 0, ..needle_zero }.hands_on(&b), (2, 3));
    }

    #[test]
    fn the_pillar_debuffs_the_cards_played_this_ante_and_rounds_carry_them() {
        // blind.lua `Blind:debuff_card`: The Pillar debuffs a card with `played_this_ante`, which
        // state_events.lua sets on each card played and clears on every card once a boss is beaten
        let fresh = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1.0, fresh: true, ..Default::default() };
        let mut save = fresh.clone();
        for c in save.deck.iter_mut().filter(|c| c.rank.0 >= 13) {
            c.played_this_ante = true;
        }
        let pillar = RoundStart { blind: "bl_pillar".into(), ..save.clone() };
        let b = sample_board(&[]);
        let debuffed = |b: &Board, s: &RoundStart| b.at_blind_set(s).1.deck.iter().filter(|c| c.debuff).map(|c| c.label()).collect::<Vec<_>>();
        assert_eq!(debuffed(&b, &pillar).len(), 8);
        assert!(debuffed(&b, &RoundStart { disabled: true, ..pillar.clone() }).is_empty());
        assert!(debuffed(&b, &RoundStart { blind: "bl_big".into(), ..save.clone() }).is_empty());

        // a card the board's rounds played is marked on one card like it, unless that one is
        // marked already; after a boss beaten the start's marks are an ante gone
        let mut played = sample_board(&[]);
        played.played_ante = Card::parse_list("AS KS QH").unwrap();
        let two_queens = RoundStart { deck: [save.deck.clone(), Card::parse_list("QH").unwrap()].concat(), ..pillar.clone() };
        let d = debuffed(&played, &two_queens);
        assert_eq!((d.len(), d.iter().filter(|l| l.starts_with("Q♥")).count()), (9, 1));
        // each played card marks its own copy, and only a copy with the same bonus chips
        let mut twice = sample_board(&[]);
        twice.played_ante = Card::parse_list("QH QH 2C").unwrap();
        twice.played_ante[2].perma_bonus = 4.0;
        let d = debuffed(&twice, &RoundStart { deck: [fresh.deck.clone(), Card::parse_list("QH").unwrap()].concat(), ..pillar.clone() });
        assert_eq!((d.len(), d.iter().filter(|l| l.starts_with("Q♥")).count()), (2, 2));
        // a new ante (a boss beaten, then this ante's rounds played on)
        played.ante_over = true;
        assert_eq!(debuffed(&played, &pillar).len(), 3);

        // a round carries the cards it played to the next one, and a boss beaten ends the ante
        let big = RoundStart { blind: "bl_big".into(), ..fresh.clone() };
        let (r, end) = play_round(&b, &big, &mut Rng::new(5), &[]);
        assert!(r.won && !end.played_ante.is_empty() && end.played_ante.iter().all(|c| c.played_this_ante) && !end.ante_over);
        let n = end.played_ante.len();
        assert_eq!(debuffed(&end, &RoundStart { blind: "bl_pillar".into(), ..fresh.clone() }).len(), n);
        let (r, after_boss) = play_round(&end, &RoundStart { blind: "bl_pillar".into(), ..fresh.clone() }, &mut Rng::new(5), &[]);
        assert!(r.won && after_boss.played_ante.is_empty() && after_boss.ante_over);
    }

    #[test]
    fn a_fresh_round_runs_each_jokers_setting_blind_as_the_game_does() {
        // card.lua `Card:calculate_joker`, `context.setting_blind`, left to right
        let fresh = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1000.0, fresh: true, blind: "bl_small".into(), ..Default::default() };
        let keys = |b: &Board| b.jokers.iter().map(|j| j.key.clone()).collect::<Vec<_>>();

        // Riff-raff: up to 2 Common jokers while the slots allow, none you own, each with a place
        // of its own; a copy makes its own; only where a round is simulated (its random part)
        let riff = sample_board(&["j_riff_raff"]);
        let (end, _) = riff.at_blind_set_drawn(&fresh, 1);
        assert_eq!(end.jokers.len(), 3);
        assert!(end.jokers[1..].iter().all(|j| j.rarity == 1 && j.key != "j_riff_raff") && end.jokers[1].key != end.jokers[2].key);
        assert!(end.jokers[1].id > 0 && end.jokers[2].id > end.jokers[1].id);
        assert_eq!(riff.at_blind_set(&fresh).0.jokers.len(), 1, "the fixed part makes none");
        let mut full = sample_board(&["j_riff_raff", "j_joker"]);
        full.joker_slots = 2;
        assert_eq!(full.at_blind_set_drawn(&fresh, 1).0.jokers.len(), 2);
        let copied = sample_board(&["j_blueprint", "j_riff_raff"]);
        assert_eq!(copied.at_blind_set_drawn(&fresh, 1).0.jokers.len(), 5, "3 made, as the 5 slots allow");
        let juggler = (0..200).map(|seed| riff.at_blind_set_drawn(&fresh, seed)).find(|(b, _)| b.jokers.iter().any(|j| j.key == "j_juggler")).expect("a draw makes Juggler");
        assert_eq!((juggler.1.hand_size, juggler.0.round_resets.0), (9, 1));
        assert_eq!(juggler.0.at_blind_set(&fresh).1.hand_size, 9, "the next round keeps its hand size");
        let draws: std::collections::BTreeSet<Vec<String>> = (0..20).map(|seed| keys(&riff.at_blind_set_drawn(&fresh, seed).0)).collect();
        assert!(draws.len() > 1, "each round draws anew");

        // Chicot disables a boss after the other jokers act: what it took given back (Burglar's
        // discards under The Water too, whichever side it sits), The Wall's target halved
        let chicot = sample_board(&["j_chicot"]);
        assert_eq!(RoundStart { blind: "bl_needle".into(), ..fresh.clone() }.hands_on(&chicot), (4, 3));
        let (end, s) = chicot.at_blind_set(&RoundStart { blind: "bl_wall".into(), ..fresh.clone() });
        assert!(end.blind.disabled && s.target == 500.0);
        assert_eq!(chicot.at_blind_set(&RoundStart { blind: "bl_manacle".into(), ..fresh.clone() }).1.hand_size, 8);
        assert!(!chicot.at_blind_set(&fresh).0.blind.disabled);
        let water = RoundStart { blind: "bl_water".into(), ..fresh.clone() };
        assert_eq!(water.hands_on(&sample_board(&["j_chicot", "j_burglar"])), (7, 3));
        assert_eq!(water.hands_on(&sample_board(&["j_burglar", "j_chicot"])), (7, 3));

        // Marble Joker: a Stone card into the deck, dealt in later rounds too (Hologram grows by
        // it); the Juggle Tags held add their hand size this round only
        let marble = sample_board(&["j_marble", "j_hologram"]);
        let (end, s) = marble.at_blind_set(&RoundStart { juggle: 2, ..fresh.clone() });
        assert_eq!((s.deck.len(), end.stone_tally, end.playing_cards), (53, 1, 53));
        assert_eq!((end.jokers[1].x_mult, s.hand_size, s.juggle), (1.25, 14, 0));
        assert_eq!(end.at_blind_set(&fresh).1.deck.len(), 54, "the next round deals the first Stone card too");

        // not modelled: Ceremonial Dagger's and Madness's (gap 8), Cartomancer's Tarot
        let left = sample_board(&["j_ceremonial", "j_madness", "j_joker", "j_cartomancer"]);
        let (end, _) = left.at_blind_set_drawn(&fresh, 1);
        assert_eq!((keys(&end), end.jokers[0].mult, end.jokers[1].x_mult, end.planet_slots), (keys(&left), 0.0, 1.0, left.planet_slots));

        // what a joker gives while held: none while debuffed (card.lua `Card:set_debuff`)
        let mut drunk = sample_board(&["j_drunkard"]);
        assert_eq!(drunk.jokers[0].round_mods(), (0, 0, 1));
        drunk.jokers[0].debuff = true;
        assert_eq!(drunk.jokers[0].round_mods(), (0, 0, 0));
    }

    #[test]
    fn verdant_leaf_debuffs_every_card_until_the_player_sells_a_joker() {
        // blind.lua `Blind:debuff_card`: every card under Verdant Leaf; card.lua `Card:sell_card`
        // disables it (`Blind:disable` sets every card's debuff again), Campfire grows by the sale
        let fresh = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 1, discards: 0, scored: 0.0, target: 1.0, fresh: true, blind: "bl_final_leaf".into(), ..Default::default() };
        let b = sample_board(&["j_joker", "j_jolly"]);
        let (_, s) = b.at_blind_set(&fresh);
        assert!(s.deck.iter().all(|c| c.debuff));
        assert!(!b.at_blind_set(&RoundStart { disabled: true, ..fresh.clone() }).1.deck.iter().any(|c| c.debuff));

        let mut fire = sample_board(&["j_joker", "j_campfire"]);
        fire.blind.key = "bl_final_leaf".into();
        fire.jokers[0].sell_value = 2.0;
        let x = fire.jokers[1].x_mult;
        assert_eq!(fire.sell_joker(0), 2.0);
        assert!(fire.blind.disabled && fire.jokers.len() == 1 && fire.jokers[0].x_mult == x + fire.jokers[0].extra.n && fire.dollars == 2.0);
        // a debuffed Campfire doesn't grow (card.lua: a debuffed joker answers no context)
        let mut out = sample_board(&["j_joker", "j_campfire"]);
        out.jokers[1].debuff = true;
        out.sell_joker(0);
        assert_eq!(out.jokers[0].x_mult, x);
        let mut cards = Card::parse_list("AS AH").unwrap();
        b.at_blind_set(&fresh).0.debuff_cards(&mut cards, &mut []);
        assert!(cards.iter().all(|c| c.debuff));
        fire.debuff_cards(&mut cards, &mut []);
        assert!(!cards.iter().any(|c| c.debuff));

        // the player sells the joker whose sale leaves the best play scoring most (Joker, keeping
        // Jolly Joker's +8 for the Pair), never an eternal one, and is paid for it
        let pair = RoundStart { hand: Card::parse_list("AS AH 2C 5D 7H 9S JC 4D").unwrap(), deck: vec![], hands: 1, ..fresh.clone() };
        let mut sell = sample_board(&["j_joker", "j_jolly"]);
        sell.jokers.iter_mut().for_each(|j| j.sell_value = 3.0);
        let (r, end) = play_round(&sell, &pair, &mut Rng::new(1), &[]);
        assert!(r.won && r.money == 3.0 && end.jokers.len() == 1 && end.jokers[0].key == "j_jolly" && end.blind.disabled);
        assert_eq!(r.plays[0].1, (10.0 + 11.0 + 11.0) * (2.0 + 8.0));
        sell.jokers[0].eternal = true;
        let (_, end) = play_round(&sell, &pair, &mut Rng::new(1), &[]);
        assert_eq!(end.jokers.iter().map(|j| j.key.as_str()).collect::<Vec<_>>(), ["j_joker"]);
        // Best play's first move is played on the board the sale leaves, as the rest of its round
        // (Joker eternal: Jolly Joker sold, Joker's +4 kept)
        let o = &outcomes_after(&sell, &RoundStart { target: 1e9, ..pair.clone() }, &Move::Play(vec![0, 1]), 0..1, 3, &[])[0];
        assert_eq!((o.total, o.cash), ((10.0 + 11.0 + 11.0) * (2.0 + 4.0), 3.0));
        // nothing to sell: the round is played with every card debuffed
        let (r, end) = play_round(&sample_board(&[]), &pair, &mut Rng::new(1), &[]);
        assert!(!end.blind.disabled && r.plays[0].1 == 10.0 * 2.0);
    }

    #[test]
    fn a_held_consumable_is_used_at_the_finish_when_it_pays_more() {
        // The round is won on the first hand either way. Held: a consumable that adds two
        // copies of the Blue Seal 3♠ to the hand. Not needed for score (on pace), so it's held,
        // and used right before the winning hand: three Blue Seals held at the end, three
        // planets (its own slot freed), not one.
        let seal = Card::parse_list("3S:blue").unwrap()[0];
        let hand = Card::parse_list("AS AH AD AC KS 3S:blue 2C 4D").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit)).collect();
        let mut b = sample_board(&["j_joker"]);
        b.planet_slots = 2;
        b.goals = Some(RoundGoals { planet: [0.1; 12], ..Default::default() });
        let copies = Use { key: "c_test".into(), swap: vec![(seal, seal)], add: vec![seal, seal], ..Default::default() };
        let start = RoundStart { hand, deck, hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 100.0, fresh: false, ..Default::default() };
        let r = sim_round_uses(&b, &start, &mut Rng::new(3), &[copies]);
        assert!(r.won && r.planets >= 3.0, "planets {}", r.planets);
    }

    #[test]
    fn the_oracle_stays_out_of_the_policys_look_ahead() {
        // The oracle (`set_oracle`) judges the policy's decisions on futures the policy plays;
        // the policy's own look-ahead (`play_on_instead`: a Pair of Kings wins now, or dig on for a
        // Flush, whose planet is worth more on the Blue Seal held) is the policy's estimate
        // too. With the oracle on, its futures meet decisions the oracle would make, but it
        // makes none of them, and the flag is left as it was.
        let hand = Card::parse_list("KH KD 9S 7S 5S 3S:blue 2C 4D").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit)).collect();
        let mut b = sample_board(&["j_joker"]);
        b.planet_slots = 2;
        let mut planet = [0.0; 12];
        planet[HandType::Flush as usize] = 0.5;
        b.goals = Some(RoundGoals { planet, ..Default::default() });
        let win = vec![0, 1];
        let target = 0.9 * score::score(&b, &hand[..2], &hand[2..], &mut Unlucky, false).score;
        let decisions = || ORACLE_DECISIONS.with(|x| x.get());
        set_oracle(8);
        let before = decisions();
        let on = play_on_instead(&b, &hand, &deck, 4, 3, 0.0, target, 8, &win, None, &[], &[], &Rng::new(3));
        let during = decisions() - before;
        let start = RoundStart { hand: hand.clone(), deck: deck.clone(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target, fresh: false, ..Default::default() };
        outcomes_after(&b, &start, &Move::Discard(vec![0, 1, 6, 7]), 0..8, 3, &[]);
        let met = decisions() - before - during;
        let flag = IN_ROLLOUT.with(|x| x.get());
        set_oracle(0);
        let off = play_on_instead(&b, &hand, &deck, 4, 3, 0.0, target, 8, &win, None, &[], &[], &Rng::new(3));
        assert!(matches!(on, Some(Action::Discard(_))), "plays on: the look-ahead ran ({on:?})");
        assert!(met > 0, "the same kind of futures meet the oracle's decisions outside a look-ahead");
        assert_eq!(during, 0, "the oracle decided inside the policy's look-ahead");
        assert_eq!(format!("{on:?}"), format!("{off:?}"));
        assert!(!flag);
    }

    #[test]
    fn a_consumable_that_gains_nothing_by_waiting_is_used_while_on_pace() {
        // On pace with a Pair over several hands, holding a Pair planet: it puts nothing in
        // hand that pays at round end, so it's used at once (every hand scores more), not held.
        // Also with a Blue Seal in hand and no free slot: the slot it frees is freed whenever
        // it's used, so that's no reason to wait.
        for (cards, slots) in [("AS AH KD 9C 7S 5H 4D 2C", 2), ("AS AH KD 9C 7S 5H:blue 4D 2C", 0)] {
            let hand = Card::parse_list(cards).unwrap();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit)).collect();
            let mut b = sample_board(&["j_joker"]);
            b.planet_slots = slots;
            let pair = best_play(&b, &hand).unwrap().floor;
            let mut levels = [0; 12];
            levels[HandType::Pair as usize] = 3;
            let planet = Use { key: "c_test".into(), levels, planet: true, ..Default::default() };
            let start = RoundStart { hand, deck, hand_size: 8, hands: 4, discards: 0, scored: 0.0, target: pair * 2.5, fresh: false, ..Default::default() };
            let with = sim_round_uses(&b, &start, &mut Rng::new(5), &[planet]);
            let without = sim_round_uses(&b, &start, &mut Rng::new(5), &[]);
            // the first hand already has the lift
            assert!(with.plays[0].1 > without.plays[0].1, "{cards}: {} vs {}", with.plays[0].1, without.plays[0].1);
        }
    }

    #[test]
    fn a_burn_keeps_the_flush_draw() {
        // Burning discards for Mystic Summit with four Spades in hand: the cards thrown are the
        // ones a digging player throws (the off-suit ones), not simply the lowest.
        let hand = Card::parse_list("AS KS 9S 4S 7H 7D 3C 2C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit)).collect();
        let mut b = sample_board(&["j_mystic_summit"]);
        b.discards_left = 2;
        b.hands_left = 3;
        let pair = best_play(&b, &hand).unwrap().floor;
        match decide(&b, &hand, &deck, 3, 2, pair * 2.0, 8) {
            Action::Discard(v) => assert!(v.iter().all(|&i| hand[i].suit != Suit::Spades), "threw a Spade: {:?}", v.iter().map(|&i| hand[i].label()).collect::<Vec<_>>()),
            other => panic!("expected a discard, got {other:?}"),
        }
    }
}
