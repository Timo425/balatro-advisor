# The owner's view

The owner's own judgement of where the advisor stands and which gaps matter. Only the owner's
words go here, quoted or relayed, each dated; a chat edits it only to write down what the owner
said. `tools/status.py` reads the ratings below; CLAUDE.md says when to read this file.

## The goal, in the owner's words

- **2026-11-02** "in the long run i'd like the advisor to be as good as me or better. and if we
  manage to hit that point, maybe we can learn from better players". On the bot: it's a work in
  progress, ironed out so it plays end to end; "bot running is proportional to the task and to
  the bot's current state".
- **2026-11-10** "the goal is to have a repo that [...] can beat gold stake (and then as an extra
  has enough architecture and design to keep improving it, by analyzing actually good players";
  "beating gold stake was the original goal (advisor being at that level and also later -
  independent botrunner) then later the idea that ofc we would want it to play even better and
  not take a whole day to beat it once". Goalposts like "as good as me" are "for now, then lets
  take it from there if we manage to do it". On learning from better players: "idk what would
  be the best way to get step by step from a youtube video and i doubt a transscript would do
  it so i probably need to manually play equal to them to get it recoded" ("i have some stuff
  setup for it already").
- **2026-11-11** "I Just created this repo to have a balatro advisor, something that can help me
  beat gold stakes more easily. while i can beat it sometimes, a pro player can beat it more
  often than not, so there is definitely a lot of hidden potential there"; the bot, added later,
  "should also be able to beat gold stake and in a reasonable time not like run for 3 days or
  something per session". "i dont like micromanaging the repo much deeper than that like
  ordering or judging cards and stuff like that, but i try to keep tabs on what an agent is
  doing because sometimes at least opus tends to tunnel vision on specific problems too much
  and lose the big picture".

## Speed and priority, in the owner's words

- **2026-11-10** On how long the advice may take: "it should be a balance thing, wouldnt want a
  shop to take 20 mins when a 30 second shop can do most of the same kind of thing"; and the
  worry that the advisor may be "at my level or better" only when it isn't "relatively fast
  (like not 8 mins slow)". On the logged times of October: "the times you are comparing against
  is when the advisor was weaker and simpler, i havent played since".
- **2026-11-10** On priority: "i've struggled with priority a couple of times"; against "the
  queue being stacked on a task and then spawning many subtasks for it as the work continues,
  which could end up claude spending weeks on edge cases before returning to main cards that
  actually affect things most of the time".
- **2026-11-11** On the work's own speed: "developing the repo is already quite slow, like hours
  or sometimes almost a day for a single task, so if we add optimization tasks there we are
  just getting nowhere"; the wish: "not blindly spend time on results when a much quicker but
  more or less same result can happen". On comparing with October: "shop was likely much faster
  before because it was also considerably more basic, something that claude agents seem to miss
  when conmparing to past state". On the queue's order: "queue order by my rating happened
  because at one point if felt like claude kept working on some less important tasks for days
  and not getting to the juicy ones, but im also not too interested in micromanaging the tasks
  order myself, i barely understand most of them at this stage".

## The state, in the owner's words

- **2026-10-27** "I don't think the advisor can beat ante 8 as is."
- **2026-10-28** The last bot test skipped every booster pack (thought fixed since) and its joker
  buy evaluations seemed off (days of work done on them since). (Relayed from a brainstorm.)
- **2026-10-28** Restarts at Ante 1 when the jokers look weak, by judgement and not always
  perfectly: "at times I play a not very hopeful joker out a bit."
- **2026-10-30** How they buy: "when i buy jokers, early i just buy jokers that truly help me,
  like money is more important"; "buying jokers is more about 'what is worth buying and keeps me
  afloat' and not 'what will this combine with', other than chips and mult combining better
  than just 2 mults or 2 chips and then xmult is better and better the later ante [...] i just
  buy things that boost my score, its rarely about the combos because most jokers are not even
  good enough on their own."
- **2026-10-30** On the bot (seeing three rentals held with little money): "nowhere close to
  probably desperate enough to need all these AND they all are chips anyway so dont scale that
  much"; it "doesnt seem to have any strategy on mind when it comes to building joker set and
  economy properly".

## Each gap's impact

The gap's number and title are `docs/design.md`'s "Known gaps". Impact: high, medium, low or ?
(not rated yet), with the owner's one-line reason.

| Gap | Title                                                                                                                                                                                                                                     | Impact    | Why                                                                                                                                           |
| ---:| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------- | --------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | Best play's search still misses sometimes                                                                                                                                                                                                 | low       | seems like low impact finetuning                                                                                                              |
| 2   | Best play and the simulated round try one target per consumable                                                                                                                                                                           | medium    | i think consumable targets need better support but idk how much of that this is                                                               |
| 3   | Random effects and DNA reach only part of the search                                                                                                                                                                                      | low       | seems like an edge case, depending on what random effects are                                                                                 |
| 4   | The simulated round only reports planets, money, hands left and the best card it drew for a held consumable (seen)                                                                                                                        | medium    | you said gap 2 and tarots so seems at least medium                                                                                            |
| 5   | Valuation is still split                                                                                                                                                                                                                  | high      | I don't like things being split, but does forcing unification work everywhere?                                                                |
| 6   | The simulated player's policy (decide_cards) is still partly a rulebook                                                                                                                                                                   | high      | i dont like relying on hardcoded rules where reasonable                                                                                       |
| 7   | Shop ranking and skip-or-play aren't confidence-based                                                                                                                                                                                     | high      | this probably should be aligned with best play?                                                                                               |
| 8   | The policy weighs what a discard does to the jokers only in this round's points, and only in its digs                                                                                                                                     | medium    | seems important                                                                                                                               |
| 9   | The long-run projection spends money on your main hand's planets                                                                                                                                                                          | medium    | idk what the issue is here but long run projections are important i think                                                                     |
| 10  | Packs are valued in two places, sized by their label                                                                                                                                                                                      | high      | seems like we  dont want to value them in 2 places?                                                                                           |
| 11  | advise.rs still holds the shop options, tarot valuation and outlook inline                                                                                                                                                                | high      | something to clean up then                                                                                                                    |
| 12  | Consumable slots, hands, discards and hand size grow only by a voucher bought now, and a slot only counts for Blue Seals                                                                                                                  | high      |                                                                                                                                               |
| 13  | The long run doesn't know the run ends                                                                                                                                                                                                    | high      |                                                                                                                                               |
| 14  | You can't keep a joker out of the sell choices                                                                                                                                                                                            | high      |                                                                                                                                               |
| 15  | Bosses whose rule isn't in the simulated round are played as plain blinds                                                                                                                                                                 | high      |                                                                                                                                               |
| 16  | What isn't valued ranks as worth nothing, and an empty slot counts as filled                                                                                                                                                              | high      |                                                                                                                                               |
| 17  | The money's finds add every joker's gain, count a full board's swaps in two units, and the judge can't play tarots                                                                                                                        | highest   | i recall this being an annoyance in one of the tasks, took 3 days to do the task and it kept telling me how the judge doesnt do this and that |
| 18  | Growth is played for at Best play's own move, the simulated player's finish and its plays that cost no pace, in its digs only as this round's points (gap 8), and a joker the projection replaces counts only in this ante's blinds ahead | high      |                                                                                                                                               |
| 19  | The card order tried is one proposal, checked, not a search                                                                                                                                                                               | medium?   |                                                                                                                                               |
| 20  | Big hands: moves from the structure, screened by their score now                                                                                                                                                                          | medium    |                                                                                                                                               |
| 21  | Of the tags you hold only the Juggle Tag is read, and a tag's effect on a later blind is valued only in one case                                                                                                                          | high      |                                                                                                                                               |
| 22  | A joker bought or sold during a blind doesn't change that blind's hand size or discards                                                                                                                                                   | medium    |                                                                                                                                               |
| 23  | Shop options are ranked as alternatives, and a reroll doesn't count the shop it replaces                                                                                                                                                  | high      |                                                                                                                                               |
| 24  | A Stone card's rank and suit are shown and used, though the game hides them                                                                                                                                                               | medium    |                                                                                                                                               |
| 25  | After the first move, the simulated player knows the face-down cards                                                                                                                                                                      | high      | 2026-11-02, on what is left: every read of a face-down card high ((a) look-ahead, (b) tarot targets, (d) names, (f) fallback): "the advisor tool seeing things i dont see seems important"; (c) big hands, (e) low |
| 26  | A joker added to a board goes in one fixed place, so the jokers' order isn't the one you'd pick                                                                                                                                           | very high | is this botrunner or advisor? advisor rearranges. seems important                                                                             |
| 27  | The long run can't see a joker's future partner, so nothing builds toward a combo                                                                                                                                                         | high      | this seems very important but also.. very prone to heavy searching and overhead                                                               |
| 28  | Every blind ahead starts from your board as it is now, not the one the blinds before it leave                                                                                                                                             | medium    | not sure i understood                                                                                                                         |
| 29  | Plays that win the round now and are worth the same are shown as one arbitrary pick                                                                                                                                                       | medium    | which is the issue here? is this about seals etc?                                                                                             |
| 30  | Best play trades win chance for money by a dollar's weight that was never measured                                                                                                                                                        | high      | best play doing win/money/tarot etc considerations well is important                                                                          |
| 31  | Best play says "use Death first" without naming its cards                                                                                                                                                                                 | high      |                                                                                                                                               |
| 32  | An open pack's "skip" wins ties, even when skipping keeps nothing                                                                                                                                                                         | highest   | also how about taking just to sell for money consideration                                                                                    |
| 33  | Selling a Negative joker is counted as making room                                                                                                                                                                                        | very high | fix                                                                                                                                           |
| 34  | The simulated player never banks a free hand before the hand that wins                                                                                                                                                                    | medium    | i donbt understand this one                                                                                                                   |
| 35  | Every face-down card is named "face-down card", so the page can't say which to throw                                                                                                                                                      | high      | same as the other face up card one? or similar, adjacent.                                                                                     |
| 36  | Money past the cap buys nothing in the long run, and the race for the joker a buy replaces counts saves at the two bosses only                                                                                                            | high      | seems important for the economy or shop                                                                                                       |
| 37  | The jokers' order is one order for the board, never chosen for the moment it acts in                                                                                                                                                      | high      | order may need consideration at every action in specific situations, like copy or duplicationg consumables etc                                |
| 38  | The shop pool assumes every joker unlocked                                                                                                                                                                                                | low       | white stakes and not finished profiles are low prio for now                                                                                   |
