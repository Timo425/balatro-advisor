//! **Best play**: the move to make now in a blind. Every move (every play, every discard, for
//! your hand as it is and after each held consumable) is screened in stages, and the best are
//! played out by the simulated player over many rounds on the same draws, compared on one
//! measure (`sim::RoundGoals`) until the best is clear.

use super::*;
use super::value::{self, Gain, LongRun, Spending};

/// The draws of the face-down cards a play holding one is shown over (its hands and score
/// range): the first simulated rounds' (`sim::hidden_hands`). A budget: it shows, it doesn't rank.
const SHOWN_DRAWS: usize = 64;

/// Best play's note on a hand holding a face-down card (in its tip and the analysis' heuristics)
pub(super) const FACE_DOWN_NOTE: &str = "Face-down cards: every play and discard with them (in a hand of up to 12 cards; in a bigger one the plays and discards its structure gives on 4 draws of them and every play holding one, each play screened by its mean score over up to 128 draws, fewer the more plays there are to rank (past 28 cards all face down, or fewer face down in a bigger hand; 4 at the least), and where even 16 draws can't rank every play holding one (past 42 cards all face down) the structure's plays only: a budget) and a tarot on them is tried, valued over what they could be (which of them it goes on too), drawn anew in each simulated round from the unseen cards (the draw pile and themselves) as both sorts and the boss allow (their order among themselves included; drawn card by card, then each redrawn 16 times given the others: close to every set that fits being as likely, a budget); a play holding one shows the hands it makes (up to 3, likeliest first) and its lowest to highest score over 64 of those draws. A card a tarot flips face up while changing it counts as seen (the game shows it). After the first move the simulated player can't see them either: it decides on 4 draws of what they could be by what the sorts show then: the consumable most of them use first is used and what's next decided again on the cards it shows, else, of those using none, where they disagree and not every one has a sure win, the action the policy's own estimates of the round's points value most summed over them (heuristic: ranked in points, not by what else the round is worth, such as money or growth), else the action most of them make (the first draw's on a tie); 4 draws is a budget; a consumable held for a face-down card goes, in each of those draws, on the card that draw put in its place; the cards it draws later in the round are dealt face down where the boss turns them down (The Mark's face cards, The Wheel's 1 in 7, The Fish's draw after a hand) and decided on the same way; a play's face-up cards are arranged round a face-down card, which keeps its place and is taken as doing nothing (heuristic); a play holding one is scored in that order but still listed as your hand holds it (not yet shown)";

/// Best play's fallback (no round simulated) with a face-down card in hand
pub(super) const FACE_DOWN_FALLBACK_NOTE: &str = "Face-down cards: no round was simulated here, so this play is the one that scores the most on average over 64 draws of what they could be, by score alone (heuristic); a play holding one shows the hands it makes and its lowest to highest score over them, a play keeping one held its lowest score over them (a range where it moves); its face-up cards arranged round a face-down card, which keeps its place and is taken as doing nothing (heuristic), scored in that order but still listed as your hand holds it (not yet shown)";

pub(super) fn best_play(ctx: &Ctx, lr: &LongRun, spending: &Spending, tarots: &[TarotValue], hand_order: &[usize], ahead: Option<&value::Ahead>) -> Option<PlayAdvice> {
    let (run, data) = (ctx.run, ctx.data);
    if !(run.screen.in_blind() && !run.hand.is_empty()) {
        return None;
    }
    let mut b = ctx.base.clone();
    // each card of your hand by what you can see of it (a face-down one by its place and the
    // sorts: `model::hand_labels`)
    let names = crate::model::hand_labels(&run.hand);
    let names = &names[..];
    // A planet from a Blue Seal held at round end is the planet of the hand played last
    // (card.lua): that hand +1 level and Constellation ×0.1, as its gain in the same
    // long-run projection everything else is valued by. One value per hand type.
    let has_seals = run.hand.iter().chain(&run.draw_pile).any(|c| c.seal == Some(crate::model::Seal::Blue));
    // A dollar won this round, in the same long-run measure: what +$10 does, spent once and
    // held (rerolls and packs, `Spending`)
    let dollar_gain = ((lr.value(&Gain::money(10.0)) * spending.factor(run.dollars + 10.0) / spending.factor(run.dollars).max(1e-9) - 1.0) / 10.0).max(0.0);
    b.deck_remaining = run.draw_pile.len() as i64;
    // Cards worth drawing this round: for each consumable you hold that goes on one card, how
    // much more your run is worth with it used on a card in the draw pile than on its target in
    // your hand (the one its search chose: `tarot_values`), by `LongRun::deck_value` (quick
    // projection, then the full one; Blue Seal planets counted). A
    // simulated round that draws one is worth that much more (`RoundGoals::seen`).
    // (and whether any counted something in the blinds still ahead)
    let (seen, seen_ahead): (Vec<(String, Card, f64)>, bool) = {
        use crate::engine::consumable;
        let deck = &ctx.fresh_deck;
        let idx_of = |c: &Card| deck.iter().position(|d| d.same_kind(c));
        // the cards still to come as the game counts them: the draw pile and the face-down cards
        // in hand (UI_definitions.lua `G.UIDEF.view_deck`), never which of them is face down
        let mut kinds: Vec<Card> = vec![];
        for c in run.draw_pile.iter().chain(run.hand.iter().filter(|c| c.face_down)).map(|c| Card { face_down: false, ..*c }) {
            if !kinds.iter().any(|k| k.same_kind(&c)) {
                kinds.push(c);
            }
        }
        let mut out: Vec<(String, Card, f64)> = vec![];
        let mut any_ahead = false;
        for held in &run.consumables {
            if out.iter().any(|(k, _, _)| k == &held.key) {
                continue;
            }
            let Some((effect, 1, 1)) = data.center(&held.key).and_then(|x| consumable::card_effect(&held.key, &x.config)) else { continue };
            let val = |card: &Card, rounds: usize| idx_of(card).map(|i| lr.deck_value(&consumable::apply(effect, deck, &[i]), 0.0, rounds));
            // its searched target, not the best of noisy values; never below not using it
            // (1.0: your deck as it is, exact on the same rounds); on a face-down card, the decks
            // of what it could be (`decks`)
            let searched = tarots.iter().find(|t| t.key == held.key);
            let target = searched.and_then(|t| t.deck.clone());
            let now = |rounds: usize| searched.and_then(|t| t.deck_long(lr, 0.0, rounds)).unwrap_or(1.0).max(1.0);
            // a quick screen, then what clears the noise margin on the full projection
            let (now_q, now_f) = (now(TARGET_SCREEN_ROUNDS), now(value::TAROT_ROUNDS));
            let quick = par_map(&kinds, |k| val(k, TARGET_SCREEN_ROUNDS).unwrap_or(0.0) - now_q);
            let likely: Vec<Card> = kinds.iter().zip(&quick).filter(|(_, g)| **g > compare::EQUAL).map(|(k, _)| *k).collect();
            let full = par_map(&likely, |k| val(k, value::TAROT_ROUNDS).unwrap_or(0.0) - now_f);
            // and in the blinds still ahead in this ante (`value::Ahead::deck`), for the cards
            // whose Ante 8 worth passed (a budget: the screen is by that worth), against the same
            // deck the Ante 8 part is (its target, or your deck as it is when that's worth more)
            // (a pick of several decks, a random effect or one on a face-down card: your deck)
            let than = target.clone().filter(|_| now_f > 1.0).unwrap_or_else(|| deck.clone());
            let against = ahead.filter(|_| !likely.is_empty()).map(|a| a.deck_runs(&than));
            let in_ahead = |k: &Card| ahead.zip(idx_of(k)).zip(against.as_ref()).map_or(0.0, |((a, i), r)| a.deck(&consumable::apply(effect, deck, &[i]), r));
            for (k, g) in likely.into_iter().zip(full) {
                if g > compare::EQUAL {
                    let a = in_ahead(&k);
                    any_ahead |= a > 0.0;
                    out.push((held.key.clone(), k, g + a));
                }
            }
        }
        (out, any_ahead)
    };
    // What the jokers a round changes (Square, Green Joker, Ice Cream, …) are left with at its
    // end, in the same long-run measure: per Mult, ×Mult and Chip, and used up
    // (`LongRun::joker_worth`). A move that grows one is worth that much more. Each goal also
    // counts what it does in the blinds still ahead in this ante (`ahead`, `value::Ahead`).
    // The round's goals: those and the planets, a dollar and a hand left (`LongRun::round_goals`)
    let goals = sim::RoundGoals { seen: seen.clone(), ..lr.round_goals(has_seals, dollar_gain, true, ahead, &ctx.base) };
    let counts_ahead = goals.dollar_ahead > 0.0 || goals.planet_ahead.iter().any(|&x| x > 0.0) || seen_ahead || goals.jokers.iter().any(|w| w.ahead.counts());
    // one more of each, by what it adds to a won round in both parts (`RoundGoals::value`)
    let one_more = |w: &sim::JokerWorth, f: usize| goals.value(&sim::Outcome { won: 1.0, grown: sim::Grown { long: (1.0 + w.per[f]).ln(), ahead: w.ahead.up[f] }, ..Default::default() }) - 1.0;
    let grows: Vec<(String, [f64; 3])> = goals.jokers.iter().map(|w| (data.name(&w.key).to_string(), std::array::from_fn(|f| one_more(w, f)))).collect();
    let worth_drawing: Vec<(String, String, f64)> = seen
        .iter()
        .map(|(k, c, g)| (c.label(), run.consumables.iter().find(|x| &x.key == k).map_or(k.clone(), |x| x.name.clone()), *g))
        .collect();
    // Look-ahead: each candidate first move is simulated through the rest of the round.
    let look = ctx.specs.iter().find(|x| x.in_progress()).map(|spec| {
        let mut bb = b.clone();
        // The simulated player plays toward the same measure the advice ranks by
        bb.goals = Some(goals.clone());
        let start = spec.start.clone();
        let seed = ctx.opts.seed;
        // Consumables you hold, as the round can use them: tarots and spectral cards by
        // what they do to your hand, planets by the level they add. Every move is simulated
        // with them still held (used as the simulated player decides: `sim::use_if_better`, `sim::finish_with_uses`), and each
        // gets a row of its own: use it now, then the best move after it.
        let uses: Vec<sim::Use> = run
            .consumables
            .iter()
            .filter_map(|c| {
                if let Some(t) = tarots.iter().find(|t| t.key == c.key) {
                    return t.use_effect.clone();
                }
                let center = data.center(&c.key).filter(|x| x.set == "Planet")?;
                let h = center.config.get("hand_type")?.as_str().and_then(crate::engine::HandType::from_name)?;
                let mut levels = [0; 12];
                levels[h as usize] = 1;
                Some(sim::Use { name: center.name.clone(), key: c.key.clone(), levels, planet: true, ..Default::default() })
            })
            .collect();
        let to_opt = |m: &sim::Move, st: &RoundStart, board: &Board, (p, mean, spare, cash, planets, grown): (f64, f64, f64, f64, f64, f64), use_first: Option<String>| {
            let full = move_hand(st, board);
            let hand = &full[..];
            // A play is shown in the order to play it; a discard by rank (order doesn't matter),
            // face-down cards after the rest by their place (their stand-ins' ranks aren't theirs)
            let sorted_discard;
            let (action, idx) = match m {
                sim::Move::Play(v) => ("play", v),
                sim::Move::Discard(v) => {
                    let mut v = v.clone();
                    v.sort_by_key(|&i| match drawn(st, i) {
                        false => (false, std::cmp::Reverse(hand[i].rank.0), hand[i].suit as u8, 0),
                        true => (true, std::cmp::Reverse(0), 0, hand_order.get(i).copied().unwrap_or(usize::MAX)),
                    });
                    sorted_discard = v;
                    ("discard", &sorted_discard)
                }
            };
            let mut idx = idx.clone();
            // each card as you see it; Cryptid's copies, and a card Death copies a face-down card
            // onto, named by the card they copy, as what the analysis holds there is a stand-in's
            // (`drawn`)
            let label = |i: usize| match copy_source(st, i).filter(|&p| i >= st.hand.len() || drawn(st, p)) {
                Some(p) => format!("copy of {}", shown(hand, p, hand_order, names)),
                None => shown(hand, i, hand_order, names),
            };
            // a play holding a face-down card shows its hand and score over what the card could
            // be (the draws the first simulated rounds meet, `sim::hidden_hands`), never the
            // stand-in's alone: the hands it makes, likeliest first, and its lowest to highest
            // score (what it surely scores: the lowest of each draw's)
            let blind = action == "play" && idx.iter().any(|&i| drawn(st, i));
            let mut range = None;
            let (name, score, sure, dig, arrange) = if blind {
                let d = OverDraws::of(board, &sim::hidden_hands(board, st, seed, SHOWN_DRAWS), &idx);
                range = Some((d.lo, d.hi));
                (d.name, d.lo, d.sure, d.dig, (vec![], vec![]))
            } else if action == "play" {
                let (order, arrange, o) = arranged(board, hand, &idx, hand_order, &label);
                let dig = idx.len().saturating_sub(o.scoring.len());
                idx = order;
                (o.hand.name().to_string(), o.score, o.sure, dig, arrange)
            } else {
                (String::new(), 0.0, 0.0, 0, (vec![], vec![]))
            };
            let cards: Vec<String> = idx.iter().map(|&i| label(i)).collect();
            // cards a consumable added aren't in your hand yet: no position
            let indices = idx.iter().filter(|&&i| i < hand_order.len()).map(|&i| hand_order[i]).collect();
            PlayOption { spare_hands: spare, round_money: cash, action: action.into(), cards, indices, dig, hand: name, score, score_range: range, sure, p_win: p, mean_total: mean, use_first, planets, grown, tie: false, blind, then: None, arrange: arrange.0, arrange_indices: arrange.1 }
        };
        // Exact ties are broken by the cards themselves as you see them (`tie_key`), so neither
        // the order your hand is sorted in nor what a face-down card is changes the advice
        let canon = |m: &sim::Move, st: &RoundStart, hand: &[Card]| {
            let (kind, idx) = match m {
                sim::Move::Play(v) => (0, v),
                sim::Move::Discard(v) => (1, v),
            };
            (kind, tie_key(st, hand, idx))
        };
        // Every move: every play and every discard, for your hand as it is and after each
        // held consumable (used first). No move needs a rule to be considered. A hand of
        // more than `sim::ALL_MOVES_MAX` cards gives far more than a race can take: its moves
        // come from its structure, over every card (`moves_of`, `sim::big_hand_moves`), with its
        // face-down cards drawn on seeds the race never plays.
        type Cand = (sim::Move, Board, RoundStart, Vec<sim::Use>, Option<String>);
        let moves_of = |st: &RoundStart, b: &Board| moves_of(st, b, seed);
        let mut cands: Vec<Cand> = moves_of(&start, &bb).into_iter().map(|m| (m, bb.clone(), start.clone(), uses.clone(), None)).collect();
        for (k, u) in uses.iter().enumerate() {
            if uses[..k].iter().any(|x| x.name == u.name) {
                continue;
            }
            let Some((b2, mut h2)) = u.apply(&bb, &start.hand) else { continue };
            let rest: Vec<sim::Use> = uses.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, x)| x.clone()).collect();
            // one that goes on a face-down card: done in each round, on what that round deals there
            // (`RoundStart::changes`), so the move is valued over what the card could be; its
            // moves are those of the hand it leaves (Cryptid's copies at its end), as they're named
            // (`move_hand`)
            let mut changes = vec![];
            if start.hidden.is_some() && !u.at.is_empty() && u.swap.is_empty() && u.add.is_empty() {
                h2 = start.hand.clone();
                changes = u.at.clone();
            }
            let s2 = RoundStart { hand: h2, changes, ..start.clone() };
            for m in moves_of(&s2, &b2) {
                cands.push((m, b2.clone(), s2.clone(), rest.clone(), Some(u.name.clone())));
            }
        }
        // a hand the game allows a move from always has one; none means nothing to race: the fallback below (the best play of your hand, if any) instead of
        // ranking an empty field
        if cands.is_empty() {
            return (vec![], vec![], None, vec![]);
        }
        // All compared in one race (see `compare`), on one measure per simulated round, as
        // the shop ranking does: winning it, times the long-run value of what it leaves you
        // (the planets Blue Seals held at the end make, and its money: hands left over at cash
        // out and what it pays as you go, each by its gain in the long-run projection). A lost
        // round counts 0; how close a lost round came to the target orders what the value
        // can't separate (a round you nearly always lose). Points past the target count for
        // nothing, so a won round adds nothing there.
        let utility = |o: &sim::Outcome| goals.value(o);
        let progress = |o: &sim::Outcome| (1.0 - o.won) * (o.total / start.target.max(1.0)).min(1.0);
        let mean_u = |v: &[sim::Outcome]| v.iter().map(utility).sum::<f64>() / v.len().max(1) as f64;
        let keys: Vec<_> = cands.iter().map(|c| (c.4.clone(), canon(&c.0, &c.2, &move_hand(&c.2, &c.1)))).collect();
        let ck = |c: usize| &keys[c];
        // the budget, halving many moves (`compare::halving`)
        let budget = compare::halving(SEARCH_BUDGET, SEARCH_FIRST, cands.len());
        let race = compare::race_by_round(
            cands.len(),
            SEARCH_FIRST,
            compare::MAX,
            budget,
            |c, i| {
                let (m, b, st, u, _) = &cands[c];
                sim::outcomes_after(b, st, m, i..i + 1, seed, u).remove(0)
            },
            utility,
            progress,
            |x, y| ck(y).cmp(ck(x)),
            // a lost round counts 0
            true,
        );
        // as good as the leader: shown equal, or still undecided at the round cap (1600 rounds
        // couldn't tell them apart)
        let tied: Vec<bool> = race.tied.iter().zip(&race.undecided).map(|(t, u)| *t || *u).collect();
        // how each move left the race, for the reference (`Options::reference`)
        let fate: Vec<String> = (0..cands.len())
            .map(|c| match race.left[c] {
                _ if c == race.leader => "the race's leader".to_string(),
                Some((n, "equal")) if !race.tied[c] => format!("equal after {n} rounds to a move later found worse"),
                Some((n, why)) => format!("{why} after {n} rounds"),
                None => "undecided at the cap".to_string(),
            })
            .collect();
        let (outs, leader) = (race.samples, race.leader);
        // the yardstick for the search itself (`Options::reference`): every move on rounds of
        // its own, as many as the race's most
        let reference: Vec<f64> = if ctx.opts.reference {
            par_map(&(0..cands.len()).collect::<Vec<_>>(), |&c| {
                let (m, b, st, u, _) = &cands[c];
                mean_u(&sim::outcomes_after(b, st, m, compare::MAX..2 * compare::MAX, seed, u))
            })
        } else {
            vec![]
        };
        let mut opts: Vec<PlayOption> = (0..cands.len())
            .map(|c| {
                let v = &outs[c];
                let k = v.len().max(1) as f64;
                let avg = |f: fn(&sim::Outcome) -> f64| v.iter().map(f).sum::<f64>() / k;
                let (m, b, st, _, use_first) = &cands[c];
                // what the jokers' numbers add to the round's value (`RoundGoals::value` with them
                // against without), a lost round nothing
                let grown = v.iter().map(|o| goals.value(o) - goals.value(&sim::Outcome { grown: sim::Grown::default(), ..*o })).sum::<f64>() / k;
                let mut o = to_opt(m, st, b, (avg(|o| o.won), avg(|o| o.total), avg(|o| o.spare), avg(|o| o.cash), avg(|o| o.planets), grown), use_first.clone());
                o.tie = tied[c];
                if matches!(m, sim::Move::Discard(_)) {
                    let mut by: std::collections::HashMap<crate::engine::HandType, (usize, f64)> = std::collections::HashMap::new();
                    for (h, sc) in v.iter().filter_map(|o| o.next) {
                        let e = by.entry(h).or_insert((0, 0.0));
                        e.0 += 1;
                        e.1 += sc;
                    }
                    o.then = by.into_iter().max_by_key(|(h, (n, _))| (*n, *h as u8)).map(|(h, (n, sum))| (h.name().to_string(), sum / n as f64, n as f64 / k));
                }
                o
            })
            .collect();
        let score_of = |c: usize| mean_u(&outs[c]);
        let mut order: Vec<usize> = (0..cands.len()).collect();
        // the leader, then its ties, then the rest by their measure
        order.sort_by(|&x, &y| (y == leader).cmp(&(x == leader)).then(tied[y].cmp(&tied[x])).then(score_of(y).total_cmp(&score_of(x))).then(ck(x).cmp(ck(y))));
        // Moves as good as the leader are equal as far as can be told: among them a play that
        // wins the round right now goes first (nothing to gain by waiting), then the best
        // estimate, then how close lost rounds came, then by the cards themselves. The estimate
        // is paired: the leader's value plus each one's difference to it on the rounds both
        // were sampled on (ties stop early; their own averages are over fewer, other rounds).
        let need = start.target - start.scored;
        let wins_now = |o: &PlayOption| o.action == "play" && o.use_first.is_none() && o.sure >= need;
        let group = order.iter().take_while(|&&c| c == leader || tied[c]).count();
        let prog = |c: usize| outs[c].iter().map(progress).sum::<f64>() / outs[c].len().max(1) as f64;
        let estimate = |c: usize| {
            let k = outs[c].len().min(outs[leader].len());
            score_of(leader) + mean_u(&outs[c][..k]) - mean_u(&outs[leader][..k])
        };
        order[..group].sort_by(|&x, &y| wins_now(&opts[y]).cmp(&wins_now(&opts[x])).then(estimate(y).total_cmp(&estimate(x))).then(prog(y).total_cmp(&prog(x))).then(ck(x).cmp(ck(y))));
        let first = order[0];
        for &c in &order {
            opts[c].tie = c != first && (c == leader || tied[c]);
        }
        // the pick against the reference's best, on a block of rounds neither was chosen on
        let gap = (!reference.is_empty()).then(|| {
            let best = (0..reference.len()).max_by(|&x, &y| reference[x].total_cmp(&reference[y]).then(ck(y).cmp(ck(x)))).unwrap_or(0);
            let block = |c: usize| -> Vec<f64> {
                let (m, b, st, u, _) = &cands[c];
                sim::outcomes_after(b, st, m, 2 * compare::MAX..3 * compare::MAX, seed, u).iter().map(utility).collect()
            };
            let (p, q) = (block(first), block(best));
            let d: Vec<f64> = q.iter().zip(&p).map(|(x, y)| x - y).collect();
            let k = d.len() as f64;
            let m = d.iter().sum::<f64>() / k;
            let se = (d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (k - 1.0).max(1.0) / k).sqrt();
            let scale = q.iter().sum::<f64>() / k;
            (m / scale.max(1e-9), se / scale.max(1e-9))
        });
        let label = |o: &PlayOption| format!("{} {} [{}]", o.action, o.cards.join(" "), o.use_first.clone().unwrap_or_default());
        // the yardstick for a pick a change moved (`Options::judge`): each named move against
        // the pick on rounds the race and the reference didn't use, paired, with the oracle on
        // in each worker (thread-local): it makes the policy's decisions that aren't a win on
        // the table; the finish, consumables and valuation stay the code's own
        let oracle_block = |m: &sim::Move, b: &Board, st: &RoundStart, u: &[sim::Use]| -> Vec<f64> {
            let from = 3 * compare::MAX;
            let n = ctx.opts.judge_rounds;
            let chunks: Vec<usize> = (0..n.div_ceil(8)).collect();
            par_map(&chunks, |&k| {
                sim::set_oracle(JUDGE_ROLLOUTS);
                let v: Vec<f64> = sim::outcomes_after(b, st, m, from + 8 * k..from + (8 * k + 8).min(n), seed, u).iter().map(utility).collect();
                sim::set_oracle(0);
                v
            })
            .concat()
        };
        // a judged move by your hand's positions: one of the moves considered (with a
        // consumable used first, by its name: what it changes or adds is today's choice), else,
        // when it uses none and is a move the game allows, played as it is (the moves tried may
        // have changed)
        type Judged = (sim::Move, Board, RoundStart, Vec<sim::Use>);
        let judge_move = |j: &str| -> Option<Judged> {
            let (action, rest) = j.split_once(' ')?;
            let (pos, use_first) = rest.split_once(" [")?;
            let use_first = use_first.strip_suffix(']')?;
            let mut pos: Vec<usize> = pos.split(',').filter(|x| !x.is_empty()).map(|x| x.parse().ok()).collect::<Option<_>>()?;
            pos.sort();
            let same = |c: usize| {
                let mut ix = opts[c].indices.clone();
                ix.sort();
                opts[c].action == action && ix == pos && opts[c].use_first.as_deref().unwrap_or("") == use_first
            };
            let found: Vec<usize> = (0..cands.len()).filter(|&c| same(c)).collect();
            let allowed = matches!(action, "play" | "discard") && (1..=5).contains(&pos.len()) && pos.windows(2).all(|w| w[0] != w[1]) && (action == "play" || start.discards > 0);
            match (found.as_slice(), use_first.is_empty() && allowed) {
                ([c], _) => Some((cands[*c].0.clone(), cands[*c].1.clone(), cands[*c].2.clone(), cands[*c].3.clone())),
                ([], true) => {
                    let idx: Vec<usize> = pos.iter().map(|p| hand_order.iter().position(|h| h == p)).collect::<Option<_>>()?;
                    let m = if action == "play" { sim::Move::Play(idx) } else { sim::Move::Discard(idx) };
                    Some((m, bb.clone(), start.clone(), uses.clone()))
                }
                _ => None,
            }
        };
        let to_judge: Vec<(String, Option<Judged>)> = ctx.opts.judge.iter().map(|j| (j.clone(), judge_move(j))).collect();
        let pick_block = if ctx.opts.judge_rounds == 0 || to_judge.iter().all(|t| t.1.is_none()) {
            vec![]
        } else {
            let (m, b, st, u, _) = &cands[first];
            oracle_block(m, b, st, u)
        };
        let judged: Vec<(String, Option<(f64, f64)>)> = to_judge
            .into_iter()
            .map(|(j, mv)| {
                let r = mv.filter(|_| !pick_block.is_empty()).map(|(m, b, st, u)| {
                    let q = oracle_block(&m, &b, &st, &u);
                    let d: Vec<f64> = q.iter().zip(&pick_block).map(|(x, y)| x - y).collect();
                    let k = d.len() as f64;
                    let mean = d.iter().sum::<f64>() / k;
                    let se = (d.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (k - 1.0).max(1.0) / k).sqrt();
                    let scale = pick_block.iter().sum::<f64>() / k;
                    (mean / scale.max(1e-9), se / scale.max(1e-9))
                });
                (j, r)
            })
            .collect();
        // exact ties by the cards themselves (`ck`, as the race), not by the labels, which say a
        // face-down card's place
        let mut by_value: Vec<usize> = (0..reference.len()).collect();
        by_value.sort_by(|&x, &y| reference[y].total_cmp(&reference[x]).then_with(|| ck(x).cmp(ck(y))));
        let reference: Vec<(String, f64, String)> = by_value.into_iter().map(|c| (label(&opts[c]), reference[c], fate[c].clone())).collect();
        let mut sorted: Vec<PlayOption> = order.into_iter().map(|c| opts[c].clone()).collect();
        sorted.dedup_by(|a, b| a.action == b.action && a.cards == b.cards && a.use_first == b.use_first);
        (sorted, reference, gap, judged)
    });
    let reference = look.as_ref().map_or(vec![], |l| l.1.clone());
    let reference_gap = look.as_ref().and_then(|l| l.2);
    let judged = look.as_ref().map_or(vec![], |l| l.3.clone());
    match look.map(|l| l.0).filter(|o| !o.is_empty()) {
        Some(mut opts) => {
            let best = opts.remove(0);
            opts.truncate(4);
            // Held consumables the look-ahead can't use: say so
            let missing: Vec<&str> = run
                .consumables
                .iter()
                .filter(|c| data.center(&c.key).is_none_or(|x| x.set != "Planet") && !tarots.iter().any(|t| t.key == c.key && t.use_effect.is_some()))
                .map(|c| c.name.as_str())
                .collect();
            let mut tips: Vec<String> = vec![];
            if !missing.is_empty() {
                tips.push(format!("Not used in this look-ahead (no modelled effect on this hand): {}", missing.join(", ")));
            }
            if run.hand.iter().any(|c| c.face_down) {
                tips.push(FACE_DOWN_NOTE.into());
                // what the draws had to drop, if anything
                tips.extend(ctx.specs.iter().find(|x| x.in_progress()).and_then(|s| s.start.hidden.as_deref()).and_then(sim::Hidden::fell_back_note));
            }
            let tip = (!tips.is_empty()).then(|| tips.join(". "));
            Some(PlayAdvice { reference, reference_gap, judged, worth_drawing: worth_drawing.clone(), grows: grows.clone(), counts_ahead, grown: best.grown, then: best.then.clone(), ties: opts.iter().filter(|o| o.tie).count(), planets: Some(best.planets), use_first: best.use_first, spare_hands: Some(best.spare_hands), round_money: Some(best.round_money), action: best.action, cards: best.cards, arrange: best.arrange, arrange_indices: best.arrange_indices, dig: best.dig, indices: best.indices, hand: best.hand, score: best.score, score_range: best.score_range, blind: best.blind, p_win: Some(best.p_win), alternatives: opts, tip })
        }
        None => {
            // your hand as the round has it (a face-down card its stand-in), and what's known of
            // its face-down cards
            let start: RoundStart = match ctx.specs.iter().find(|x| x.in_progress()) {
                Some(s) => s.start.clone(),
                None => {
                    let (hand, deck, hidden) = face_down_drawn(run, &ctx.base, &run.hand, hand_order, &run.draw_pile, ctx.opts.seed);
                    RoundStart { hand, deck, hidden, ..Default::default() }
                }
            };
            let hand = &start.hand;
            let blind_hand = hand.iter().any(|c| c.face_down);
            // with a face-down card in hand, the play is picked over what they could be (the draws
            // the first simulated rounds meet, `sim::hidden_hands`), never on one draw: of every
            // play (Best play's moves, `moves_of`), the
            // one that scores the most on average over them; an exact tie goes to the fewest
            // cards played, then the cards as you see them, as the look-ahead's `canon` breaks it
            // (`tie_key`)
            let mut plain: Option<(String, f64)> = None;
            let pick: Option<(Vec<usize>, Option<OverDraws>)> = if blind_hand {
                let hands = sim::hidden_hands(&b, &start, ctx.opts.seed, SHOWN_DRAWS);
                moves_of(&start, &b, ctx.opts.seed)
                    .into_iter()
                    .filter_map(|m| match m {
                        sim::Move::Play(c) => Some(c),
                        sim::Move::Discard(_) => None,
                    })
                    .map(|c| {
                        let d = OverDraws::of(&b, &hands, &c);
                        (c, d)
                    })
                    .fold(None, |best: Option<(Vec<usize>, OverDraws)>, x| {
                        // fewer cards first (a card that doesn't score isn't thrown with it),
                        // then the cards themselves
                        let key = |c: &[usize]| (c.len(), tie_key(&start, hand, c));
                        match best {
                            Some(y) if y.1.mean > x.1.mean || (y.1.mean == x.1.mean && key(&y.0) <= key(&x.0)) => Some(y),
                            _ => Some(x),
                        }
                    })
                    .map(|(c, d)| (c, Some(d)))
            } else {
                // with none face down, the play's hand and floor as `sim::best_play` gives them
                sim::best_play(&b, hand).map(|p| {
                    plain = Some((p.hand.name().to_string(), p.floor));
                    (p.cards, None)
                })
            };
            pick.map(|(cards, over)| {
            // a play holding a face-down card: in the order its draws hold it, its hands and
            // score over them; else arranged as it's best played, its score over the draws all
            // the same where one is held (Raised Fist, Baron, Steel cards: a range when it moves)
            let blind = cards.iter().any(|&i| hand[i].face_down);
            let (order, (arrange, arrange_indices), o) = arranged(&b, hand, &cards, hand_order, &|i| shown(hand, i, hand_order, names));
            let (order, arrange, arrange_indices) = if blind { (cards.clone(), vec![], vec![]) } else { (order, arrange, arrange_indices) };
            PlayAdvice {
            use_first: None,
            reference: vec![],
            reference_gap: None,
            judged: vec![],
            worth_drawing: worth_drawing.clone(),
            grows: grows.clone(),
            counts_ahead,
            grown: 0.0,
            planets: None,
            then: None,
            ties: 0,
            spare_hands: None,
            round_money: None,
            action: "play".into(),
            cards: order.iter().map(|&i| shown(hand, i, hand_order, names)).collect(),
            arrange,
            arrange_indices,
            dig: match (&over, &plain) {
                (Some(d), _) if blind => d.dig,
                (Some(_), _) => cards.len().saturating_sub(o.scoring.len()),
                _ => 0,
            },
            indices: order.iter().map(|&i| hand_order[i]).collect(),
            hand: match (&over, &plain) {
                (Some(d), _) if blind => d.name.clone(),
                (_, Some((h, _))) => h.clone(),
                _ => o.hand.name().to_string(),
            },
            score: match (&over, &plain) {
                (Some(d), _) => d.lo,
                (_, Some((_, f))) => *f,
                _ => o.score,
            },
            score_range: over.as_ref().filter(|d| blind || d.lo < d.hi).map(|d| (d.lo, d.hi)),
            blind,
            p_win: None,
            alternatives: vec![],
            // with what the draws had to drop, if anything
            tip: blind_hand.then(|| std::iter::once(FACE_DOWN_FALLBACK_NOTE.to_string()).chain(start.hidden.as_deref().and_then(sim::Hidden::fell_back_note)).collect::<Vec<_>>().join(". ")),
            }
            })
        }
    }
}

/// A play of a hand holding face-down cards, over draws of what they could be (one hand per
/// draw, the play `idx` in each): the hands it makes, its score's range and what it surely
/// scores, as Best play shows it
struct OverDraws {
    /// The hands it makes, likeliest first (up to 3), "Flush or Pair"
    name: String,
    /// Its lowest and highest score (each with nothing random rolling its way, `engine::Unlucky`)
    lo: f64,
    hi: f64,
    /// Its mean score
    mean: f64,
    /// What it surely scores: the lowest of each draw's
    sure: f64,
    /// The cards played that don't score, when every draw agrees (else 0)
    dig: usize,
}

impl OverDraws {
    fn of(board: &Board, hands: &[Vec<Card>], idx: &[usize]) -> OverDraws {
        let outs: Vec<crate::engine::Outcome> = hands
            .iter()
            .map(|h| {
                let rest: Vec<Card> = (0..h.len()).filter(|i| !idx.contains(i)).map(|i| h[i]).collect();
                crate::engine::score(board, &idx.iter().map(|&i| h[i]).collect::<Vec<_>>(), &rest, &mut crate::engine::Unlucky, false)
            })
            .collect();
        let lo = outs.iter().map(|o| o.score).fold(f64::INFINITY, f64::min);
        let hi = outs.iter().map(|o| o.score).fold(f64::NEG_INFINITY, f64::max);
        let mean = outs.iter().map(|o| o.score).sum::<f64>() / outs.len().max(1) as f64;
        let sure = outs.iter().map(|o| o.sure).fold(f64::INFINITY, f64::min);
        let mut kinds: Vec<(crate::engine::HandType, usize)> = vec![];
        for o in &outs {
            match kinds.iter_mut().find(|k| k.0 == o.hand) {
                Some(k) => k.1 += 1,
                None => kinds.push((o.hand, 1)),
            }
        }
        kinds.sort_by(|a, b| b.1.cmp(&a.1).then((b.0 as u8).cmp(&(a.0 as u8))));
        let name = kinds.iter().take(3).map(|k| k.0.name()).collect::<Vec<_>>().join(" or ");
        let digs: Vec<usize> = outs.iter().map(|o| idx.len().saturating_sub(o.scoring.len())).collect();
        let dig = if digs.windows(2).all(|w| w[0] == w[1]) { digs.first().copied().unwrap_or(0) } else { 0 };
        OverDraws { name, lo, hi, mean, sure, dig }
    }
}

/// The play `idx` from `hand` (in the analysis' order, `model::fixed_order`; `hand_order`: each card's place in
/// your hand, cards a consumable added have none) in the order to play it; your hand left to
/// right as it should sit (the cards played in that order, then the cards kept), as labels
/// (`label`: each card as the move's listing names it) and
/// as the places they're in now, when the hand as it sits scores less (the game plays the
/// selected cards in the order they sit: state_events.lua `G.FUNCS.play_cards_from_highlighted`),
/// else none; and the play's outcome. The engine arranges the cards (`engine::score`), from the
/// cards in a fixed order, so how your hand is sorted never changes the score.
#[allow(clippy::type_complexity)]
fn arranged(board: &Board, hand: &[Card], idx: &[usize], hand_order: &[usize], label: &dyn Fn(usize) -> String) -> (Vec<usize>, (Vec<String>, Vec<usize>), crate::engine::Outcome) {
    use crate::engine::{score, score_as_played, Unlucky};
    let rest: Vec<usize> = (0..hand.len()).filter(|i| !idx.contains(i)).collect();
    let cards = |v: &[usize]| v.iter().map(|&i| hand[i]).collect::<Vec<Card>>();
    let o = score(board, &cards(idx), &cards(&rest), &mut Unlucky, false);
    let pick = |v: &[usize], order: &Option<Vec<usize>>| order.as_ref().map_or(v.to_vec(), |o| o.iter().map(|&k| v[k]).collect());
    let (play, keep) = (pick(idx, &o.played_order), pick(&rest, &o.held_order));
    let on_screen = |v: &[usize]| {
        let mut v = v.to_vec();
        v.sort_by_key(|&i| hand_order.get(i).copied().unwrap_or(usize::MAX));
        v
    };
    // as they sit, on the same measure (The Hook's pick averaged over every set: gap 15)
    let as_they_sit = score_as_played(board, &cards(&on_screen(idx)), &cards(&on_screen(&rest)), &mut Unlucky, false).score;
    let arrange = if as_they_sit < o.score {
        let all: Vec<usize> = play.iter().chain(&keep).copied().collect();
        (all.iter().map(|&i| label(i)).collect(), all.iter().filter_map(|&i| hand_order.get(i).copied()).collect())
    } else {
        (vec![], vec![])
    };
    (play, arrange, o)
}

/// Best play's moves from `st` on `b`, listed on the hand they're named on (`move_hand`): every
/// play and discard of a hand of up to `sim::ALL_MOVES_MAX` cards, a bigger one's from its
/// structure (`sim::big_hand_moves`) with its face-down cards as simulated rounds draw them
/// (`sim::hidden_hands`, a consumable used first done to each draw), on seeds of their own (the
/// race's rounds use `seed`'s)
pub(super) fn moves_of(st: &RoundStart, b: &Board, seed: u64) -> Vec<sim::Move> {
    let hand = move_hand(st, b);
    if hand.len() > sim::ALL_MOVES_MAX {
        let draws = if st.hand.iter().any(|c| c.face_down) { sim::hidden_hands_par(b, st, seed ^ 0x6269_675f_6861_6e64, sim::BIG_HAND_RANK_DRAWS) } else { vec![hand.into_owned()] };
        debug_assert!(draws.iter().all(|h| h.len() == move_hand(st, b).len()));
        return sim::big_hand_moves(b, &draws, &st.deck, st.discards);
    }
    let mut v = sim::all_plays(&hand);
    v.extend(sim::all_discards(&hand, st.discards));
    v
}

/// The hand a move is named and ordered on: the round's, with the cards a consumable used first
/// on a face-down card adds at its end (`RoundStart::changes`: Cryptid's copies), face up as the
/// game deals them (card.lua `Card:use_consumeable`, common_events.lua `copy_card` doesn't copy
/// `facing`, and `CardArea:emplace` turns nothing down), though what they are is what each
/// simulated round deals at the card they copy (`drawn`)
pub(super) fn move_hand<'a>(st: &'a RoundStart, b: &Board) -> std::borrow::Cow<'a, [Card]> {
    if st.changes.is_empty() {
        return std::borrow::Cow::Borrowed(&st.hand);
    }
    let mut h = st.hand.clone();
    for (e, places) in &st.changes {
        let added: Vec<Card> = sim::effect_on(b, &h, *e, places).into_iter().skip(h.len()).collect();
        h.extend(added);
    }
    std::borrow::Cow::Owned(h)
}

/// The place card `i` of the hand a move is named on (`move_hand` of `st`) was copied from by a
/// consumable used first: Cryptid's copies (the cards added after the hand) the card used on,
/// the card Death changes its right card ("the first becomes a copy of the second")
fn copy_source(st: &RoundStart, i: usize) -> Option<usize> {
    use crate::engine::consumable::CardEffect;
    st.changes.iter().rev().find_map(|(e, places)| match e {
        CardEffect::Copies(_) if i >= st.hand.len() => places.first().copied(),
        CardEffect::CopyLeftToRight if places.first() == Some(&i) => places.get(1).copied(),
        _ => None,
    })
}

/// Whether card `i` of the hand a move is named on (`move_hand` of `st`) is one each simulated
/// round deals anew, so the analysis' card there is a stand-in: a card face down in the round's
/// hand (changed by a consumable used first or not: `move_hand` adds what a change adds, but
/// leaves the cards changed in place as they were), or a copy of one (`copy_source`: Cryptid's,
/// Death's)
fn drawn(st: &RoundStart, i: usize) -> bool {
    let down = |p: usize| st.hand.get(p).is_some_and(|c| c.face_down);
    down(i) || copy_source(st, i).is_some_and(down)
}

/// The key exact ties between moves are broken by (`best_play`'s `canon`, its fallback): the
/// cards played or thrown, each by what you can see of it, so neither how your hand is sorted
/// nor what a face-down card really is changes the advice: a face-up card by `Card::order_key`,
/// one the rounds deal anew (`drawn`) by its place in the analysis' hand (`model::fixed_order`:
/// what the sorts show), never by its stand-in
type CardKey = (bool, usize, (std::cmp::Reverse<u8>, u8, String));

fn tie_key(st: &RoundStart, hand: &[Card], idx: &[usize]) -> Vec<CardKey> {
    let mut k: Vec<CardKey> = idx.iter().map(|&i| if drawn(st, i) { (true, i, (std::cmp::Reverse(0), 0, String::new())) } else { (false, 0, hand[i].order_key()) }).collect();
    k.sort();
    k
}

/// Card `i` of `hand` (in the analysis' order, `model::fixed_order`; `hand_order`: each card's place in your hand)
/// as you'd name it: a face-down card by its place and what the sorts show (`names`, from
/// `model::hand_labels` of your hand), never by the stand-in the advisor picked for it; any
/// other card (and one a consumable added) by its `Card::label`.
fn shown(hand: &[Card], i: usize, hand_order: &[usize], names: &[String]) -> String {
    match hand_order.get(i).and_then(|&p| names.get(p)) {
        Some(n) if hand[i].face_down => n.clone(),
        _ => hand[i].label(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::consumable::CardEffect;

    #[test]
    fn copies_of_a_face_down_card_are_drawn_and_cryptids_dealt_face_up_and_tied_by_place() {
        // Cryptid used first on a face-down card (place 1): the game deals its copies face up
        // (card.lua `Card:use_consumeable`, common_events.lua `copy_card`), so they're listed
        // face up, while what they are is what each simulated round deals at the card they
        // copy (`drawn`). A tie between moves reads where those cards sit, never the stand-in:
        // two starts with different stand-ins there give the same keys. Fails if the copies are
        // dealt face down again or a tie reads a stand-in's key.
        let start = |down: &str| {
            let mut hand = Card::parse_list(&format!("AS {down} 7D 4H")).unwrap();
            hand[1].face_down = true;
            RoundStart { hand, changes: vec![(CardEffect::Copies(2), vec![1])], ..Default::default() }
        };
        let b = Board::empty();
        let (a, c) = (start("9S"), start("8C"));
        let h = move_hand(&a, &b);
        assert_eq!(h.len(), 6);
        assert!(h[4..].iter().all(|x| !x.face_down && x.rank == h[1].rank && x.suit == h[1].suit), "{h:?}");
        assert!(h[1].face_down);
        assert_eq!((0..6).map(|i| drawn(&a, i)).collect::<Vec<_>>(), vec![false, true, false, false, true, true]);
        let hc = move_hand(&c, &b);
        for idx in [vec![1, 4], vec![0, 5], vec![1, 2, 3]] {
            assert_eq!(tie_key(&a, &h, &idx), tie_key(&c, &hc, &idx), "{idx:?}");
        }
        // a face-up card still by what it is
        assert_ne!(tie_key(&a, &h, &[0]), tie_key(&a, &h, &[2]));
        // with no change on a face-down card, nothing it adds is drawn
        let plain = RoundStart { hand: Card::parse_list("AS 9S 7D").unwrap(), changes: vec![(CardEffect::Copies(2), vec![1])], ..Default::default() };
        assert!(!(0..5).any(|i| drawn(&plain, i)));
        // Death used first on a face-up card and a face-down one: the first becomes a copy of
        // the second, so what it is too is each round's draw; on two face-up cards, neither
        let mut hand = Card::parse_list("AS 9S 7D").unwrap();
        hand[1].face_down = true;
        let death = RoundStart { hand, changes: vec![(CardEffect::CopyLeftToRight, vec![0, 1])], ..Default::default() };
        assert_eq!((0..3).map(|i| drawn(&death, i)).collect::<Vec<_>>(), vec![true, true, false]);
        let seen = RoundStart { hand: Card::parse_list("AS 9S 7D").unwrap(), changes: vec![(CardEffect::CopyLeftToRight, vec![0, 1])], ..Default::default() };
        assert!(!(0..3).any(|i| drawn(&seen, i)));
        // a face-down card Death turns into a copy of a face-up one: still each round's card
        // (the hand Best play names holds its stand-in there, unchanged: gap 25 (g))
        let mut hand = Card::parse_list("AS 9S 7D").unwrap();
        hand[0].face_down = true;
        let known = RoundStart { hand, changes: vec![(CardEffect::CopyLeftToRight, vec![0, 1])], ..Default::default() };
        assert_eq!((0..3).map(|i| drawn(&known, i)).collect::<Vec<_>>(), vec![true, false, false]);
    }
}
