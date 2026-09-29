//! Monte Carlo on top of the scoring engine: best play for a hand, and whole-round
//! simulations for "chance to beat this blind".
//!
//! The discard policy is a heuristic (labelled as such in every output): the game has
//! no "right" discard, and a perfect one would need a search far beyond a 1 s budget.

use crate::engine::hand::{self, HandType};
use crate::engine::score::{self, Board};
use crate::engine::{Kind, Rng, Rolls, Unlucky};
use crate::model::{Card, Edition, Enhancement, Suit};

/// Best play found for one hand.
#[derive(Debug, Clone)]
pub struct Play {
    /// Indices into the hand, in the order to play them.
    pub cards: Vec<usize>,
    pub hand: HandType,
    /// Score with every random roll failing (used to choose).
    pub floor: f64,
}

/// Whether the board has jokers that care about unscored kickers (so kicker choices matter).
fn kickers_matter(b: &Board) -> bool {
    b.blind.key == "bl_psychic"
        || b.jokers.iter().any(|j| {
            matches!(j.kind, Kind::Half | Kind::Square | Kind::Blackboard | Kind::RaisedFist | Kind::Splash | Kind::Hiker)
        })
}

/// Order a play the way a player would: +Chips/+Mult cards first, ×Mult cards (Glass,
/// Polychrome) last, so multipliers apply to everything before them. Heuristic.
fn arrange(hand: &[Card], idx: &mut [usize]) {
    let xmult = |c: &Card| c.enhancement == Some(Enhancement::Glass) || c.edition == Some(Edition::Polychrome);
    idx.sort_by_key(|&i| (xmult(&hand[i]), i));
}

/// Tries every 1–5 card subset of `hand` and returns the highest-scoring play.
/// Subsets with unscored kickers are skipped unless a joker or the boss cares about them.
pub fn best_play(b: &Board, hand: &[Card]) -> Option<Play> {
    let n = hand.len().min(16);
    let flags = b.rule_flags();
    let keep_kickers = kickers_matter(b);
    let mut best: Option<Play> = None;
    let mut played: Vec<Card> = Vec::with_capacity(5);
    let mut held: Vec<Card> = Vec::with_capacity(n);
    let mut idx: Vec<usize> = Vec::with_capacity(5);
    for mask in 1u32..(1 << n) {
        let k = mask.count_ones();
        if k > 5 {
            continue;
        }
        idx.clear();
        idx.extend((0..n).filter(|i| mask & (1 << i) != 0));
        arrange(hand, &mut idx);
        played.clear();
        played.extend(idx.iter().map(|&i| hand[i]));
        let info = hand::detect(&played, flags);
        if !keep_kickers && info.scoring.len() != played.len() {
            continue;
        }
        held.clear();
        held.extend((0..n).filter(|i| mask & (1 << i) == 0).map(|i| hand[i]));
        let o = score::score_detected(b, &played, &held, info, &mut Unlucky, false);
        if best.as_ref().is_none_or(|p| o.score > p.floor) {
            best = Some(Play { cards: idx.clone(), hand: o.hand, floor: o.score });
        }
    }
    best
}

/// Fisher–Yates.
pub fn shuffle<T>(v: &mut [T], rng: &mut Rng) {
    for i in (1..v.len()).rev() {
        let j = rng.below(i + 1);
        v.swap(i, j);
    }
}

/// Boss effects on a round simulation that the scoring engine doesn't cover.
#[derive(Debug, Clone, Default)]
pub struct RoundRules {
    /// Cards of this suit are debuffed (The Club/Goad/Window/Head).
    pub debuff_suit: Option<Suit>,
    /// Face cards are debuffed (The Plant).
    pub debuff_faces: bool,
    pub hand_size_delta: i64,
}

impl RoundRules {
    pub fn for_blind(key: &str) -> RoundRules {
        let mut r = RoundRules::default();
        match key {
            "bl_club" => r.debuff_suit = Some(Suit::Clubs),
            "bl_goad" => r.debuff_suit = Some(Suit::Spades),
            "bl_window" => r.debuff_suit = Some(Suit::Diamonds),
            "bl_head" => r.debuff_suit = Some(Suit::Hearts),
            "bl_plant" => r.debuff_faces = true,
            "bl_manacle" => r.hand_size_delta = -1,
            _ => {}
        }
        r
    }

    /// `Blind:debuff_card` for suit/face bosses.
    pub fn apply(&self, cards: &mut [Card], smeared: bool, pareidolia: bool) {
        for (i, c) in cards.iter_mut().enumerate() {
            let by_suit = self.debuff_suit.is_some_and(|s| hand::is_suit(c, s, true, false, smeared));
            let by_face = self.debuff_faces && hand::is_face(c, i, pareidolia, true);
            if by_suit || by_face {
                c.debuff = true;
            }
        }
    }
}

/// Where a round starts.
#[derive(Debug, Clone)]
pub struct RoundStart {
    /// Cards already in hand (empty for a fresh round).
    pub hand: Vec<Card>,
    /// The draw pile (shuffled by the simulation).
    pub deck: Vec<Card>,
    pub hand_size: i64,
    pub hands: i64,
    pub discards: i64,
    pub scored: f64,
    pub target: f64,
}

#[derive(Debug, Clone)]
pub struct RoundResult {
    pub total: f64,
    pub won: bool,
    pub best_hand: f64,
    /// Hands played (type, score, whether it was a junk hand played to dig).
    pub plays: Vec<(HandType, f64, bool)>,
}

fn draw(hand: &mut Vec<Card>, deck: &mut Vec<Card>, size: usize) {
    while hand.len() < size {
        match deck.pop() {
            Some(c) => hand.push(c),
            None => break,
        }
    }
}

/// Chance of holding `need` cards of a suit after `draws` digs, each replacing up to 5
/// off-suit cards (a discard, or a junk hand played to dig). Exact hypergeometric DP
/// over (held, suited left in deck, deck size); adapted from balatro-agent's
/// `tools/balatro_state.py::flush_odds`.
pub fn flush_odds(hold: usize, deck_suit: usize, deck_size: usize, hand_size: usize, need: usize, draws: usize) -> f64 {
    use std::collections::HashMap;
    fn comb(n: usize, k: usize) -> f64 {
        if k > n {
            return 0.0;
        }
        (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
    }
    let mut dist: HashMap<(usize, usize, usize), f64> = HashMap::from([((hold, deck_suit, deck_size), 1.0)]);
    for _ in 0..draws {
        let mut next: HashMap<(usize, usize, usize), f64> = HashMap::new();
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
enum Action {
    /// cards, and whether this is a junk hand played only to dig
    Play(Vec<usize>, bool),
    Discard(Vec<usize>),
}

/// The heuristic play/discard policy (labelled as a heuristic everywhere it shows):
/// - play the best hand if it wins, if it's the last hand, or if repeating it keeps pace;
/// - otherwise, if chasing a flush is worth more than the best hand (exact draw odds ×
///   what that flush would score), throw away off-suit cards: with a discard, or by
///   playing them as a junk hand once discards are gone;
/// - otherwise discard the cards outside the best play and hope to improve it.
fn decide(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize) -> Action {
    let Some(best) = best_play(b, hand) else { return Action::Play(vec![], false) };
    let play_best = Action::Play(best.cards.clone(), false);
    if best.floor >= need || hands <= 1 || deck.is_empty() {
        return play_best;
    }
    let on_pace = best.floor * hands as f64 >= need;
    let f = b.rule_flags();
    let need_f = if f.four_fingers { 4 } else { 5 };
    let suited = |c: &Card, s: Suit| hand::is_suit(c, s, false, true, f.smeared);
    let (suit, group) = Suit::ALL
        .iter()
        .map(|&s| (s, (0..hand.len()).filter(|&i| suited(&hand[i], s)).collect::<Vec<usize>>()))
        .max_by_key(|(_, g)| g.len())
        .unwrap_or((Suit::Spades, vec![]));
    let off: Vec<usize> = (0..hand.len()).filter(|i| !group.contains(i)).collect();

    // Flush plan: exact odds of completing it with the digs left × what it would score.
    if group.len() < need_f && group.len() >= 2 {
        let in_deck: Vec<&Card> = deck.iter().filter(|c| suited(c, suit)).collect();
        let digs = (discards + hands - 1).max(0) as usize;
        let p = flush_odds(group.len(), in_deck.len(), deck.len(), size, need_f, digs);
        if p > 0.1 && in_deck.len() + group.len() >= need_f {
            let mut cards: Vec<Card> = group.iter().map(|&i| hand[i]).collect();
            let mut extra: Vec<Card> = in_deck.iter().map(|c| **c).collect();
            extra.sort_by(|a, c| c.rank.chips().total_cmp(&a.rank.chips()));
            cards.extend(extra.into_iter().take(need_f - group.len()));
            cards.truncate(5);
            let est = score::score(b, &cards, &[], &mut Unlucky, false).score;
            // When on pace, only chase if the flush is clearly better AND the best hand
            // would spend 2+ of the suited cards (playing it would break the draw).
            let breaks_draw = best.hand != HandType::Flush && best.cards.iter().filter(|i| group.contains(i)).count() >= 2;
            let chase = if on_pace {
                breaks_draw && group.len() + 1 >= need_f && p * est > 1.5 * best.floor
            } else {
                p * est * (hands as f64 - 1.0).clamp(1.0, 2.0) > best.floor * hands as f64
            };
            if chase {
                let mut toss = off.clone();
                toss.sort_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips()));
                toss.truncate(5);
                if !toss.is_empty() {
                    if discards > 0 {
                        return Action::Discard(toss);
                    }
                    // No discards: dig with the best hand the off-suit cards make (so it still
                    // scores), topped up with off-suit kickers to throw away 5 cards.
                    let cards: Vec<Card> = off.iter().map(|&i| hand[i]).collect();
                    let mut dig: Vec<usize> = best_play(b, &cards).map(|p| p.cards.iter().map(|&k| off[k]).collect()).unwrap_or_default();
                    for i in toss {
                        if dig.len() >= 5 {
                            break;
                        }
                        if !dig.contains(&i) {
                            dig.push(i);
                        }
                    }
                    return Action::Play(dig, true);
                }
            }
        }
    }
    if on_pace {
        return play_best;
    }
    if discards > 0 {
        let mut toss: Vec<usize> = (0..hand.len()).filter(|i| !best.cards.contains(i)).collect();
        toss.sort_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips()));
        toss.truncate(5);
        if !toss.is_empty() {
            return Action::Discard(toss);
        }
    }
    play_best
}

/// Simulates the rest of a round with the `decide` policy.
pub fn sim_round(board: &Board, start: &RoundStart, rng: &mut Rng) -> RoundResult {
    let mut b = board.clone();
    let mut deck = start.deck.clone();
    shuffle(&mut deck, rng);
    let size = start.hand_size.max(1) as usize;
    let mut hand = start.hand.clone();
    draw(&mut hand, &mut deck, size);
    let (mut hands, mut discards) = (start.hands, start.discards);
    let mut total = start.scored;
    let mut best_hand: f64 = 0.0;
    let mut plays = Vec::new();
    while hands > 0 && !hand.is_empty() {
        b.hands_left = hands;
        b.discards_left = discards;
        b.deck_remaining = deck.len() as i64;
        match decide(&b, &hand, &deck, hands, discards, start.target - total, size) {
            Action::Play(mut idx, dig) => {
                if idx.is_empty() {
                    break;
                }
                idx.truncate(5);
                let played: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
                let held: Vec<Card> = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
                let o = score::score(&b, &played, &held, rng, false);
                total += o.score;
                best_hand = best_hand.max(o.score);
                plays.push((o.hand, o.score, dig));
                if b.blind.key == "bl_eye" {
                    b.blind.eye_seen |= o.hand.bit();
                }
                if b.blind.key == "bl_mouth" && b.blind.mouth_only.is_none() {
                    b.blind.mouth_only = Some(o.hand);
                }
                let lvl = &mut b.levels[o.hand as usize];
                lvl.played += 1;
                lvl.played_this_round += 1;
                b.hands_played += 1;
                hand = held;
                hands -= 1;
                if total >= start.target {
                    return RoundResult { total, won: true, best_hand, plays };
                }
                draw(&mut hand, &mut deck, size);
            }
            Action::Discard(idx) => {
                hand = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
                draw(&mut hand, &mut deck, size);
                discards -= 1;
            }
        }
    }
    RoundResult { total, won: total >= start.target, best_hand, plays }
}

/// Mean and quantiles of a sample.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Stats {
    pub mean: f64,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
    pub n: usize,
}

impl Stats {
    pub fn of(mut v: Vec<f64>) -> Stats {
        if v.is_empty() {
            return Stats::default();
        }
        v.sort_by(f64::total_cmp);
        let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        Stats { mean: v.iter().sum::<f64>() / v.len() as f64, p10: q(0.1), p50: q(0.5), p90: q(0.9), n: v.len() }
    }
}

/// Best-hand score on fresh deals from `deck` (the "how strong is this board" number).
pub fn typical_hands(b: &Board, deck: &[Card], hand_size: usize, samples: usize, seed: u64) -> Vec<f64> {
    typical_hands_detail(b, deck, hand_size, samples, seed).into_iter().map(|(s, _)| s).collect()
}

/// Like `typical_hands`, with the hand type each deal's best play was.
pub fn typical_hands_detail(b: &Board, deck: &[Card], hand_size: usize, samples: usize, seed: u64) -> Vec<(f64, HandType)> {
    let mut rng = Rng::new(seed);
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
                (score::score(&b, &played, &held, &mut rng, false).score, p.hand)
            })
        })
        .collect()
}

/// Chance of beating a round, from `sims` simulations (same seed → same deals across boards).
pub fn round_odds(b: &Board, start: &RoundStart, sims: usize, seed: u64) -> (f64, Stats) {
    let mut wins = 0;
    let mut totals = Vec::with_capacity(sims);
    for i in 0..sims {
        let mut rng = Rng::new(seed.wrapping_add(i as u64 * 7919));
        let r = sim_round(b, start, &mut rng);
        wins += usize::from(r.won);
        totals.push(r.total);
    }
    (wins as f64 / sims.max(1) as f64, Stats::of(totals))
}

/// Which hands score the points in simulated rounds: (hand, share of points, share of
/// hands played, mean score). Junk hands played to dig are left out.
pub fn round_hand_mix(b: &Board, start: &RoundStart, sims: usize, seed: u64) -> Vec<(HandType, f64, f64, f64)> {
    let mut pts = [0.0f64; 12];
    let mut cnt = [0usize; 12];
    for i in 0..sims {
        let mut rng = Rng::new(seed.wrapping_add(i as u64 * 7919));
        for (h, s, dig) in sim_round(b, start, &mut rng).plays {
            if !dig {
                pts[h as usize] += s;
                cnt[h as usize] += 1;
            }
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
