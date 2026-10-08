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
    /// Score with every random roll failing (used to choose, and to know a play surely wins).
    pub floor: f64,
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
/// Polychrome) last, so multipliers apply to everything before them. Heuristic, on the retire
/// list: the engine arranges the cards itself (`engine::score`) and keeps this order only when
/// its arrangement doesn't score more as played.
fn arrange(hand: &[Card], idx: &mut [usize]) {
    let xmult = |c: &Card| c.enhancement == Some(Enhancement::Glass) || c.edition == Some(Edition::Polychrome);
    idx.sort_by_key(|&i| (xmult(&hand[i]), i));
}

/// Tries every 1–5 card subset of `hand` and returns the highest-scoring play.
/// Subsets with unscored kickers are skipped unless a joker or the boss cares about them.
pub fn best_play(b: &Board, hand: &[Card]) -> Option<Play> {
    best_play_with(b, hand, true)
}

/// `best_play`; `prefilter`: skip the plays `could_all_score` rules out before detecting them
fn best_play_with(b: &Board, hand: &[Card], prefilter: bool) -> Option<Play> {
    let n = hand.len().min(16);
    best_play_of(b, hand, (1 << n) - 1, prefilter, None).map(|(mut p, rolled)| {
        // a floor that rolled nothing is every roll's score (with the same cards held: the
        // whole hand, unless it's more than 16 cards)
        p.mean = if rolled || n < hand.len() { mean_score(b, hand, &p.cards, p.floor) } else { p.floor };
        p
    })
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
}

/// `best_play_with` of the cards `part` (a mask over `hand`'s first 16), without its mean: the
/// play's cards are indices into `hand`, and whether its floor asked for a roll (`Watched`).
/// `seen`: what's known of the plays of `hand` (by mask), filled in as it goes.
fn best_play_of(b: &Board, hand: &[Card], part: u32, prefilter: bool, mut seen: Option<&mut [Option<SeenPlay>]>) -> Option<(Play, bool)> {
    let n = hand.len().min(16);
    let flags = b.rule_flags();
    let keep_kickers = kickers_matter(b);
    let mut best: Option<(Play, bool)> = None;
    let mut played: Vec<Card> = Vec::with_capacity(5);
    let mut held: Vec<Card> = Vec::with_capacity(n);
    let mut idx: Vec<usize> = Vec::with_capacity(5);
    // Face-down cards can't be planned around (you don't know them): only as filler
    let hidden: u32 = (0..n).filter(|&i| hand[i].face_down).fold(0, |m, i| m | (1 << i));
    // every play within the part, in increasing order of its mask
    let mut mask = 0u32;
    loop {
        mask = mask.wrapping_sub(part) & part;
        if mask == 0 {
            break;
        }
        let k = mask.count_ones();
        if k > 5 || mask & hidden != 0 {
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
        let (floor, hand_type, asked) = match known.as_ref().and_then(|k| k.floor).filter(|f| held_mask & !f.held == 0) {
            Some(f) => (f.score, f.hand, f.asked),
            None => {
                held.clear();
                held.extend((0..n).filter(|i| held_mask & (1 << i) != 0).map(|i| hand[i]));
                let mut rolls = Watched { rolls: Unlucky, asked: false };
                let o = score::score_detected(b, &played, &held, info.clone(), &mut rolls, false);
                if let Some(k) = known.as_mut().filter(|k| k.floor.is_none() && !o.held_used) {
                    k.floor = Some(SeenFloor { held: held_mask, score: o.score, hand: o.hand, asked: rolls.asked });
                }
                (o.score, o.hand, rolls.asked)
            }
        };
        if best.as_ref().is_none_or(|(p, _)| floor > p.floor) {
            best = Some((Play { cards: idx.clone(), hand: hand_type, floor, mean: floor }, asked));
        }
    }
    best
}

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
}

/// The best plays of parts of one hand (a decision's: the hand each plan's dig leaves, the
/// cards a junk hand is made of), each part worked out once, each play's hand detected once
/// for every part it's in, and its floor scored once for every part it's in when no held card
/// takes part. The same results as `best_play` of the part's cards.
struct HandParts<'a> {
    b: &'a Board,
    hand: &'a [Card],
    /// what's known of the plays, by mask (nothing for a hand of more than `PARTS_MAX` cards)
    detected: std::cell::RefCell<Vec<Option<SeenPlay>>>,
    done: std::cell::RefCell<Vec<(u32, Option<Play>)>>,
}

/// The largest hand `HandParts` shares detections for (2^n of them)
const PARTS_MAX: usize = 12;

impl<'a> HandParts<'a> {
    fn new(b: &'a Board, hand: &'a [Card]) -> Self {
        let size = if hand.len() <= PARTS_MAX { 1 << hand.len() } else { 0 };
        HandParts { b, hand, detected: std::cell::RefCell::new(vec![None; size]), done: std::cell::RefCell::new(vec![]) }
    }

    /// `best_play` of the cards at `idx` (indices into the hand, in order): the play's cards
    /// as indices into the hand
    fn best(&self, idx: &[usize]) -> Option<Play> {
        let (b, hand) = (self.b, self.hand);
        let rest: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
        if hand.len() > PARTS_MAX {
            return best_play(b, &rest).map(|mut p| {
                p.cards = p.cards.iter().map(|&k| idx[k]).collect();
                p
            });
        }
        let part: u32 = idx.iter().fold(0, |m, &i| m | 1 << i);
        if let Some((_, p)) = self.done.borrow().iter().find(|e| e.0 == part) {
            return p.clone();
        }
        let p = best_play_of(b, hand, part, true, Some(&mut self.detected.borrow_mut())).map(|(mut p, rolled)| {
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
/// hand detection: Stone cards always do (hand.lua `evaluate_play`); of the rest, 2 or 3 can
/// only all score as one rank (a Pair, Three of a Kind: no flush or straight is that short),
/// and 4 without Four Fingers only as Two Pair or Four of a Kind; 5 without Four Fingers only
/// as a flush (hand.rs `flush`: one suit, a Wild card or Smeared Joker's colours aside), a
/// straight (5 ranks) or two ranks at most (Full House, Five of a Kind). Splash isn't checked
/// here (`best_play` doesn't call this with it). A shortcut: `hand::detect` decides.
fn could_all_score(hand: &[Card], mask: u32, n: usize, flags: hand::RuleFlags) -> bool {
    if flags.splash {
        return true;
    }
    let mut cards = [0usize; 5];
    let mut k = 0;
    for i in 0..n {
        if mask & (1 << i) != 0 && hand[i].enhancement != Some(Enhancement::Stone) {
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

/// Every play of 1 to 5 cards, each in the order to play it. Face-down cards are left out:
/// you can't plan around a card you can't see.
pub fn all_plays(hand: &[Card]) -> Vec<Move> {
    let n = hand.len().min(12);
    let hidden: u32 = (0..n).filter(|&i| hand[i].face_down).fold(0, |m, i| m | (1 << i));
    (1u32..(1 << n))
        .filter(|m| m.count_ones() <= 5 && m & hidden == 0)
        .map(|m| {
            let mut idx: Vec<usize> = (0..n).filter(|i| m & (1 << i) != 0).collect();
            arrange(hand, &mut idx);
            Move::Play(idx)
        })
        .collect()
}

/// Every discard of 1 to 5 cards from the hand (none without a discard left), for screening
/// all of them instead of guessing which are worth simulating.
pub fn all_discards(hand: &[Card], discards: i64) -> Vec<Move> {
    if discards <= 0 {
        return vec![];
    }
    let n = hand.len().min(12);
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
}

/// The rounds numbered `range` after `first` (round i always draws the same cards, whatever
/// the move, so moves compare on the same draws and more rounds can be added later).
pub fn outcomes_after(b: &Board, start: &RoundStart, first: &Move, range: std::ops::Range<usize>, seed: u64, uses: &[Use]) -> Vec<Outcome> {
    let size = start.hand_size.max(1) as usize;
    range
        .map(|i| {
            let mut rng = Rng::new(seed.wrapping_add(i as u64 * 7919));
            let mut deck = start.deck.clone();
            shuffle(&mut deck, &mut rng);
            let mut hand = start.hand.clone();
            draw(&mut hand, &mut deck, size);
            let mut bb = b.clone();
            bb.hands_left = start.hands;
            bb.discards_left = start.discards;
            bb.deck_remaining = deck.len() as i64;
            let mut o = Outcome::default();
            let next = match first {
                Move::Play(idx) => {
                    let played: Vec<Card> = idx.iter().filter_map(|&k| hand.get(k).copied()).collect();
                    let held: Vec<Card> = (0..hand.len()).filter(|k| !idx.contains(k)).map(|k| hand[k]).collect();
                    let s = score::score(&bb, &played, &held, &mut rng, false);
                    o.cash += s.dollars;
                    let total = start.scored + s.score;
                    bb.after_hand(&s);
                    if total >= start.target || start.hands <= 1 {
                        if total >= start.target {
                            o.won = 1.0;
                            o.spare = (start.hands - 1) as f64;
                            o.last = Some(s.hand);
                            o.seen = bb.goals.as_ref().map_or(0.0, |g| g.seen_gain(&hand).max(g.carried.iter().map(|e| e.1).fold(0.0, f64::max)));
                            o.planets = seal_planets(&bb, &held);
                            o.cash += won_money(&bb, &held);
                        }
                        o.total = total;
                        return o;
                    }
                    RoundStart { hand: held, deck, hands: start.hands - 1, scored: total, ..start.clone() }
                }
                Move::Discard(idx) => {
                    o.cash += bb.discard(&picked(&hand, idx));
                    let kept: Vec<Card> = (0..hand.len()).filter(|k| !idx.contains(k)).map(|k| hand[k]).collect();
                    RoundStart { hand: kept, deck, discards: (start.discards - 1).max(0), ..start.clone() }
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
            o
        })
        .collect()
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
        if let Some(p) = best_play(b, &cards).filter(|p| p.floor >= need) {
            return Some(p.cards.iter().map(|&j| free[j]).collect());
        }
    }
    None
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
/// (`held_dollars`) and the engine's `won_round_money` (Delayed Gratification).
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
    /// Planets from Blue Seal cards held at the end of a won round.
    pub planets: f64,
    /// The most a card drawn this round adds (`RoundGoals::seen`)
    pub seen: f64,
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
    use std::cell::RefCell;
    use std::collections::HashMap;
    // The same situations recur constantly across simulations, so cache per thread.
    thread_local! {
        static CACHE: RefCell<HashMap<(usize, usize, usize, usize, usize, usize), f64>> = RefCell::new(HashMap::new());
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

fn flush_odds_uncached(hold: usize, deck_suit: usize, deck_size: usize, hand_size: usize, need: usize, draws: usize) -> f64 {
    // a BTreeMap: the sum below in a fixed order, so the odds are the same to the last bit
    // every time (a HashMap's order differs per map, and near-ties between plans flipped)
    use std::collections::BTreeMap;
    fn comb(n: usize, k: usize) -> f64 {
        if k > n {
            return 0.0;
        }
        (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
    }
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
#[derive(Debug)]
enum Action {
    /// cards, and whether this is a junk hand played only to dig
    Play(Vec<usize>, bool),
    Discard(Vec<usize>),
}

/// Cards that pay at the end of the round while held (`pays_at_end`) stay out of play as long
/// as the rest of the hand keeps you on pace for the target; then the usual policy decides
/// with the rest.
fn decide(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize) -> Action {
    // every simulated decision: where an analysis asked to stop does (`progress`)
    crate::progress::checkpoint();
    let keep = pays_at_end(b, hand);
    if !keep.is_empty() && hands > 1 {
        let free: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i)).collect();
        let cards: Vec<Card> = free.iter().map(|&i| hand[i]).collect();
        if best_play(b, &cards).is_some_and(|p| p.mean * hands as f64 >= need) {
            return match decide_cards(b, &cards, deck, hands, discards, need, size.saturating_sub(keep.len()).max(1), true) {
                Action::Play(v, d) => Action::Play(v.into_iter().map(|k| free[k]).collect(), d),
                Action::Discard(v) => Action::Discard(v.into_iter().map(|k| free[k]).collect()),
            };
        }
    }
    decide_cards(b, hand, deck, hands, discards, need, size, true)
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
    if b.discards_left <= 0 || play.is_empty() {
        return false;
    }
    let mut none = b.clone();
    none.discards_left = 0;
    let played: Vec<Card> = play.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !play.contains(i)).map(|i| hand[i]).collect();
    let info = hand::detect(&played, b.rule_flags());
    let now = score::score_detected(b, &played, &held, info.clone(), &mut Unlucky, false).score;
    score::score_detected(&none, &played, &held, info, &mut Unlucky, false).score > now * 1.001
}

/// The heuristic play/discard policy (labelled as a heuristic everywhere it shows):
/// - while safe, cash the discard that pays most (`discard_money`); the last discard right
///   before the round ends;
/// - with discards left and a best play that scores more with none left, discard first;
/// - play the best hand if it surely wins, if it's the last hand, or if repeating it keeps
///   pace (its average score: `Play::mean`);
/// - otherwise, if digging for a hand (`aims`: a flush, a straight, a straight flush, one
///   more of a rank, a Full House) is worth more than playing on (`chase`: the round's
///   expected points, dig by dig as this policy plays it), throw away the cards outside it:
///   with a discard, or by playing them as a junk hand once discards are gone;
/// - otherwise discard the cards outside the best play and hope to improve it.
/// Cards that add to the best play while held (`held_value`) are never thrown away.
/// `burn`: whether to check if burning discards pays (off inside that check itself).
#[allow(clippy::too_many_arguments)]
fn decide_cards(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize, burn: bool) -> Action {
    // the best plays of this hand and of its parts (`HandParts`)
    let parts = HandParts::new(b, hand);
    let whole: Vec<usize> = (0..hand.len()).collect();
    let Some(best) = parts.best(&whole) else { return Action::Play(vec![], false) };
    let play_best = Action::Play(with_fillers(b, hand, &best.cards), false);
    // Discards that pay money (`discard_money`, the engine's discard effects): while the
    // round is safe (on pace with this hand), cash the discard that pays most among those
    // whose remaining hand still keeps you on pace (held cards count too: Steel, Baron's
    // Kings). With one discard left, cash it right before the winning hand (or the last
    // one): more paying cards may come by then. A threshold, not yet weighed against the
    // round's value (`RoundGoals`): see the register in design.md.
    let all: Vec<usize> = (0..hand.len()).collect();
    let now = discards > 1 || best.floor >= need || hands <= 1;
    if now && discards > 0 && best.mean * hands as f64 >= need && discard_money(b, hand, &all) > 0.0 {
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
            best_play(b, &rest).is_some_and(|p| p.mean * hands as f64 >= need)
        };
        if let Some((v, _)) = paying.into_iter().take(16).find(|(v, _)| keeps_pace(v)) {
            return Action::Discard(v);
        }
    }
    if best.floor >= need || hands <= 1 || deck.is_empty() {
        return play_best;
    }
    let on_pace = best.mean * hands as f64 >= need;
    // When your best play scores more with no discards left (the engine knows which jokers
    // pay that way), discard before playing. Which cards: what the policy digs with when it's
    // behind (a flush chase when the odds are there, else what's outside the best play). The
    // discards are spent anyway, so keeping a draw costs nothing; being on pace only decides
    // whether to spend a discard, which the burn already has. Nothing to throw: play.
    if burn && discards > 0 && burn_pays(b, hand, &best.cards) {
        if let Action::Discard(v) = decide_cards(b, hand, deck, hands, discards, f64::INFINITY, size, false) {
            return Action::Discard(v);
        }
        return play_best;
    }
    // Dig plans (`aims`): every hand you could dig for, each chased dig by dig as this policy
    // would (`chase`): the one whose chase is worth most, when that beats playing on.
    // the cards worth holding (`held_value`), worked out only when needed
    let held_cell = std::cell::OnceCell::new();
    let held_keep = || held_cell.get_or_init(|| held_value(b, hand, &best.cards));
    // When on pace, a hand is only chased if it's one card away and the best hand would spend
    // 2+ of its cards (playing it would break the draw); then only if clearly better
    // (`ON_PACE_MARGIN`).
    let eligible = |a: &Aim| {
        let breaks_draw = best.hand != a.hand && best.cards.iter().filter(|i| a.keep.contains(i)).count() >= 2;
        let one_away = a.groups.iter().map(|g| g.1).sum::<usize>() == 1;
        !on_pace || (breaks_draw && one_away)
    };
    let all = aims(b, hand, deck);
    // a plan's score, on its middle completion (the screen) or all of them (`aim_score`),
    // worked out once each
    let quick: Vec<std::cell::OnceCell<f64>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let quick_of = |k: usize| *quick[k].get_or_init(|| aim_quick(b, &all[k], hand, held_keep()));
    let full: Vec<std::cell::OnceCell<f64>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let full_of = |k: usize| *full[k].get_or_init(|| aim_score(b, &all[k], hand, held_keep()));
    let round = ChaseRound { pile: deck.len(), size, hands, discards, need, best: best.mean, on_pace };
    // the first dig for each plan (`dig_for`), and the hand it leaves (its best play: what a
    // failed chase is left with)
    let digs: Vec<std::cell::OnceCell<Option<(Vec<usize>, bool)>>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let dig_of = |k: usize| digs[k].get_or_init(|| dig_for(&parts, &all[k].keep, held_keep(), discards));
    let left: Vec<std::cell::OnceCell<f64>> = all.iter().map(|_| std::cell::OnceCell::new()).collect();
    let left_of = |k: usize| {
        *left[k].get_or_init(|| {
            let thrown = dig_of(k).as_ref().map_or(&[][..], |d| &d.0[..]);
            let rest: Vec<usize> = (0..hand.len()).filter(|i| !thrown.contains(i)).collect();
            parts.best(&rest).map_or(0.0, |p| p.mean)
        })
    };
    // each plan's chase (`chase`): with the lesser plans it keeps alive (a lesser hand whose
    // cards it keeps), always on their full estimate (a middle completion can be far off: a
    // flush draw's may be a straight flush), and the hand it leaves
    let run = |k: usize, score: &dyn Fn(usize) -> f64| {
        let lesser: Vec<(&Aim, f64)> = (0..all.len())
            .filter(|&j| (all[j].hand as usize) > (all[k].hand as usize) && all[j].keep.iter().all(|i| all[k].keep.contains(i)))
            .map(|j| (&all[j], full_of(j)))
            .collect();
        chase(&all[k], score(k), left_of(k), &lesser, &round)
    };
    // screened on one completion each; the best few get the full estimate
    // (a plan with nothing to throw can't be chased)
    let mut plan = (0..all.len()).filter(|&k| eligible(&all[k]) && dig_of(k).is_some()).map(|k| (k, run(k, &quick_of))).collect::<Vec<_>>();
    plan.sort_by(|a, c| c.1.value.total_cmp(&a.1.value));
    plan.truncate(AIM_FINALISTS);
    let plan = plan
        .into_iter()
        .map(|(k, _)| (k, run(k, &full_of)))
        .filter(|(_, c)| c.go)
        .max_by(|a, c| a.1.value.total_cmp(&c.1.value));
    if let Some((k, _)) = plan {
        if let Some((v, junk)) = dig_of(k).clone() {
            return if junk { Action::Play(v, true) } else { Action::Discard(v) };
        }
    }
    if on_pace {
        return play_best;
    }
    if discards > 0 {
        let mut toss: Vec<usize> = (0..hand.len()).filter(|i| !best.cards.contains(i) && !held_keep().contains(i)).collect();
        toss.sort_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips()));
        toss.truncate(5);
        if !toss.is_empty() {
            return Action::Discard(toss);
        }
    }
    play_best
}

/// The first dig for a plan keeping `keep`: up to 5 of the lowest cards outside it, never
/// those worth holding (`held`), thrown with a discard; once discards are gone, played as a
/// junk hand: the best hand the cards it may throw make (so it still scores), topped up with
/// the lowest of them to 5. The cards it removes from the hand, and whether it's a junk hand;
/// none when there's nothing to throw.
fn dig_for(parts: &HandParts, keep: &[usize], held: &[usize], discards: i64) -> Option<(Vec<usize>, bool)> {
    let hand = parts.hand;
    let free: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i) && !held.contains(i)).collect();
    let mut toss = free.clone();
    toss.sort_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips()));
    toss.truncate(5);
    if toss.is_empty() {
        return None;
    }
    if discards > 0 {
        return Some((toss, false));
    }
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

thread_local! {
    /// Futures per alternative for the oracle player (0: off, the default)
    static ORACLE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static IN_ROLLOUT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A yardstick for the simulated player, never used by the advice: with `rollouts` > 0, every
/// decision in this thread's simulated rounds that isn't a win on the table compares the
/// policy's move with the alternatives from the hand's structure (`oracle_moves`), each on
/// `rollouts` futures played on by the policy, and takes one only when it's clearly better on
/// the same futures (paired, 2 standard errors). Hundreds of times slower than the policy:
/// for measuring how much it leaves on the table (`tests/player.rs`).
pub fn set_oracle(rollouts: usize) {
    ORACLE.with(|o| o.set(rollouts));
}

#[allow(clippy::too_many_arguments)]
fn oracle_decide(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, scored: f64, target: f64, size: usize, uses: &[Use], rng: &mut Rng) -> Option<Action> {
    let r = ORACLE.with(|o| o.get());
    if r < 2 || IN_ROLLOUT.with(|x| x.get()) || hand.is_empty() {
        return None;
    }
    let base = decide(b, hand, deck, hands, discards, target - scored, size);
    let mut cands: Vec<Move> = vec![match &base {
        Action::Play(v, _) => Move::Play(v.clone()),
        Action::Discard(v) => Move::Discard(v.clone()),
    }];
    let norm = |m: &Move| match m {
        Move::Play(v) => (0, { let mut v = v.clone(); v.sort(); v }),
        Move::Discard(v) => (1, { let mut v = v.clone(); v.sort(); v }),
    };
    for m in oracle_moves(b, hand, deck, discards) {
        if !cands.iter().any(|x| norm(x) == norm(&m)) {
            cands.push(m);
        }
    }
    if cands.len() <= 1 {
        return None;
    }
    let start = RoundStart { hand: hand.to_vec(), deck: deck.to_vec(), hand_size: size as i64, hands, discards, scored, target };
    let seed = rng.next_u64();
    IN_ROLLOUT.with(|x| x.set(true));
    let g = b.goals.clone();
    let vals: Vec<Vec<f64>> = cands
        .iter()
        .map(|m| outcomes_after(b, &start, m, 0..r, seed, uses).iter().map(|o| g.as_ref().map_or(o.won, |g| g.value(o))).collect())
        .collect();
    IN_ROLLOUT.with(|x| x.set(false));
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
/// as `aims` reads them (hand.lua): suits as flushes count them (`hand::is_suit`: Wild cards,
/// Smeared Joker), runs of 4 with Four Fingers and with Shortcut's gaps, ranks as straights
/// and sets see them (`hand::card_id`: Stone cards have none).
fn oracle_moves(b: &Board, hand: &[Card], deck: &[Card], discards: i64) -> Vec<Move> {
    let mut out = vec![];
    let Some(best) = best_play(b, hand) else { return out };
    out.push(Move::Play(with_fillers(b, hand, &best.cards)));
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
    sets.push(best.cards);
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
    hand: HandType,
    keep: Vec<usize>,
    groups: Vec<(Vec<Card>, usize)>,
}

/// Every hand worth digging for, from the game's own hand rules (hand.lua `get_flush`,
/// `get_straight`, `get_X_same`, with Four Fingers, Shortcut and Smeared Joker): a flush of
/// each suit with 2+ cards of it; each straight with all but 1 or 2 of its ranks in hand
/// (one card of each kept), and each straight flush (a suit's flush part with a straight,
/// hand.lua `evaluate_poker_hand`); one more of a rank you hold 2+ of (Three, Four, Five of a Kind);
/// a Full House from Two Pair. Hands already made aren't aims (the best play has them).
fn aims(b: &Board, hand: &[Card], deck: &[Card]) -> Vec<Aim> {
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
            out.push(Aim { hand: Flush, keep, groups: vec![(fits, n)] });
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
    let mut straights: Vec<(Vec<usize>, Vec<i32>, Vec<Vec<i32>>)> = vec![];
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
                out.push(Aim { hand: Straight, keep, groups: vec![(g, 1)] });
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
            out.push(Aim { hand: Straight, keep, groups });
        }
    }
    // Straight flushes, by the game's rule: a flush part and a straight part found apart
    // (hand.lua `evaluate_poker_hand`; with Four Fingers each is 4 of the 5 cards played), so a
    // play holds `need` cards of the suit and at most 5 − need others. For each suit and
    // straight: the suit's copy of each rank, another only within that allowance, else the rank
    // is drawn; then the suit's other cards in hand, as many as there's room for (the flush
    // part). One card away, its fits are the cards that make a straight flush with what's kept
    // (`hand::detect`); two away, a card of each missing rank, as many of them of the suit as
    // the flush part still lacks.
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
            // the flush part: the suit's other cards, the highest, while there's room
            let mut more: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i) && in_suit(&hand[*i])).collect();
            more.sort_by(|&a, &c| hand[c].rank.chips().total_cmp(&hand[a].rank.chips()).then(a.cmp(&c)));
            more.truncate(5 - keep.len() - missing.len().max(1));
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
                out.push(Aim { hand: StraightFlush, keep: keep.clone(), groups: vec![(g, 1)] });
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
                out.push(Aim { hand: StraightFlush, keep: keep.clone(), groups });
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
            out.push(Aim { hand: more, keep, groups: vec![(g, 1)] });
        }
    }
    for (k, &r1) in pairs.iter().enumerate() {
        for &r2 in &pairs[k + 1..] {
            let mut keep = rank_of(r1);
            keep.extend(rank_of(r2));
            let mut g = fits(r1);
            g.extend(fits(r2));
            if !g.is_empty() {
                out.push(Aim { hand: FullHouse, keep, groups: vec![(g, 1)] });
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

/// Every run of `need` ranks a straight can be (hand.lua `get_straight`): consecutive ranks,
/// or with Shortcut a gap of one rank between any two; an Ace is 1 or 14. Computed once per
/// rule set.
fn straight_runs(need: usize, shortcut: bool) -> std::rc::Rc<Vec<Vec<i32>>> {
    thread_local! {
        static RUNS: std::cell::RefCell<Vec<((usize, bool), std::rc::Rc<Vec<Vec<i32>>>)>> = const { std::cell::RefCell::new(vec![]) };
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
/// it. A flush: the exact draw (`flush_odds`: the suited cards you draw stay, so later digs
/// see fewer). Others: the cards the digs would see if each refilled every slot outside the
/// aim (up to 5), taken as one draw from the pile (`draw_odds`). As if every one of them went
/// to this plan: `chase` asks it dig by dig.
fn aim_odds(aim: &Aim, pile: usize, size: usize, digs: usize) -> f64 {
    if aim.hand == HandType::Flush {
        let (fits, n) = &aim.groups[0];
        return flush_odds(aim.keep.len(), fits.len(), pile, size, aim.keep.len() + n, digs);
    }
    let seen = (digs * size.saturating_sub(aim.keep.len()).min(5)).min(pile);
    let groups: Vec<(usize, usize)> = aim.groups.iter().map(|g| (g.0.len(), g.1)).collect();
    draw_odds(&groups, pile, seen)
}

/// The round a chase is played out in: the draw pile, hand size, hands and discards left, the
/// score still needed, the best play's average and whether it keeps pace.
#[derive(Clone, Copy)]
struct ChaseRound {
    pile: usize,
    size: usize,
    hands: i64,
    discards: i64,
    need: f64,
    best: f64,
    on_pace: bool,
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
/// at every dig), then the best play's average: a simpler test than the policy's own at its
/// next decision, which sees the new cards and, on pace, its margin and rules. At each dig, the plan is made with its
/// chance given the digs before failed (`aim_odds` between them); else a lesser plan it keeps
/// alive (`lesser`: every card it keeps, the chase keeps) can be made first, and ends it when
/// it puts the round on pace (its score × the hands left after the dig); else the chase goes
/// on, or stops with the hand it leaves (`left`: the best play of the hand the first dig leaves). A
/// hand made is played once, the hands after it at the best play's average, and a junk hand
/// scores nothing; playing on now is the best play every hand. A lesser plan's chance per
/// dig: its cards among the dig's new ones (other than the plan's own fits; cards drawn
/// toward it in earlier digs aren't counted, the chase throws them away), the best of them
/// when several turn up. On pace, the chase must beat playing on by `ON_PACE_MARGIN` of the
/// best play.
fn chase(aim: &Aim, score: f64, left: f64, lesser: &[(&Aim, f64)], r: &ChaseRound) -> Chase {
    let t = r.size.saturating_sub(aim.keep.len()).min(5);
    let digs = (r.discards + r.hands - 1).max(0) as usize;
    // made within j digs, for j up to every dig
    let made: Vec<f64> = (0..=digs).map(|j| aim_odds(aim, r.pile, r.size, j)).collect();
    let fits: Vec<&Card> = aim.groups.iter().flat_map(|g| g.0.iter()).collect();
    // each lesser plan's groups without the plan's own fits, the best first
    let mut lesser: Vec<(Vec<(usize, usize)>, f64)> = lesser
        .iter()
        .map(|(l, sc)| (l.groups.iter().map(|g| (g.0.iter().filter(|c| !fits.contains(c)).count(), g.1)).collect(), *sc))
        .collect();
    lesser.sort_by(|a, c| c.1.total_cmp(&a.1));
    // the hands left at dig s (a junk hand once the discards are gone)
    let hands_at = |s: usize| if s as i64 >= r.discards { r.hands - (s as i64 - r.discards) } else { r.hands };
    // playing on at dig s instead of digging
    let stay = |s: usize| {
        let h = hands_at(s) as f64;
        if s == 0 { r.best * h } else { left + (h - 1.0) * r.best }
    };
    let mut out = Chase { value: stay(0), go: false };
    // the chase from the next dig on, when the policy goes on with it
    let mut next: Option<f64> = None;
    for s in (0..digs).rev() {
        let (f0, f1) = (made[s], made[s + 1]);
        let hit = if f0 < 1.0 { ((f1 - f0) / (1.0 - f0)).clamp(0.0, 1.0) } else { 1.0 };
        let hands = hands_at(s);
        let after = if s as i64 >= r.discards { hands - 1 } else { hands };
        let ends = |x: f64| x + (after - 1).max(0) as f64 * r.best;
        let pile = r.pile.saturating_sub(t * s).saturating_sub(fits.len());
        let (mut miss, mut stop) = (1.0, 0.0);
        for (groups, sc) in &lesser {
            if sc * (after as f64) < r.need {
                continue;
            }
            let q = draw_odds(groups, pile, t.min(pile));
            stop += miss * q * ends(*sc);
            miss *= 1.0 - q;
        }
        let value = hit * ends(score) + (1.0 - hit) * (stop + miss * next.unwrap_or_else(|| stay(s + 1)));
        let margin = if s == 0 && r.on_pace { ON_PACE_MARGIN * r.best } else { 0.0 };
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
    fn comb(n: usize, k: usize) -> f64 {
        if k > n {
            return 0.0;
        }
        (0..k).fold(1.0, |acc, i| acc * (n - i) as f64 / (i + 1) as f64)
    }
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

/// The screen for `aim_score`: its first completion only, every random roll failing.
fn aim_quick(b: &Board, aim: &Aim, hand: &[Card], held: &[usize]) -> f64 {
    let kept: Vec<Card> = held.iter().filter(|i| !aim.keep.contains(i)).map(|&i| hand[i]).collect();
    score::score(b, &aim_middle(aim, hand), &kept, &mut Unlucky, false).score
}

/// The middle one of `aim`'s completions (the screen's and the random check's: not the best,
/// not the worst)
fn aim_middle(aim: &Aim, hand: &[Card]) -> Vec<Card> {
    let mut all = aim_completions(aim, hand);
    let k = all.len() / 2;
    all.swap_remove(k).0
}

/// Random completions drawn for a plan more than one card away
const AIM_FILLS: usize = 4;

/// The ways `aim` can be completed, each with its weight: with one card missing from each
/// group (at most two groups), every distinct card that fits, weighted by its copies (exact);
/// otherwise `AIM_FILLS` draws at
/// random from what fits (a fixed seed: the same draws for every call). Not a fixed "typical"
/// card: the middle ones are neighbouring ranks, so a flush draw looked like a straight flush;
/// spread ones never are, though with Four Fingers and Shortcut one often is.
fn aim_completions(aim: &Aim, hand: &[Card]) -> Vec<(Vec<Card>, f64)> {
    let base: Vec<Card> = aim.keep.iter().map(|&i| hand[i]).collect();
    let distinct = |fits: &[Card]| -> Vec<(Card, f64)> {
        let mut v: Vec<(Card, f64)> = vec![];
        for c in fits {
            match v.iter_mut().find(|x| x.0.same_kind(c)) {
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
    for c in &mut out {
        c.0.truncate(5);
    }
    out
}

/// What completing `aim` is expected to score: the weighted average over its completions
/// (`aim_completions`), with the cards worth holding (`held`) held, random effects at their
/// average (`MEAN_ROLLS` rolls in all) when the middle completion or any Lucky card in it is
/// random. Only the cards that complete it count, not what else the chase leaves in hand (see
/// the register).
fn aim_score(b: &Board, aim: &Aim, hand: &[Card], held: &[usize]) -> f64 {
    let kept: Vec<Card> = held.iter().filter(|i| !aim.keep.contains(i)).map(|&i| hand[i]).collect();
    let all = aim_completions(aim, hand);
    // each completion's hand detected once
    let flags = b.rule_flags();
    let infos: Vec<hand::HandInfo> = all.iter().map(|x| hand::detect(&x.0, flags)).collect();
    let (mid, mid_info) = (&all[all.len() / 2].0, &infos[all.len() / 2]);
    let lucky = |c: &Card| c.enhancement == Some(Enhancement::Lucky);
    // the middle completion's floor, when worked out for the check (it's its score when nothing
    // is random); a floor that asked for no roll (`Watched`) is its ceiling too
    let mut mid_floor = None;
    let random = all.iter().any(|x| x.0.iter().any(lucky)) || kept.iter().any(lucky) || {
        let mut w = Watched { rolls: Unlucky, asked: false };
        let floor = score::score_detected(b, mid, &kept, mid_info.clone(), &mut w, false).score;
        mid_floor = Some(floor);
        w.asked && score::score_detected(b, mid, &kept, mid_info.clone(), &mut crate::engine::Lucky, false).score > floor
    };
    let mut rolls = Rng::new(0x1d1e);
    let per = if random { MEAN_ROLLS.div_ceil(all.len()) } else { 1 };
    let (mut total, mut weight) = (0.0, 0.0);
    for (k, ((cards, w), info)) in all.iter().zip(&infos).enumerate() {
        for _ in 0..per {
            total += w * match (random, mid_floor) {
                (true, _) => score::score_detected(b, cards, &kept, info.clone(), &mut rolls, false).score,
                (false, Some(f)) if k == all.len() / 2 => f,
                (false, _) => score::score_detected(b, cards, &kept, info.clone(), &mut Unlucky, false).score,
            };
            weight += w;
        }
    }
    total / weight
}

/// The cards of `hand` outside `play` that add to its score while held (Baron's Kings, Steel
/// cards, Shoot the Moon's Queens, …: the engine's held effects): thrown away, they'd cost
/// the play now. One scoring per card.
fn held_value(b: &Board, hand: &[Card], play: &[usize]) -> Vec<usize> {
    let played: Vec<Card> = play.iter().map(|&i| hand[i]).collect();
    let rest: Vec<usize> = (0..hand.len()).filter(|i| !play.contains(i)).collect();
    let held = |skip: Option<usize>| -> Vec<Card> { rest.iter().filter(|&&i| Some(i) != skip).map(|&i| hand[i]).collect() };
    let info = hand::detect(&played, b.rule_flags());
    let all = score::score_detected(b, &played, &held(None), info.clone(), &mut Unlucky, false).score;
    rest.iter().copied().filter(|&i| score::score_detected(b, &played, &held(Some(i)), info.clone(), &mut Unlucky, false).score < all).collect()
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
    pub add: Vec<Card>,
    pub levels: [i64; 12],
    /// A Planet card: Constellation grows ×0.1 when it's used
    pub planet: bool,
}

impl Use {
    pub fn apply(&self, b: &Board, hand: &[Card]) -> Option<(Board, Vec<Card>)> {
        let mut h = hand.to_vec();
        let mut done = vec![false; h.len()];
        for (from, to) in &self.swap {
            let i = (0..h.len()).find(|&i| !done[i] && h[i].same_kind(from))?;
            h[i] = Card { debuff: h[i].debuff, face_down: h[i].face_down, ..*to };
            done[i] = true;
        }
        h.extend(self.add.iter().copied());
        let mut b = b.clone();
        if let Some(g) = b.goals.as_mut() {
            g.seen.retain(|(k, _, _)| k != &self.key);
        }
        b.playing_cards += self.add.len() as i64;
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
}

/// What a won round is worth beyond winning it, in the advice's long-run measure: each planet
/// a Blue Seal makes, by the hand played last (it's that hand's planet), and each dollar.
/// The round simulation uses it to choose between winning now and playing on for more.
#[derive(Debug, Clone, Default)]
pub struct RoundGoals {
    pub planet: [f64; 12],
    pub dollar: f64,
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
}

impl RoundGoals {
    /// A simulated round's value: 0 if lost, else 1 plus what it leaves you.
    pub fn value(&self, o: &Outcome) -> f64 {
        let planet = o.last.map_or(0.0, |h| self.planet[h as usize]);
        o.won * (1.0 + planet * o.planets + self.dollar * (o.spare * self.per_hand + o.cash) + o.seen)
    }

    /// The most a card in `cards` adds (`seen`)
    pub fn seen_gain(&self, cards: &[Card]) -> f64 {
        cards.iter().flat_map(|c| self.seen.iter().filter(move |(_, k, _)| k.same_kind(c)).map(|(_, _, g)| *g)).fold(0.0, f64::max)
    }
}

/// Simulated futures per choice when the round simulation weighs winning now against
/// playing on.
const LOOKAHEAD_ROLLOUTS: usize = 8;

/// A win is on the table: is playing on for a better finish worth more? Compares winning now
/// with `win` against the move the policy would make if it weren't finishing (digging with
/// the cards that don't pay at round end), on a few simulated futures, by `RoundGoals`. Only
/// when a better finish could be worth something; the futures play on without looking ahead.
#[allow(clippy::too_many_arguments)]
/// `finish`: the finish with its uses (`finish_with_uses`: board, hand, winning play, value),
/// when there is one, else winning now with `win`; `seen_by`: the round's `seen` credit so far.
#[allow(clippy::type_complexity)]
fn play_on_instead(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, scored: f64, target: f64, size: usize, win: &[usize], finish: Option<(&Board, &[Card], &[usize], f64)>, seen_by: &[(String, f64)], uses: &[Use], rng: &mut Rng) -> Option<Action> {
    let g = b.goals.as_ref()?;
    if hands <= 1 || g.no_lookahead {
        return None;
    }
    // the finish as it would be played (with its uses)
    let (fb, fh, fw) = finish.map_or((b, hand, win), |f| (f.0, f.1, f.2));
    let played: Vec<Card> = fw.iter().map(|&i| fh[i]).collect();
    let held: Vec<Card> = (0..fh.len()).filter(|i| !fw.contains(i)).map(|i| fh[i]).collect();
    let h = score::score(fb, &played, &held, &mut Unlucky, false).hand;
    let planets = seal_planets(fb, &held);
    // the most a different finish could add: the best planet for the seals kept
    let best = g.planet.iter().copied().fold(0.0, f64::max);
    if (best - g.planet[h as usize]) * planets < 0.01 {
        return None;
    }
    let seen = seen_by.iter().map(|e| e.1).fold(0.0, f64::max);
    let now = finish.map_or_else(|| finish_value(b, g, hand, win, hands, seen), |f| f.3);
    // what the policy does when not finishing now, with the cards that pay at round end kept
    let keep = pays_at_end(b, hand);
    let free: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i)).collect();
    let cards: Vec<Card> = free.iter().map(|&i| hand[i]).collect();
    let alt = match decide_cards(b, &cards, deck, hands, discards, f64::INFINITY, size.saturating_sub(keep.len()).max(1), true) {
        Action::Play(v, _) => Move::Play(v.into_iter().map(|k| free[k]).collect()),
        Action::Discard(v) => Move::Discard(v.into_iter().map(|k| free[k]).collect()),
    };
    if let Move::Play(v) = &alt {
        let mut a = v.clone();
        let mut w = win.to_vec();
        a.sort();
        w.sort();
        if a == w || v.is_empty() {
            return None;
        }
    }
    // the futures play to the same goals (the finish included), without looking ahead again
    let mut plain = b.clone();
    if let Some(pg) = plain.goals.as_mut() {
        pg.no_lookahead = true;
        pg.carried = seen_by.to_vec();
    }
    let start = RoundStart { hand: hand.to_vec(), deck: deck.to_vec(), hand_size: size as i64, hands, discards, scored, target };
    let seed = rng.next_u64();
    let outs = outcomes_after(&plain, &start, &alt, 0..LOOKAHEAD_ROLLOUTS, seed, uses);
    let later = outs.iter().map(|o| g.value(o)).sum::<f64>() / outs.len() as f64;
    (later > now).then(|| match alt {
        Move::Play(v) => Action::Play(v, true),
        Move::Discard(v) => Action::Discard(v),
    })
}

/// What winning now with `win` is worth (`RoundGoals::value`, the measure every move is ranked
/// by): the planets of the Blue Seals left in hand (the planet of the hand played), the money
/// it pays and the Gold cards' held, the hands left over, and the best card drawn for a
/// consumable still held (`seen`).
fn finish_value(b: &Board, g: &RoundGoals, hand: &[Card], win: &[usize], hands: i64, seen: f64) -> f64 {
    let played: Vec<Card> = win.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !win.contains(i)).map(|i| hand[i]).collect();
    let s = score::score(b, &played, &held, &mut Unlucky, false);
    let o = Outcome { won: 1.0, spare: (hands - 1) as f64, cash: s.dollars + won_money(b, &held), planets: seal_planets(b, &held), last: Some(s.hand), seen, ..Default::default() };
    g.value(&o)
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
            let Some(w2) = win_keeping_seals(&b2, &h2, need) else { continue };
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

/// Notes, per consumable, the best `RoundGoals::seen` gain among the cards in `hand`.
fn note_seen(b: &Board, hand: &[Card], acc: &mut Vec<(String, f64)>) {
    let Some(g) = &b.goals else { return };
    for (k, card, gain) in &g.seen {
        if hand.iter().any(|c| c.same_kind(card)) {
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
/// (`finish_with_uses`) it's worth more. Others gain nothing by waiting.
fn use_if_better(b: &mut Board, hand: &mut Vec<Card>, uses: &[Use], used: &mut [bool], need: f64, hands: i64) {
    while used.iter().any(|u| !u) {
        let now = best_play(b, hand).map_or(0.0, |p| p.mean);
        let on_pace = now * hands.max(1) as f64 >= need;
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
        let Some((_, k, b2, h2)) = best else { return };
        *b = b2;
        *hand = h2;
        used[k] = true;
    }
}

/// `sim_round` with consumables held (see `Use`).
pub fn sim_round_uses(board: &Board, start: &RoundStart, rng: &mut Rng, uses: &[Use]) -> RoundResult {
    let mut used = vec![false; uses.len()];
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
    let mut money = 0.0;
    // Cards worth drawing (`RoundGoals::seen`), per consumable: the best gain drawn so far. A
    // consumable used during the round (on its own fixed targets) takes its gain with it.
    let mut seen_by: Vec<(String, f64)> = b.goals.as_ref().map_or(vec![], |g| g.carried.clone());
    note_seen(&b, &hand, &mut seen_by);
    while hands > 0 && !hand.is_empty() {
        b.hands_left = hands;
        b.discards_left = discards;
        b.deck_remaining = deck.len() as i64;
        use_if_better(&mut b, &mut hand, uses, &mut used, start.target - total, hands);
        drop_used_seen(&b, &mut seen_by);
        let act = match win_keeping_seals(&b, &hand, start.target - total) {
            Some(v) => {
                // the finish, with any held consumable that makes it worth more used first
                let fin = b.goals.as_ref().filter(|_| used.iter().any(|u| !u)).and_then(|g| finish_with_uses(&b, g, &hand, &v, uses, &used, start.target - total, hands, &seen_by));
                let unused: Vec<Use> = uses.iter().zip(&used).filter(|(_, u)| !**u).map(|(x, _)| x.clone()).collect();
                let finish = fin.as_ref().map(|f| (&f.0, f.1.as_slice(), f.2.as_slice(), f.4));
                match play_on_instead(&b, &hand, &deck, hands, discards, total, start.target, size, &v, finish, &seen_by, &unused, rng) {
                    Some(a) => a,
                    None => match fin {
                        Some((b2, h2, v2, ks, _)) => {
                            b = b2;
                            hand = h2;
                            ks.into_iter().for_each(|k| used[k] = true);
                            drop_used_seen(&b, &mut seen_by);
                            Action::Play(v2, false)
                        }
                        None => Action::Play(v, false),
                    },
                }
            }
            None => {
                // the yardstick (`set_oracle`), when it's on
                let oracle = (ORACLE.with(|o| o.get()) > 0)
                    .then(|| {
                        let unused: Vec<Use> = uses.iter().zip(&used).filter(|(_, u)| !**u).map(|(x, _)| x.clone()).collect();
                        oracle_decide(&b, &hand, &deck, hands, discards, total, start.target, size, &unused, rng)
                    })
                    .flatten();
                oracle.unwrap_or_else(|| decide(&b, &hand, &deck, hands, discards, start.target - total, size))
            }
        };
        match act {
            Action::Play(mut idx, dig) => {
                if idx.is_empty() {
                    break;
                }
                idx.truncate(5);
                let played: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
                let held: Vec<Card> = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
                let o = score::score(&b, &played, &held, rng, false);
                total += o.score;
                money += o.dollars;
                best_hand = best_hand.max(o.score);
                plays.push((o.hand, o.score, dig));
                b.after_hand(&o);
                hand = held;
                hands -= 1;
                if total >= start.target {
                    money += won_money(&b, &hand);
                    let seen = seen_by.iter().map(|e| e.1).fold(0.0, f64::max);
                    return RoundResult { total, won: true, saved: false, best_hand, plays, money, hands_left: hands, planets: seal_planets(&b, &hand), seen };
                }
                draw(&mut hand, &mut deck, size);
                note_seen(&b, &hand, &mut seen_by);
            }
            Action::Discard(idx) => {
                money += b.discard(&picked(&hand, &idx));
                hand = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
                draw(&mut hand, &mut deck, size);
                discards -= 1;
                note_seen(&b, &hand, &mut seen_by);
            }
        }
    }
    // Mr. Bones: a lost round is saved if you reached 25% of the blind (card.lua, game_over)
    let bones = b.jokers.iter().any(|j| j.key == "j_mr_bones" && !j.debuff);
    let won = total >= start.target;
    RoundResult { total, won, saved: !won && bones && total >= 0.25 * start.target, best_hand, plays, money, hands_left: 0, planets: 0.0, seen: seen_by.iter().map(|e| e.1).fold(0.0, f64::max) }
}

/// Mean and quantiles of a sample.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Stats {
    pub mean: f64,
    pub p10: f64,
    pub p50: f64,
    pub p90: f64,
    pub n: usize,
    /// Mean money earned while scoring, per round (round simulations only).
    #[serde(skip)]
    pub money: f64,
    /// Chance of getting through with Mr. Bones' save counted (round simulations only).
    #[serde(skip)]
    pub p_saved: f64,
}

impl Stats {
    pub fn of(mut v: Vec<f64>) -> Stats {
        if v.is_empty() {
            return Stats::default();
        }
        v.sort_by(f64::total_cmp);
        let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
        Stats { mean: v.iter().sum::<f64>() / v.len() as f64, p10: q(0.1), p50: q(0.5), p90: q(0.9), n: v.len(), money: 0.0, p_saved: 0.0 }
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
    let mut st = Stats::of(results.into_iter().map(|r| r.total).collect());
    st.money = money;
    st.p_saved = (wins + saved) as f64 / sims.max(1) as f64;
    (wins as f64 / sims.max(1) as f64, st)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::{sample_board, standard_deck};

    #[test]
    fn the_simulated_player_burns_discards_only_when_the_board_scores_more_without_them() {
        // On pace with a Pair, so without a reason to discard the player plays it. With
        // Mystic Summit the best play scores more with no discards left: discard first.
        let hand = Card::parse_list("AS AH KD 9C 7S 5H 3D 2C").unwrap();
        // no Aces left to draw: no Three of a Kind to dig for while on pace
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit) && c.rank.0 != 14).collect();
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
    fn a_straight_flush_is_a_plan_of_its_own() {
        // A straight flush is its own hand (scored at its own level): the game finds its flush
        // part and straight part apart (hand.lua `evaluate_poker_hand`)
        let plans = |cards: &str, keys: &[&str]| -> Vec<(Vec<usize>, Vec<String>)> {
            let hand = Card::parse_list(cards).unwrap();
            let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.same_kind(c))).collect();
            aims(&sample_board(keys), &hand, &deck)
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
    fn a_chase_is_worked_out_dig_by_dig_against_playing_on() {
        // a plan whose fits are `fits` groups of `n` cards in a 44-card pile, 4 cards kept of 8
        let aim = |groups: &[usize]| Aim {
            hand: HandType::StraightFlush,
            keep: vec![0, 1, 2, 3],
            groups: groups.iter().map(|&n| (standard_deck()[..n].to_vec(), 1)).collect(),
        };
        let round = |best: f64, need: f64, discards: i64| ChaseRound { pile: 44, size: 8, hands: 4, discards, need, best, on_pace: best * 4.0 >= need };
        // one card away, 5 fits: one dig makes it 39% of the time, too little on its own to
        // beat playing on, but the policy digs again after a miss, so six digs make it nearly
        // always: worth chasing (a chase decided on one dig's odds would never start)
        let one = aim(&[5]);
        assert!((aim_odds(&one, 44, 8, 1) - 0.394).abs() < 0.01);
        let c = chase(&one, 8652.0, 74.0, &[], &round(3312.0, 21360.0, 3));
        assert!(c.go && c.value > 4.0 * 3312.0, "{}", c.value);
        // two cards away, 3 fits each, with 1 discard: the rule before took every dig left
        // (the discard and 3 junk hands) as spent on it (56%, × score × 2 against the best × 4)
        // and chased it; dig by dig, each junk hand costs a hand and a failed chase leaves only
        // what it kept (here High Card), so it isn't worth more than playing on
        let two = aim(&[3, 3]);
        let p = aim_odds(&two, 44, 8, 4);
        assert!(p * 1000.0 * 2.0 > 180.0 * 4.0, "{p}");
        let c = chase(&two, 1000.0, 20.0, &[], &round(180.0, 876.0, 1));
        assert!(!c.go && c.value < 180.0 * 4.0, "{}", c.value);
        // the lesser hand its cards make on the way (a Flush on pace) is where it often ends,
        // and is valued there
        let flush = Aim { hand: HandType::Flush, keep: vec![0, 1, 2, 3], groups: vec![(standard_deck()[10..19].to_vec(), 1)] };
        let with = chase(&two, 1000.0, 20.0, &[(&flush, 300.0)], &round(180.0, 876.0, 1));
        assert!(with.value > c.value, "{} {}", with.value, c.value);
    }

    #[test]
    fn a_dig_leaves_the_cards_worth_holding() {
        // Baron: the Kings score while held, so a dig for the Hearts never throws them, with a
        // discard or as a junk hand, and the hand a failed chase is left with still has them
        let b = sample_board(&["j_baron"]);
        let hand = Card::parse_list("KS KC 2H 5H 9H 3C 4D 8S").unwrap();
        let hearts = [2, 4, 5];
        let held = held_value(&b, &hand, &[]);
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
        // random effects score
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
            for t in 0..100 {
                let mut deck = standard_deck();
                shuffle(&mut deck, &mut rng);
                let mut hand: Vec<Card> = deck[..6 + t % 5].to_vec();
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
                }
                for _ in 0..12 {
                    let m = rng.below(1 << hand.len()) as u32 | 1;
                    let idx: Vec<usize> = (0..hand.len()).filter(|i| m & (1 << i) != 0).collect();
                    let cards: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
                    let want = best_play(&b, &cards).map(|p| (p.cards.iter().map(|&k| idx[k]).collect::<Vec<_>>(), p.hand, p.floor, p.mean));
                    let got = parts.best(&idx).map(|p| (p.cards, p.hand, p.floor, p.mean));
                    assert_eq!(got, want, "{keys:?} {hand:?} {idx:?}");
                }
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
        // no Aces left to draw: the Pair of Aces has no Three of a Kind to dig for, so only
        // the money decides
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| c.rank.0 != 14).collect();
        let decide_on = |keys: &[&str], hand: &str, discards: i64, need_mult: f64| {
            let hand = Card::parse_list(hand).unwrap();
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
        let start = RoundStart { hand: hand.clone(), deck, hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1.0 };
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
    fn the_round_plays_on_the_board_a_discard_leaves() {
        // Hit the Road grows ×0.5 for each Jack discarded (card.lua, discard context): the
        // hands after that discard score with it, on the same draws
        let hand = Card::parse_list("JS JH JD JC 2S 3H 5D 7C").unwrap();
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit)).collect();
        let start = RoundStart { hand, deck, hand_size: 8, hands: 2, discards: 1, scored: 0.0, target: 1e12 };
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
        let start = RoundStart { hand, deck, hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 1e9 };
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
        let start = RoundStart { hand: vec![], deck: standard_deck(), hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 900.0 };
        let (_, st) = round_odds(&b, &start, 64, 11u64.wrapping_add(16 * 7919));
        let one = round_results(&b, &start, 16..80, 11);
        assert!((one.iter().map(|r| r.total).sum::<f64>() / 64.0 - st.mean).abs() < 1e-6);
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
        let start = RoundStart { hand, deck, hand_size: 8, hands: 4, discards: 3, scored: 0.0, target: 100.0 };
        let r = sim_round_uses(&b, &start, &mut Rng::new(3), &[copies]);
        assert!(r.won && r.planets >= 3.0, "planets {}", r.planets);
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
            let start = RoundStart { hand, deck, hand_size: 8, hands: 4, discards: 0, scored: 0.0, target: pair * 2.5 };
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
