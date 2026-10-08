//! **Best play**: the move to make now in a blind. Every move (every play, every discard, for
//! your hand as it is and after each held consumable) is screened in stages, and the best are
//! played out by the simulated player over many rounds on the same draws, compared on one
//! measure (`sim::RoundGoals`) until the best is clear.

use super::*;
use super::value::{Gain, LongRun, Spending};

pub(super) fn best_play(ctx: &Ctx, lr: &LongRun, spending: &Spending, tarots: &[TarotValue], hand_order: &[usize]) -> Option<PlayAdvice> {
    let (run, data) = (ctx.run, ctx.data);
    if !(run.screen.in_blind() && !run.hand.is_empty()) {
        return None;
    }
    let mut b = ctx.base.clone();
    // A planet from a Blue Seal held at round end is the planet of the hand played last
    // (card.lua): that hand +1 level and Constellation ×0.1, as its gain in the same
    // long-run projection everything else is valued by. One value per hand type.
    let has_seals = run.hand.iter().chain(&run.draw_pile).any(|c| c.seal == Some(crate::model::Seal::Blue));
    let planet_gain_by: Vec<f64> = if has_seals { crate::engine::HandType::ALL.iter().map(|&h| (lr.planet(h) - 1.0).max(0.0)).collect() } else { vec![0.0; 12] };
    // A dollar won this round, in the same long-run measure: what +$10 does, spent once and
    // held (rerolls and packs, `Spending`)
    let dollar_gain = ((lr.value(&Gain::money(10.0)) * spending.factor(run.dollars + 10.0) / spending.factor(run.dollars).max(1e-9) - 1.0) / 10.0).max(0.0);
    b.deck_remaining = run.draw_pile.len() as i64;
    // Cards worth drawing this round: for each consumable you hold that goes on one card, how
    // much more your run is worth with it used on a card in the draw pile than on its target in
    // your hand (the one its search chose: `tarot_values`), by `LongRun::deck_value` (quick
    // projection, then the full one; Blue Seal planets counted). A
    // simulated round that draws one is worth that much more (`RoundGoals::seen`).
    let seen: Vec<(String, Card, f64)> = {
        use crate::engine::consumable;
        let deck = &ctx.fresh_deck;
        let idx_of = |c: &Card| deck.iter().position(|d| d.same_kind(c));
        let mut kinds: Vec<Card> = vec![];
        for c in &run.draw_pile {
            if !kinds.iter().any(|k| k.same_kind(c)) {
                kinds.push(*c);
            }
        }
        let mut out: Vec<(String, Card, f64)> = vec![];
        for held in &run.consumables {
            if out.iter().any(|(k, _, _)| k == &held.key) {
                continue;
            }
            let Some((effect, 1, 1)) = data.center(&held.key).and_then(|x| consumable::card_effect(&held.key, &x.config)) else { continue };
            let val = |card: &Card, rounds: usize| idx_of(card).map(|i| lr.deck_value(&consumable::apply(effect, deck, &[i]), 0.0, rounds));
            // its searched target, not the best of noisy values; never below not using it
            // (1.0: your deck as it is, exact on the same rounds)
            let target = tarots.iter().find(|t| t.key == held.key).and_then(|t| t.deck.clone());
            let now = |rounds: usize| target.as_ref().map_or(1.0, |d| lr.deck_value(d, 0.0, rounds)).max(1.0);
            // a quick screen, then what clears the noise margin on the full projection
            let (now_q, now_f) = (now(TARGET_SCREEN_ROUNDS), now(value::TAROT_ROUNDS));
            let quick = par_map(&kinds, |k| val(k, TARGET_SCREEN_ROUNDS).unwrap_or(0.0) - now_q);
            let likely: Vec<Card> = kinds.iter().zip(&quick).filter(|(_, g)| **g > compare::EQUAL).map(|(k, _)| *k).collect();
            let full = par_map(&likely, |k| val(k, value::TAROT_ROUNDS).unwrap_or(0.0) - now_f);
            for (k, g) in likely.into_iter().zip(full) {
                if g > compare::EQUAL {
                    out.push((held.key.clone(), k, g));
                }
            }
        }
        out
    };
    let worth_drawing: Vec<(String, String, f64)> = seen
        .iter()
        .map(|(k, c, g)| (c.label(), run.consumables.iter().find(|x| &x.key == k).map_or(k.clone(), |x| x.name.clone()), *g))
        .collect();
    // Look-ahead: each candidate first move is simulated through the rest of the round.
    let look = ctx.specs.iter().find(|x| x.in_progress).map(|spec| {
        let mut bb = ctx.board_for(&b, spec);
        // The simulated player plays toward the same measure the advice ranks by
        let goals = sim::RoundGoals { planet: std::array::from_fn(|i| planet_gain_by[i]), dollar: dollar_gain, per_hand: run.money_per_hand, seen: seen.clone(), ..Default::default() };
        bb.goals = Some(goals.clone());
        let start = ctx.start_for(spec, &bb);
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
        let to_opt = |m: &sim::Move, hand: &[Card], board: &Board, (p, mean, spare, cash, planets): (f64, f64, f64, f64, f64), use_first: Option<String>| {
            // A play is shown in the order to play it; a discard by rank (order doesn't matter)
            let sorted_discard;
            let (action, idx) = match m {
                sim::Move::Play(v) => ("play", v),
                sim::Move::Discard(v) => {
                    let mut v = v.clone();
                    v.sort_by_key(|&i| (std::cmp::Reverse(hand[i].rank.0), hand[i].suit as u8));
                    sorted_discard = v;
                    ("discard", &sorted_discard)
                }
            };
            let mut idx = idx.clone();
            let (name, score, dig, arrange) = if action == "play" {
                let (order, arrange, o) = arranged(board, hand, &idx, hand_order);
                let dig = idx.len().saturating_sub(o.scoring.len());
                idx = order;
                (o.hand.name().to_string(), o.score, dig, arrange)
            } else {
                (String::new(), 0.0, 0, (vec![], vec![]))
            };
            let cards: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
            // cards a consumable added aren't in your hand yet: no position
            let indices = idx.iter().filter(|&&i| i < hand_order.len()).map(|&i| hand_order[i]).collect();
            PlayOption { spare_hands: spare, round_money: cash, action: action.into(), cards: cards.iter().map(Card::label).collect(), indices, dig, hand: name, score, p_win: p, mean_total: mean, use_first, planets, tie: false, then: None, arrange: arrange.0, arrange_indices: arrange.1 }
        };
        // Exact ties are broken by the cards themselves, so the order your hand is sorted in
        // never changes the advice
        let canon = |m: &sim::Move, hand: &[Card]| {
            let (kind, idx) = match m {
                sim::Move::Play(v) => (0, v),
                sim::Move::Discard(v) => (1, v),
            };
            let mut k: Vec<_> = idx.iter().map(|&i| hand[i].order_key()).collect();
            k.sort();
            (kind, k)
        };
        // Every move: every play and every discard, for your hand as it is and after each
        // held consumable (used first). No move needs a rule to be considered.
        type Cand = (sim::Move, Board, RoundStart, Vec<sim::Use>, Option<String>);
        let moves_of = |st: &RoundStart| {
            let mut v = sim::all_plays(&st.hand);
            v.extend(sim::all_discards(&st.hand, st.discards));
            v
        };
        let mut cands: Vec<Cand> = moves_of(&start).into_iter().map(|m| (m, bb.clone(), start.clone(), uses.clone(), None)).collect();
        for (k, u) in uses.iter().enumerate() {
            if uses[..k].iter().any(|x| x.name == u.name) {
                continue;
            }
            let Some((b2, h2)) = u.apply(&bb, &start.hand) else { continue };
            let rest: Vec<sim::Use> = uses.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, x)| x.clone()).collect();
            let s2 = RoundStart { hand: h2, ..start.clone() };
            for m in moves_of(&s2) {
                cands.push((m, b2.clone(), s2.clone(), rest.clone(), Some(u.name.clone())));
            }
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
        let ck = |c: usize| (cands[c].4.clone(), canon(&cands[c].0, &cands[c].2.hand));
        let budget = |done: usize| SEARCH_BUDGET.iter().find(|b| done <= b.0).map_or(SEARCH_BUDGET[SEARCH_BUDGET.len() - 1].1, |b| b.1);
        let race = compare::race(
            cands.len(),
            SEARCH_FIRST,
            compare::MAX,
            budget,
            |c, rounds| {
                let (m, b, st, u, _) = &cands[c];
                sim::outcomes_after(b, st, m, rounds, seed, u)
            },
            utility,
            progress,
            |x, y| ck(y).cmp(&ck(x)),
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
                let mut o = to_opt(m, &st.hand, b, (avg(|o| o.won), avg(|o| o.total), avg(|o| o.spare), avg(|o| o.cash), avg(|o| o.planets)), use_first.clone());
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
        order.sort_by(|&x, &y| (y == leader).cmp(&(x == leader)).then(tied[y].cmp(&tied[x])).then(score_of(y).total_cmp(&score_of(x))).then(ck(x).cmp(&ck(y))));
        // Moves as good as the leader are equal as far as can be told: among them a play that
        // wins the round right now goes first (nothing to gain by waiting), then the best
        // estimate, then how close lost rounds came, then by the cards themselves. The estimate
        // is paired: the leader's value plus each one's difference to it on the rounds both
        // were sampled on (ties stop early; their own averages are over fewer, other rounds).
        let need = start.target - start.scored;
        let wins_now = |o: &PlayOption| o.action == "play" && o.use_first.is_none() && o.score >= need;
        let group = order.iter().take_while(|&&c| c == leader || tied[c]).count();
        let prog = |c: usize| outs[c].iter().map(progress).sum::<f64>() / outs[c].len().max(1) as f64;
        let estimate = |c: usize| {
            let k = outs[c].len().min(outs[leader].len());
            score_of(leader) + mean_u(&outs[c][..k]) - mean_u(&outs[leader][..k])
        };
        order[..group].sort_by(|&x, &y| wins_now(&opts[y]).cmp(&wins_now(&opts[x])).then(estimate(y).total_cmp(&estimate(x))).then(prog(y).total_cmp(&prog(x))).then(ck(x).cmp(&ck(y))));
        let first = order[0];
        for &c in &order {
            opts[c].tie = c != first && (c == leader || tied[c]);
        }
        // the pick against the reference's best, on a block of rounds neither was chosen on
        let gap = (!reference.is_empty()).then(|| {
            let best = (0..reference.len()).max_by(|&x, &y| reference[x].total_cmp(&reference[y]).then(ck(y).cmp(&ck(x)))).unwrap_or(0);
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
        let mut reference: Vec<(String, f64, String)> = reference.iter().enumerate().map(|(c, v)| (label(&opts[c]), *v, fate[c].clone())).collect();
        reference.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut sorted: Vec<PlayOption> = order.into_iter().map(|c| opts[c].clone()).collect();
        sorted.dedup_by(|a, b| a.action == b.action && a.cards == b.cards && a.use_first == b.use_first);
        (sorted, reference, gap)
    });
    let reference = look.as_ref().map_or(vec![], |l| l.1.clone());
    let reference_gap = look.as_ref().and_then(|l| l.2);
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
            let tip = if missing.is_empty() {
                None
            } else {
                let m = format!("Not used in this look-ahead (no modelled effect on this hand): {}", missing.join(", "));
                Some(m)
            };
            Some(PlayAdvice { reference, reference_gap, worth_drawing: worth_drawing.clone(), then: best.then.clone(), ties: opts.iter().filter(|o| o.tie).count(), planets: Some(best.planets), use_first: best.use_first, spare_hands: Some(best.spare_hands), round_money: Some(best.round_money), action: best.action, cards: best.cards, arrange: best.arrange, arrange_indices: best.arrange_indices, dig: best.dig, indices: best.indices, hand: best.hand, score: best.score, p_win: Some(best.p_win), alternatives: opts, tip })
        }
        None => {
            let hand: Vec<Card> = hand_order.iter().map(|&i| run.hand[i]).collect();
            sim::best_play(&b, &hand).map(|p| {
            let (order, (arrange, arrange_indices), _) = arranged(&b, &hand, &p.cards, hand_order);
            PlayAdvice {
            use_first: None,
            reference: vec![],
            reference_gap: None,
            worth_drawing: worth_drawing.clone(),
            planets: None,
            then: None,
            ties: 0,
            spare_hands: None,
            round_money: None,
            action: "play".into(),
            cards: order.iter().map(|&i| hand[i].label()).collect(),
            arrange,
            arrange_indices,
            dig: 0,
            indices: order.iter().map(|&i| hand_order[i]).collect(),
            hand: p.hand.name().to_string(),
            score: p.floor,
            p_win: None,
            alternatives: vec![],
            tip: None,
            }
            })
        }
    }
}

/// The play `idx` from `hand` (in `Card::order_key` order; `hand_order`: each card's place in
/// your hand, cards a consumable added have none) in the order to play it; your hand left to
/// right as it should sit (the cards played in that order, then the cards kept), as labels and
/// as the places they're in now, when the hand as it sits scores less (the game plays the
/// selected cards in the order they sit: state_events.lua `G.FUNCS.play_cards_from_highlighted`),
/// else none; and the play's outcome. The engine arranges the cards (`engine::score`), from the
/// cards in a fixed order, so how your hand is sorted never changes the score.
#[allow(clippy::type_complexity)]
fn arranged(board: &Board, hand: &[Card], idx: &[usize], hand_order: &[usize]) -> (Vec<usize>, (Vec<String>, Vec<usize>), crate::engine::Outcome) {
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
    let as_they_sit = score_as_played(board, &cards(&on_screen(idx)), &cards(&on_screen(&rest)), &mut Unlucky, false).score;
    let arrange = if as_they_sit < o.score {
        let all: Vec<usize> = play.iter().chain(&keep).copied().collect();
        (all.iter().map(|&i| hand[i].label()).collect(), all.iter().filter_map(|&i| hand_order.get(i).copied()).collect())
    } else {
        (vec![], vec![])
    };
    (play, arrange, o)
}
