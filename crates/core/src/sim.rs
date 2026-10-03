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
    // Face-down cards can't be planned around (you don't know them): only as filler
    let hidden: u32 = (0..n).filter(|&i| hand[i].face_down).fold(0, |m, i| m | (1 << i));
    for mask in 1u32..(1 << n) {
        let k = mask.count_ones();
        if k > 5 || mask & hidden != 0 {
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
                    if total >= start.target || start.hands <= 1 {
                        if total >= start.target {
                            o.won = 1.0;
                            o.spare = (start.hands - 1) as f64;
                            o.last = Some(s.hand);
                            o.seen = bb.goals.as_ref().map_or(0.0, |g| g.seen_gain(&hand));
                            o.planets = seal_planets(&bb, &held);
                            o.cash += held_dollars(&held);
                        }
                        o.total = total;
                        return o;
                    }
                    RoundStart { hand: held, deck, hands: start.hands - 1, scored: total, ..start.clone() }
                }
                Move::Discard(idx) => {
                    o.cash += discard_money(&bb, &hand, idx);
                    let kept: Vec<Card> = (0..hand.len()).filter(|k| !idx.contains(k)).map(|k| hand[k]).collect();
                    RoundStart { hand: kept, deck, discards: (start.discards - 1).max(0), ..start.clone() }
                }
            };
            let r = sim_round_uses(b, &next, &mut rng, uses);
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

/// Money discarding `idx` from `hand` pays (the engine's discard effects).
pub fn discard_money(b: &Board, hand: &[Card], idx: &[usize]) -> f64 {
    let cards: Vec<Card> = idx.iter().filter_map(|&i| hand.get(i).copied()).collect();
    crate::engine::discard_money(b, &cards)
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
    let keep = pays_at_end(b, hand);
    if !keep.is_empty() && hands > 1 {
        let free: Vec<usize> = (0..hand.len()).filter(|i| !keep.contains(i)).collect();
        let cards: Vec<Card> = free.iter().map(|&i| hand[i]).collect();
        if best_play(b, &cards).is_some_and(|p| p.floor * hands as f64 >= need) {
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
    let now = score::score(b, &played, &held, &mut Unlucky, false).score;
    score::score(&none, &played, &held, &mut Unlucky, false).score > now * 1.001
}

/// The heuristic play/discard policy (labelled as a heuristic everywhere it shows):
/// - while safe, cash the discard that pays most (`discard_money`); the last discard right
///   before the round ends;
/// - with discards left and a best play that scores more with none left, discard first;
/// - play the best hand if it wins, if it's the last hand, or if repeating it keeps pace;
/// - otherwise, if chasing a flush is worth more than the best hand (exact draw odds ×
///   what that flush would score), throw away off-suit cards: with a discard, or by
///   playing them as a junk hand once discards are gone;
/// - otherwise discard the cards outside the best play and hope to improve it.
/// `burn`: whether to check if burning discards pays (off inside that check itself).
#[allow(clippy::too_many_arguments)]
fn decide_cards(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize, burn: bool) -> Action {
    let Some(best) = best_play(b, hand) else { return Action::Play(vec![], false) };
    let play_best = Action::Play(with_fillers(b, hand, &best.cards), false);
    // Discards that pay money (`discard_money`, the engine's discard effects): while the
    // round is safe (on pace with this hand), cash the discard that pays most among those
    // whose remaining hand still keeps you on pace (held cards count too: Steel, Baron's
    // Kings). With one discard left, cash it right before the winning hand (or the last
    // one): more paying cards may come by then. A threshold, not yet weighed against the
    // round's value (`RoundGoals`): see the register in design.md.
    let all: Vec<usize> = (0..hand.len()).collect();
    let now = discards > 1 || best.floor >= need || hands <= 1;
    if now && discards > 0 && best.floor * hands as f64 >= need && discard_money(b, hand, &all) > 0.0 {
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
            best_play(b, &rest).is_some_and(|p| p.floor * hands as f64 >= need)
        };
        if let Some((v, _)) = paying.into_iter().take(16).find(|(v, _)| keeps_pace(v)) {
            return Action::Discard(v);
        }
    }
    if best.floor >= need || hands <= 1 || deck.is_empty() {
        return play_best;
    }
    let on_pace = best.floor * hands as f64 >= need;
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
    let f = b.rule_flags();
    let need_f = if f.four_fingers { 4 } else { 5 };
    let suited = |c: &Card, s: Suit| hand::is_suit(c, s, false, true, f.smeared);
    // Flush plan: for each suit you could chase, the exact odds of completing it with the
    // digs left (from what's in hand and what's left in the draw pile) × what it would
    // score; the best suit is the one to chase, not simply the one you hold most of.
    let digs = (discards + hands - 1).max(0) as usize;
    let plan = Suit::ALL
        .iter()
        .filter_map(|&suit| {
            let group: Vec<usize> = (0..hand.len()).filter(|&i| suited(&hand[i], suit)).collect();
            if group.len() >= need_f || group.len() < 2 {
                return None;
            }
            let in_deck: Vec<&Card> = deck.iter().filter(|c| suited(c, suit)).collect();
            if in_deck.len() + group.len() < need_f {
                return None;
            }
            let p = flush_odds(group.len(), in_deck.len(), deck.len(), size, need_f, digs);
            let mut cards: Vec<Card> = group.iter().map(|&i| hand[i]).collect();
            // The cards you'd draw: typical ones of the suit (the middle of what's left), not
            // the best; with the best, a suit you hold fewer of looked better than it is.
            let mut extra: Vec<Card> = in_deck.iter().map(|c| **c).collect();
            extra.sort_by(|a, c| c.rank.chips().total_cmp(&a.rank.chips()));
            let want = need_f - group.len();
            let start = extra.len().saturating_sub(want) / 2;
            cards.extend(extra.into_iter().skip(start).take(want));
            cards.truncate(5);
            // Average over a few rolls, so Lucky cards count for their average, not for nothing
            let mut rolls = Rng::new(0x1d1e ^ suit as u64);
            let est = (0..4).map(|_| score::score(b, &cards, &[], &mut rolls, false).score).sum::<f64>() / 4.0;
            Some((group, p, est))
        })
        .max_by(|a, c| (a.1 * a.2).total_cmp(&(c.1 * c.2)));
    if let Some((group, p, est)) = plan {
        let off: Vec<usize> = (0..hand.len()).filter(|i| !group.contains(i)).collect();
        if p > 0.1 {
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
fn play_on_instead(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, scored: f64, target: f64, size: usize, win: &[usize], uses: &[Use], rng: &mut Rng) -> Option<Action> {
    let g = b.goals.as_ref()?;
    if hands <= 1 {
        return None;
    }
    let played: Vec<Card> = win.iter().map(|&i| hand[i]).collect();
    let held: Vec<Card> = (0..hand.len()).filter(|i| !win.contains(i)).map(|i| hand[i]).collect();
    let h = score::score(b, &played, &held, &mut Unlucky, false).hand;
    let planets = seal_planets(b, &held);
    let now = 1.0 + g.planet[h as usize] * planets + g.dollar * ((hands - 1) as f64 * g.per_hand + held_dollars(&held));
    // the most a different finish could add: the best planet for the seals kept
    let best = g.planet.iter().copied().fold(0.0, f64::max);
    if (best - g.planet[h as usize]) * planets < 0.01 {
        return None;
    }
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
    let mut plain = b.clone();
    plain.goals = None;
    let start = RoundStart { hand: hand.to_vec(), deck: deck.to_vec(), hand_size: size as i64, hands, discards, scored, target };
    let seed = rng.next_u64();
    let outs = outcomes_after(&plain, &start, &alt, 0..LOOKAHEAD_ROLLOUTS, seed, uses);
    let later = outs.iter().map(|o| g.value(o)).sum::<f64>() / outs.len() as f64;
    (later > now).then(|| match alt {
        Move::Play(v) => Action::Play(v, true),
        Move::Discard(v) => Action::Discard(v),
    })
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

/// Uses a held consumable once it improves the best play in hand (by more than 1%).
fn use_if_better(b: &mut Board, hand: &mut Vec<Card>, uses: &[Use], used: &mut [bool]) {
    for (k, u) in uses.iter().enumerate() {
        if used[k] {
            continue;
        }
        let Some((b2, h2)) = u.apply(b, hand) else { continue };
        let now = best_play(b, hand).map_or(0.0, |p| p.floor);
        if best_play(&b2, &h2).is_some_and(|p| p.floor > now * 1.01) {
            *b = b2;
            *hand = h2;
            used[k] = true;
        }
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
    let mut seen_by: Vec<(String, f64)> = vec![];
    note_seen(&b, &hand, &mut seen_by);
    while hands > 0 && !hand.is_empty() {
        b.hands_left = hands;
        b.discards_left = discards;
        b.deck_remaining = deck.len() as i64;
        use_if_better(&mut b, &mut hand, uses, &mut used);
        if let Some(g) = &b.goals {
            seen_by.retain(|(k, _)| g.seen.iter().any(|(k2, _, _)| k2 == k));
        }
        let act = match win_keeping_seals(&b, &hand, start.target - total) {
            Some(v) => play_on_instead(&b, &hand, &deck, hands, discards, total, start.target, size, &v, uses, rng).unwrap_or(Action::Play(v, false)),
            None => decide(&b, &hand, &deck, hands, discards, start.target - total, size),
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
                    money += held_dollars(&hand);
                    let seen = seen_by.iter().map(|e| e.1).fold(0.0, f64::max);
                    return RoundResult { total, won: true, saved: false, best_hand, plays, money, hands_left: hands, planets: seal_planets(&b, &hand), seen };
                }
                draw(&mut hand, &mut deck, size);
                note_seen(&b, &hand, &mut seen_by);
            }
            Action::Discard(idx) => {
                money += discard_money(&b, &hand, &idx);
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
        std::thread::scope(|sc| {
            let hs: Vec<_> = (0..threads)
                .map(|t| sc.spawn(move || (t * chunk..((t + 1) * chunk).min(sims)).map(run_one).collect::<Vec<_>>()))
                .collect();
            hs.into_iter().flat_map(|h| h.join().expect("sim thread")).collect()
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
        let deck: Vec<Card> = standard_deck().into_iter().filter(|c| !hand.iter().any(|h| h.rank == c.rank && h.suit == c.suit)).collect();
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
    fn discards_that_pay_are_cashed_while_safe() {
        // Money for discards comes from the engine (`discard_money`), the policy names no
        // joker: Mail-In pays per card of its rank, Faceless Joker for 3+ faces at once.
        let deck: Vec<Card> = standard_deck();
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
