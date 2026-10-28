# The owner's view

The owner's own judgement of where the advisor stands and which gaps matter. Only the owner's
words go here, quoted or relayed, each dated; a chat edits it only to write down what the owner
said. `tools/status.py` reads the ratings below; CLAUDE.md says when to read this file.

## The state, in the owner's words

- **2026-10-27** "I don't think the advisor can beat ante 8 as is."
- **2026-10-28** The last bot test skipped every booster pack (thought fixed since) and its joker
  buy evaluations seemed off (days of work done on them since). (Relayed from a brainstorm.)
- **2026-10-28** Restarts at Ante 1 when the jokers look weak, by judgement and not always
  perfectly: "at times I play a not very hopeful joker out a bit."

## Each gap's impact

The gap's number and title are `docs/design.md`'s "Known gaps". Impact: high, medium, low or ?
(not rated yet), with the owner's one-line reason.

| Gap | Title | Impact | Why |
| ---: | --- | --- | --- |
| 1 | Best play's search still misses sometimes | ? | |
| 2 | Best play and the simulated round try one target per consumable | ? | |
| 3 | Random effects and DNA reach only part of the search | ? | |
| 4 | The simulated round only reports planets, money, hands left and the best card it drew for a held consumable (seen) | ? | |
| 5 | Valuation is still split | ? | |
| 6 | The simulated player's policy (decide_cards) is still partly a rulebook | ? | |
| 7 | Shop ranking and skip-or-play aren't confidence-based | ? | |
| 8 | The policy weighs what a discard does to the jokers only in this round's points, and only in its digs | ? | |
| 9 | The long-run projection spends money on your main hand's planets | ? | |
| 10 | Packs are valued in two places, sized by their label | ? | |
| 11 | advise.rs still holds the shop options, tarot valuation and outlook inline | ? | |
| 12 | Consumable slots, hands, discards and hand size grow only by a voucher bought now, and a slot only counts for Blue Seals | ? | |
| 13 | The long run doesn't know the run ends | ? | |
| 14 | You can't keep a joker out of the sell choices | ? | |
| 15 | Bosses whose rule isn't in the simulated round are played as plain blinds | ? | |
| 16 | What isn't valued ranks as worth nothing, and an empty slot counts as filled | ? | |
| 17 | The money's finds add every joker's gain, count a full board's swaps in two units, and the judge can't play tarots | ? | |
| 18 | Growth is played for at Best play's own move, the simulated player's finish and its plays that cost no pace, in its digs only as this round's points (gap 8), and a joker the projection replaces counts only in this ante's blinds ahead | ? | |
| 19 | The card order tried is one proposal, checked, not a search | ? | |
| 20 | Big hands: moves from the structure, screened by their score now | ? | |
| 21 | Of the tags you hold only the Juggle Tag is read, and a tag's effect on a later blind is valued only in one case | ? | |
| 22 | A joker bought or sold during a blind doesn't change that blind's hand size or discards | ? | |
| 23 | Shop options are ranked as alternatives, and a reroll doesn't count the shop it replaces | ? | |
| 24 | A Stone card's rank and suit are shown and used, though the game hides them | ? | |
| 25 | A face-down card is never played, even when the hand's sort shows what it is | ? | |
| 26 | A joker added to a board goes in one fixed place, so the jokers' order isn't the one you'd pick | ? | |
| 27 | The long run can't see a joker's future partner, so nothing builds toward a combo | ? | |
| 28 | Every blind ahead starts from your board as it is now, not the one the blinds before it leave | ? | |
| 29 | Plays that win the round now and are worth the same are shown as one arbitrary pick | ? | |
| 30 | Best play trades win chance for money by a dollar's weight that was never measured | ? | |
| 31 | Best play says "use Death first" without naming its cards | ? | |
| 32 | An open pack's "skip" wins ties, even when skipping keeps nothing | ? | |
| 33 | Selling a Negative joker is counted as making room | ? | |
| 34 | The simulated player never banks a free hand before the hand that wins | ? | |
| 35 | Every face-down card is named "face-down card", so the page can't say which to throw | ? | |
| 36 | Money past the cap buys nothing in the long run, and the race for the joker a buy replaces counts saves at the two bosses only | ? | |
| 37 | The jokers' order is one order for the board, never chosen for the moment it acts in | ? | |
| 38 | The shop pool assumes every joker unlocked | ? | |
