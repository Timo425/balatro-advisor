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

/// First moves worth simulating: the best few plays by score with every roll failing and
/// by average score (Lucky rolls averaged), the best play holding back each special card
/// (enhanced, sealed or with an edition), and what the usual policy would do (which may
/// be a dig or a discard).
pub fn candidate_moves(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize) -> Vec<Move> {
    let n = hand.len().min(12);
    let flags = b.rule_flags();
    let keep_kickers = kickers_matter(b);
    // (mask, arranged cards, floor, mean)
    let mut all: Vec<(u32, Vec<usize>, f64, f64)> = Vec::new();
    let hidden: u32 = (0..n)
        // face-down cards can't be planned around; Blue Seal cards are kept for their planet
        .filter(|&i| hand[i].face_down || hand[i].seal == Some(crate::model::Seal::Blue))
        .fold(0, |m, i| m | (1 << i));
    for mask in 1u32..(1 << n) {
        if mask.count_ones() > 5 || mask & hidden != 0 {
            continue;
        }
        let mut idx: Vec<usize> = (0..n).filter(|i| mask & (1 << i) != 0).collect();
        arrange(hand, &mut idx);
        let played: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
        let info = hand::detect(&played, flags);
        if !keep_kickers && info.scoring.len() != played.len() {
            continue;
        }
        let held: Vec<Card> = (0..n).filter(|i| mask & (1 << i) == 0).map(|i| hand[i]).collect();
        let floor = score::score_detected(b, &played, &held, info.clone(), &mut Unlucky, false).score;
        let mut rng = Rng::new(0x5eed ^ mask as u64);
        let mean = (0..6).map(|_| score::score_detected(b, &played, &held, info.clone(), &mut rng, false).score).sum::<f64>() / 6.0;
        all.push((mask, idx, floor, mean));
    }
    let mut out: Vec<Move> = Vec::new();
    let push = |m: Move, out: &mut Vec<Move>| {
        let key = |m: &Move| match m {
            Move::Play(v) => (0, { let mut v = v.clone(); v.sort(); v }),
            Move::Discard(v) => (1, { let mut v = v.clone(); v.sort(); v }),
        };
        if !out.iter().any(|x| key(x) == key(&m)) {
            out.push(m);
        }
    };
    let mut by_floor: Vec<&(u32, Vec<usize>, f64, f64)> = all.iter().collect();
    by_floor.sort_by(|a, b| b.2.total_cmp(&a.2));
    for c in by_floor.iter().take(4) {
        push(Move::Play(c.1.clone()), &mut out);
        // the same play, topped up with junk to dig
        push(Move::Play(with_fillers(b, hand, &c.1)), &mut out);
    }
    let mut by_mean = by_floor.clone();
    by_mean.sort_by(|a, b| b.3.total_cmp(&a.3));
    for c in by_mean.iter().take(3) {
        push(Move::Play(c.1.clone()), &mut out);
    }
    // Hold one special card back for later
    for k in 0..n {
        let c = &hand[k];
        let special = !c.debuff && (c.enhancement.is_some() || c.seal.is_some() || c.edition.is_some());
        if special {
            if let Some(best) = by_floor.iter().find(|x| x.0 & (1 << k) == 0) {
                push(Move::Play(best.1.clone()), &mut out);
            }
        }
    }
    // A dig play: 5 junk cards (outside the suit your jokers reward, else your deck's commonest;
    // no sealed or enhanced cards), so a whole hand's worth of new cards comes in
    {
        let keep = keep_suit(b, hand, deck);
        let mut junk: Vec<usize> = (0..n)
            .filter(|&i| {
                let c = &hand[i];
                Some(c.suit) != keep && c.seal.is_none() && c.enhancement.is_none() && c.edition.is_none()
            })
            .collect();
        junk.sort_by(|&x, &y| hand[x].rank.chips().total_cmp(&hand[y].rank.chips()));
        junk.truncate(5);
        if junk.len() >= 3 {
            push(Move::Play(junk), &mut out);
        }
    }
    // Already won: keep the smallest play that still wins and dig with up to 5 of the rest
    // (the score past the target is worth nothing; new cards might be)
    if discards > 0 {
        let winners: Vec<&(u32, Vec<usize>, f64, f64)> = all.iter().filter(|x| x.2 >= need).collect();
        if let Some(least) = winners.iter().min_by_key(|x| (x.1.len(), std::cmp::Reverse((x.2 * 100.0) as i64))) {
            let mut toss: Vec<usize> = (0..n).filter(|i| least.0 & (1 << i) == 0).filter(|&i| hand[i].seal.is_none() && hand[i].enhancement.is_none() && hand[i].edition.is_none()).collect();
            toss.sort_by(|&x, &y| hand[x].rank.chips().total_cmp(&hand[y].rank.chips()));
            toss.truncate(5);
            if !toss.is_empty() {
                push(Move::Discard(toss), &mut out);
            }
        }
    }
    // A plain dig: discard Mail-In's rank (it pays) and the weakest cards outside the best play,
    // keeping the suit your jokers reward and sealed or enhanced cards
    if discards > 0 {
        if let Some(best) = by_floor.first() {
            let keep = keep_suit(b, hand, deck);
            let pays = |i: usize| b.mail_rank == Some(hand[i].rank.0);
            let mut toss: Vec<usize> = (0..n)
                .filter(|i| best.0 & (1 << i) == 0)
                .filter(|&i| pays(i) || (Some(hand[i].suit) != keep && hand[i].seal.is_none() && hand[i].enhancement.is_none() && hand[i].edition.is_none()))
                .collect();
            toss.sort_by(|&x, &y| {
                let pays = |i: usize| b.mail_rank == Some(hand[i].rank.0);
                pays(y).cmp(&pays(x)).then(hand[x].rank.chips().total_cmp(&hand[y].rank.chips()))
            });
            toss.truncate(5);
            if !toss.is_empty() {
                push(Move::Discard(toss.clone()), &mut out);
            }
            // Mail-In's rank pays even when it's in the best play: try cashing those too, and
            // let the win chance say whether the round can spare them
            let all_pay: Vec<usize> = (0..n).filter(|&i| pays(i)).collect();
            if all_pay.iter().any(|i| !toss.contains(i)) {
                let mut with: Vec<usize> = all_pay.clone();
                with.extend(toss.iter().copied().filter(|i| !all_pay.contains(i)));
                with.truncate(5);
                push(Move::Discard(with), &mut out);
            }
        }
    }
    match decide(b, hand, deck, hands, discards, need, size) {
        Action::Play(idx, _) if !idx.is_empty() => push(Move::Play(idx), &mut out),
        Action::Discard(idx) if !idx.is_empty() => push(Move::Discard(idx), &mut out),
        _ => {}
    }
    out
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

/// Chance to win the round after making `first`, then playing on with the usual policy,
/// and the mean round total. Same seeds for every move, so moves compare on the same draws.
/// Also the mean number of hands left over when it's won (each pays at cash out).
pub fn odds_after(b: &Board, start: &RoundStart, first: &Move, sims: usize, seed: u64) -> (f64, f64, f64, f64) {
    let (p, mean, spare, cash, _) = odds_after_uses(b, start, first, sims, seed, &[]);
    (p, mean, spare, cash)
}

/// `odds_after` with consumables held: the rest of the round may use them (see `Use`). Also
/// the mean number of planets Blue Seal cards held at the end make (0 in a lost round).
pub fn odds_after_uses(b: &Board, start: &RoundStart, first: &Move, sims: usize, seed: u64, uses: &[Use]) -> (f64, f64, f64, f64, f64) {
    let outs = outcomes_after(b, start, first, 0..sims, seed, uses);
    let n = sims.max(1) as f64;
    let sum = |f: fn(&Outcome) -> f64| outs.iter().map(f).sum::<f64>() / n;
    (sum(|o| o.won), sum(|o| o.total), sum(|o| o.spare), sum(|o| o.cash), sum(|o| o.planets))
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

/// Money a discard pays: Mail-In Rebate's $5 per card of its rank (card.lua discard).
pub fn discard_money(b: &Board, hand: &[Card], idx: &[usize]) -> f64 {
    let Some(rank) = b.mail_rank else { return 0.0 };
    let jokers = b.jokers.iter().filter(|j| j.key == "j_mail" && !j.debuff).count() as f64;
    if jokers == 0.0 {
        return 0.0;
    }
    let n = idx.iter().filter_map(|&i| hand.get(i)).filter(|c| c.rank.0 == rank && c.enhancement != Some(Enhancement::Stone) && !c.debuff).count();
    5.0 * jokers * n as f64
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
            return match decide_cards(b, &cards, deck, hands, discards, need, size.saturating_sub(keep.len()).max(1)) {
                Action::Play(v, d) => Action::Play(v.into_iter().map(|k| free[k]).collect(), d),
                Action::Discard(v) => Action::Discard(v.into_iter().map(|k| free[k]).collect()),
            };
        }
    }
    decide_cards(b, hand, deck, hands, discards, need, size)
}

/// The heuristic play/discard policy (labelled as a heuristic everywhere it shows):
/// - play the best hand if it wins, if it's the last hand, or if repeating it keeps pace;
/// - otherwise, if chasing a flush is worth more than the best hand (exact draw odds ×
///   what that flush would score), throw away off-suit cards: with a discard, or by
///   playing them as a junk hand once discards are gone;
/// - otherwise discard the cards outside the best play and hope to improve it.
fn decide_cards(b: &Board, hand: &[Card], deck: &[Card], hands: i64, discards: i64, need: f64, size: usize) -> Action {
    let Some(best) = best_play(b, hand) else { return Action::Play(vec![], false) };
    let play_best = Action::Play(with_fillers(b, hand, &best.cards), false);
    // Mail-In Rebate: cash cards of its rank with a discard before playing (they pay $5
    // each), as long as the play doesn't need them. That keeps discards for cashing and
    // makes playing the way to dig.
    // Only when the round is safe (on pace with this hand): in a tight round the discards
    // are worth more for digging than $5.
    if discards > 0 && b.mail_rank.is_some() && best.floor * hands as f64 >= need {
        let pays: Vec<usize> = (0..hand.len()).filter(|&i| discard_money(b, hand, &[i]) > 0.0).collect();
        // also the ones in the best play, when the rest of the hand still keeps you on pace
        let rest: Vec<Card> = (0..hand.len()).filter(|i| !pays.contains(i)).map(|i| hand[i]).collect();
        let on_pace_without = best_play(b, &rest).is_some_and(|p| p.floor * hands as f64 >= need);
        let cash: Vec<usize> = pays.iter().copied().filter(|i| on_pace_without || !best.cards.contains(i)).collect();
        // With one discard left, collect them and cash them all right before the winning
        // hand (or the last one)
        let now = discards > 1 || best.floor >= need || hands <= 1;
        if !cash.is_empty() && now {
            return Action::Discard(cash);
        }
    }
    if best.floor >= need || hands <= 1 || deck.is_empty() {
        return play_best;
    }
    let on_pace = best.floor * hands as f64 >= need;
    // Mystic Summit pays +Mult on every hand once no discards are left: burn them first on
    // the worst cards (which also digs), unless Banner pays more for keeping them.
    let has = |k: crate::engine::Kind| b.jokers.iter().any(|j| j.kind == k && !j.debuff);
    if discards > 0 && has(crate::engine::Kind::MysticSummit) && !has(crate::engine::Kind::Banner) {
        let mut toss: Vec<usize> = (0..hand.len()).filter(|i| !best.cards.contains(i)).collect();
        toss.sort_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips()));
        toss.truncate(5);
        if toss.is_empty() {
            toss.push((0..hand.len()).min_by(|&a, &c| hand[a].rank.chips().total_cmp(&hand[c].rank.chips())).unwrap_or(0));
        }
        return Action::Discard(toss);
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
    pub swap: Vec<(Card, Card)>,
    pub add: Vec<Card>,
    pub levels: [i64; 12],
    /// A Planet card: Constellation grows ×0.1 when it's used
    pub planet: bool,
}

impl Use {
    pub fn apply(&self, b: &Board, hand: &[Card]) -> Option<(Board, Vec<Card>)> {
        let same = |a: &Card, c: &Card| a.rank == c.rank && a.suit == c.suit && a.enhancement == c.enhancement && a.edition == c.edition && a.seal == c.seal;
        let mut h = hand.to_vec();
        let mut done = vec![false; h.len()];
        for (from, to) in &self.swap {
            let i = (0..h.len()).find(|&i| !done[i] && same(&h[i], from))?;
            h[i] = Card { debuff: h[i].debuff, face_down: h[i].face_down, ..*to };
            done[i] = true;
        }
        h.extend(self.add.iter().copied());
        let mut b = b.clone();
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
    let has_seal = |h: &[Card]| h.iter().any(|c| c.seal == Some(crate::model::Seal::Blue));
    let mut seen = b.seal_seen_value <= 0.0 || has_seal(&start.hand);
    if b.seal_seen_value > 0.0 && !seen && has_seal(&hand) {
        money += b.seal_seen_value;
        seen = true;
    }
    while hands > 0 && !hand.is_empty() {
        b.hands_left = hands;
        b.discards_left = discards;
        b.deck_remaining = deck.len() as i64;
        use_if_better(&mut b, &mut hand, uses, &mut used);
        let act = match win_keeping_seals(&b, &hand, start.target - total) {
            Some(v) => Action::Play(v, false),
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
                    return RoundResult { total, won: true, saved: false, best_hand, plays, money, hands_left: hands, planets: seal_planets(&b, &hand) };
                }
                draw(&mut hand, &mut deck, size);
                if !seen && has_seal(&hand) {
                    money += b.seal_seen_value;
                    seen = true;
                }
            }
            Action::Discard(idx) => {
                money += discard_money(&b, &hand, &idx);
                hand = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
                draw(&mut hand, &mut deck, size);
                discards -= 1;
                if !seen && has_seal(&hand) {
                    money += b.seal_seen_value;
                    seen = true;
                }
            }
        }
    }
    // Mr. Bones: a lost round is saved if you reached 25% of the blind (card.lua, game_over)
    let bones = b.jokers.iter().any(|j| j.key == "j_mr_bones" && !j.debuff);
    let won = total >= start.target;
    RoundResult { total, won, saved: !won && bones && total >= 0.25 * start.target, best_hand, plays, money, hands_left: 0, planets: 0.0 }
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
