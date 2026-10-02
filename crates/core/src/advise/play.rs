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
    let has = |k: Kind| b.jokers.iter().any(|j| j.kind == k && !j.debuff);
    let tip = (run.discards_left > 0 && has(Kind::MysticSummit) && !has(Kind::Banner)).then(|| {
        format!(
            "Use your {} discard{} first on cards outside this play: Mystic Summit gives +15 Mult on every hand once none are left",
            run.discards_left,
            if run.discards_left > 1 { "s" } else { "" }
        )
    });
    // Look-ahead: each candidate first move is simulated through the rest of the round.
    let look = ctx.specs.iter().find(|x| x.in_progress).map(|spec| {
        let mut bb = ctx.board_for(&b, spec);
        // The simulated player plays toward the same measure the advice ranks by
        let goals = sim::RoundGoals { planet: std::array::from_fn(|i| planet_gain_by[i]), dollar: dollar_gain, per_hand: run.money_per_hand };
        bb.goals = Some(goals);
        // Holding Cryptid: drawing a Blue Seal card this round means two more of it (an
        // estimate: about three planets' worth)
        if run.consumables.iter().any(|c| c.key == "c_cryptid") && !run.hand.iter().any(|c| c.seal == Some(crate::model::Seal::Blue)) {
            bb.seal_seen_value = 15.0;
        }
        let start = ctx.start_for(spec, &bb);
        let seed = ctx.opts.seed;
        // Consumables you hold, as the round can use them: tarots and spectral cards by
        // what they do to your hand, planets by the level they add. Every move is simulated
        // with them still held (used once they improve the best play in hand), and each
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
                Some(sim::Use { name: center.name.clone(), levels, planet: true, ..Default::default() })
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
            let cards: Vec<Card> = idx.iter().map(|&i| hand[i]).collect();
            let (name, score, dig) = if action == "play" {
                let held: Vec<Card> = (0..hand.len()).filter(|i| !idx.contains(i)).map(|i| hand[i]).collect();
                let o = crate::engine::score(board, &cards, &held, &mut crate::engine::Unlucky, false);
                let scoring = crate::engine::hand::detect(&cards, board.rule_flags()).scoring.len();
                (o.hand.name().to_string(), o.score, cards.len().saturating_sub(scoring))
            } else {
                (String::new(), 0.0, 0)
            };
            // cards a consumable added aren't in your hand yet: no position
            let indices = idx.iter().filter(|&&i| i < hand_order.len()).map(|&i| hand_order[i]).collect();
            PlayOption { spare_hands: spare, round_money: cash, action: action.into(), cards: cards.iter().map(Card::label).collect(), indices, dig, hand: name, score, p_win: p, mean_total: mean, use_first, planets, tie: false, then: None }
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
        // Every move, narrowed down in stages on the same measure as the final comparison:
        // every play and every discard on a few quick rounds, the best on more, the best of
        // those into the comparison. No move needs a rule to be considered. The screen uses
        // draws of its own (the comparison then judges the survivors on fresh ones), all moves
        // of a stage on the same draws. A cut drops only moves clearly worse than the stage's
        // leader (paired, 95%), up to the stage's size; where the value can't separate moves
        // (a long shot loses nearly every round), points toward the target do.
        let screen_seed = seed ^ 0x5c4e_e75c;
        let screen = |b: &Board, st: &RoundStart, u: &[sim::Use]| -> Vec<sim::Move> {
            let mut pool: Vec<sim::Move> = sim::all_plays(&st.hand);
            pool.extend(sim::all_discards(&st.hand, st.discards));
            let progress = |o: &sim::Outcome| o.total / st.target.max(1.0);
            for &(n, keep) in SCREEN_STAGES {
                let outs: Vec<Vec<sim::Outcome>> = par_map(&pool, |m| sim::outcomes_after(b, st, m, 0..n, screen_seed, u));
                let mean = |v: &[sim::Outcome], f: &dyn Fn(&sim::Outcome) -> f64| v.iter().map(f).sum::<f64>() / v.len().max(1) as f64;
                let val: Vec<f64> = outs.iter().map(|v| mean(v, &|o| goals.value(o))).collect();
                let prog: Vec<f64> = outs.iter().map(|v| mean(v, &progress)).collect();
                let mut order: Vec<usize> = (0..pool.len()).collect();
                order.sort_by(|&a, &c| val[c].total_cmp(&val[a]).then(prog[c].total_cmp(&prog[a])).then(canon(&pool[a], &st.hand).cmp(&canon(&pool[c], &st.hand))));
                let lead = order[0];
                let clearly_worse = |c: usize| {
                    let d: Vec<f64> = outs[lead].iter().zip(&outs[c]).map(|(x, y)| goals.value(x) - goals.value(y)).collect();
                    let k = d.len() as f64;
                    let m = d.iter().sum::<f64>() / k;
                    let se = (d.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / (k - 1.0).max(1.0) / k).sqrt();
                    m - 2.0 * se > 0.0
                };
                pool = order.into_iter().filter(|&c| c == lead || !clearly_worse(c)).take(keep).map(|i| pool[i].clone()).collect();
            }
            pool
        };
        // Every candidate: the moves with your consumables kept, and each held consumable used
        // first, then the moves after it
        type Cand = (sim::Move, Board, RoundStart, Vec<sim::Use>, Option<String>);
        let mut cands: Vec<Cand> = screen(&bb, &start, &uses).into_iter().map(|m| (m, bb.clone(), start.clone(), uses.clone(), None)).collect();
        for (k, u) in uses.iter().enumerate() {
            if uses[..k].iter().any(|x| x.name == u.name) {
                continue;
            }
            let Some((b2, h2)) = u.apply(&bb, &start.hand) else { continue };
            let rest: Vec<sim::Use> = uses.iter().enumerate().filter(|(j, _)| *j != k).map(|(_, x)| x.clone()).collect();
            let s2 = RoundStart { hand: h2, ..start.clone() };
            for m in screen(&b2, &s2, &rest) {
                cands.push((m, b2.clone(), s2.clone(), rest.clone(), Some(u.name.clone())));
            }
        }
        // Compared on one measure per simulated round, as the shop ranking does: winning it,
        // times the long-run value of what it leaves you (the planets Blue Seals held at the
        // end make, and its money: hands left over at cash out and what it pays as you go,
        // each by its gain in the long-run projection). A lost round counts 0. All on the same draws,
        // compared in batches until the best is clear (see `compare`).
        let utility = |o: &sim::Outcome| goals.value(o);
        let mean_u = |v: &[sim::Outcome]| v.iter().map(utility).sum::<f64>() / v.len().max(1) as f64;
        let ck = |c: usize| (cands[c].4.clone(), canon(&cands[c].0, &cands[c].2.hand));
        let race = compare::race(
            cands.len(),
            |c, rounds| {
                let (m, b, st, u, _) = &cands[c];
                sim::outcomes_after(b, st, m, rounds, seed, u)
            },
            utility,
            |x, y| ck(y).cmp(&ck(x)),
        );
        let (outs, leader, tied) = (race.samples, race.leader, race.tied);
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
        // wins the round right now goes first (nothing to gain by waiting), then the one that
        // scores more toward the target, then by the cards themselves
        let need = start.target - start.scored;
        let wins_now = |o: &PlayOption| o.action == "play" && o.use_first.is_none() && o.score >= need;
        let group = order.iter().take_while(|&&c| c == leader || tied[c]).count();
        let progress = |c: usize| outs[c].iter().map(|o| o.total).sum::<f64>() / outs[c].len().max(1) as f64;
        order[..group].sort_by(|&x, &y| wins_now(&opts[y]).cmp(&wins_now(&opts[x])).then(progress(y).total_cmp(&progress(x))).then(ck(x).cmp(&ck(y))));
        let first = order[0];
        for &c in &order {
            opts[c].tie = c != first && (c == leader || tied[c]);
        }
        let mut sorted: Vec<PlayOption> = order.into_iter().map(|c| opts[c].clone()).collect();
        sorted.dedup_by(|a, b| a.action == b.action && a.cards == b.cards && a.use_first == b.use_first);
        let opts = sorted;
        opts
    });
    match look.filter(|o| !o.is_empty()) {
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
                tip
            } else {
                let m = format!("Not used in this look-ahead (no modelled effect on this hand): {}", missing.join(", "));
                Some(tip.map_or(m.clone(), |t| format!("{t}. {m}")))
            };
            Some(PlayAdvice { then: best.then.clone(), ties: opts.iter().filter(|o| o.tie).count(), planets: Some(best.planets), use_first: best.use_first, spare_hands: Some(best.spare_hands), round_money: Some(best.round_money), action: best.action, cards: best.cards, dig: best.dig, indices: best.indices, hand: best.hand, score: best.score, p_win: Some(best.p_win), alternatives: opts, tip })
        }
        None => sim::best_play(&b, &hand_order.iter().map(|&i| run.hand[i]).collect::<Vec<_>>()).map(|p| PlayAdvice {
            use_first: None,
            planets: None,
            then: None,
            ties: 0,
            spare_hands: None,
            round_money: None,
            action: "play".into(),
            cards: p.cards.iter().map(|&i| run.hand[hand_order[i]].label()).collect(),
            dig: 0,
            indices: p.cards.iter().map(|&i| hand_order[i]).collect(),
            hand: p.hand.name().to_string(),
            score: p.floor,
            p_win: None,
            alternatives: vec![],
            tip,
        }),
    }
}
