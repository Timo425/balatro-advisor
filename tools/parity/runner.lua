-- The parity tier's job runner (docs/decisions.md D17), run by crates/core/tests/parity.rs as
-- `luajit runner.lua` in this folder. Reads jobs from stdin, one a line:
--   <in> TAB <action> TAB <card ids, comma separated, in play order> TAB <out>
-- <in> is a save as plain text; action: play | discard | select_blind | cash_out | reroll |
-- open_pack (ids: the booster's place) | play_cash_out (the play, then the cash-out). Loads the
-- save through the game's own `Game:start_run`, takes the step through the game's own button
-- functions, runs the game's event queue until it rests and writes the game's own save of what
-- the step left (`save_run`, `STR_PACK`) to <out> as plain text. One line out a job:
--   ok TAB <chips the step scored> TAB <dollars it moved> TAB <G.STATE>
--   skip TAB <why the move isn't one the game takes>   |   fail TAB <error>
dofile('standin.lua')
local out = io.stdout
local verbose = os.getenv('PARITY_VERBOSE')
print = function(...) if verbose then io.stderr:write(table.concat({...}, ' '), '\n') end end

G:start_up()
G.E_MANAGER:clear_queue()
-- the tutorial, achievements and unlock pop-ups build UI the stand-in can't lay out
G.SETTINGS.tutorial_complete = true
G.SETTINGS.tutorial_progress = nil
update_canvas_juice = function() end
notify_alert = function() end
G.F_NO_SAVING = false

local REST = {}
for _, s in ipairs({'SELECTING_HAND', 'ROUND_EVAL', 'GAME_OVER', 'BLIND_SELECT', 'SHOP', 'TAROT_PACK', 'PLANET_PACK', 'SPECTRAL_PACK', 'STANDARD_PACK', 'BUFFOON_PACK'}) do REST[G.STATES[s]] = true end

local function busy()
  for _, q in pairs(G.E_MANAGER.queues) do
    for _, ev in ipairs(q) do if ev.blocking and not ev.no_delete then return true end end
  end
  return false
end

-- runs the game's frames until it rests in a state a player acts in (`want`, if given)
local function pump(max, want)
  local last, same = nil, 0
  for i = 1, max do
    G:update(1 / 20)
    local sig = tostring(G.STATE) .. ':' .. tostring(G.STATE_COMPLETE) .. ':' .. #G.E_MANAGER.queues.base
    if sig == last then same = same + 1 else same, last = 0, sig end
    -- resting behind events that wait on the UI (the win screen): rested
    if REST[G.STATE] and G.STATE_COMPLETE and same > 400 then return true end
    if REST[G.STATE] and G.STATE_COMPLETE and not busy() and (not want or G.STATE == want) then
      -- some states finish their setup a frame later
      for _ = 1, 3 do G:update(1 / 20) end
      if REST[G.STATE] and not busy() then return true end
    end
  end
  return false, 'did not rest (state ' .. tostring(G.STATE) .. ')'
end

-- a card's id as crates/core/src/save.rs `CardIds` reads it
local function id_of(c) return (c.params and c.params.playing_card) or (c.sort_id and -c.sort_id) end

-- puts the cards `ids` in the hand's places they hold, in that order, and highlights them
local function pick(ids)
  local want, slots, by = {}, {}, {}
  for k, id in ipairs(ids) do want[id] = k end
  for _, c in ipairs(G.hand.cards) do
    -- Cerulean Bell's card is always selected (blind.lua `Blind:drawn_to_hand`)
    if c.ability.forced_selection and not want[id_of(c)] then return 'Cerulean Bell forces a card the pick leaves out' end
  end
  G.hand:unhighlight_all()
  for i, c in ipairs(G.hand.cards) do
    local k = want[id_of(c)]
    if k then
      if by[k] then return 'two cards share an id (a copy)' end
      slots[#slots + 1] = i; by[k] = c
    end
  end
  if #slots ~= #ids then return 'cards not in hand' end
  for k, i in ipairs(slots) do G.hand.cards[i] = by[k] end
  G.hand:align_cards()
  for i, c in ipairs(G.hand.cards) do c.T.x = i end
  -- the forced card is highlighted already: highlighting it twice counts it twice
  for k = 1, #ids do if not by[k].highlighted then G.hand:add_to_highlighted(by[k], true) end end
end

local function read(path) local f = assert(io.open(path, 'rb')); local s = f:read('*a'); f:close(); return s end

local function run(job)
  if G.STAGE == G.STAGES.RUN then G:delete_run() end
  G.E_MANAGER:clear_queue()
  G:start_run({savetext = STR_UNPACK(read(job.input))})
  local ok, err = pump(4000)
  if not ok then return 'fail\tload: ' .. err end
  local chips0, dollars0 = G.GAME.chips, G.GAME.dollars
  local a = job.action
  if a == 'play' or a == 'discard' or a == 'play_cash_out' then
    if G.STATE ~= G.STATES.SELECTING_HAND then return 'fail\tnot selecting a hand (state ' .. G.STATE .. ')' end
    local why = pick(job.ids)
    if why then return 'skip\t' .. why end
    if a == 'discard' then G.FUNCS.discard_cards_from_highlighted() else G.FUNCS.play_cards_from_highlighted() end
  elseif a == 'select_blind' then
    if G.STATE ~= G.STATES.BLIND_SELECT then return 'fail\tnot selecting a blind (state ' .. G.STATE .. ')' end
    G.FUNCS.select_blind({config = {ref_table = G.P_BLINDS[G.GAME.round_resets.blind_choices[G.GAME.blind_on_deck]]}})
  elseif a == 'reroll' then
    if G.STATE ~= G.STATES.SHOP then return 'fail\tnot in the shop (state ' .. G.STATE .. ')' end
    G.FUNCS.reroll_shop()
  elseif a == 'open_pack' then
    if G.STATE ~= G.STATES.SHOP then return 'fail\tnot in the shop (state ' .. G.STATE .. ')' end
    local c = G.shop_booster and G.shop_booster.cards[job.ids[1]]
    if not c then return 'fail\tno such booster' end
    G.FUNCS.use_card({config = {ref_table = c}})
  elseif a == 'cash_out' then
    if G.STATE ~= G.STATES.ROUND_EVAL then return 'fail\tnot cashing out (state ' .. G.STATE .. ')' end
    G.FUNCS.cash_out({config = {}})
  else
    return 'fail\tunknown action ' .. a
  end
  ok, err = pump(20000)
  if not ok then return 'fail\tstep: ' .. err end
  if a == 'play_cash_out' then
    -- a cash-out runs on from the winning hand's state: a cash-out screen loaded from its save
    -- has lost the blind's reward
    if G.STATE ~= G.STATES.ROUND_EVAL then return 'fail\tthe hand did not win the round (state ' .. G.STATE .. ')' end
    G.FUNCS.cash_out({config = {}})
    ok, err = pump(20000, G.STATES.SHOP)
    if not ok then return 'fail\tcash out: ' .. err end
  end
  save_run()
  local f = assert(io.open(job.output, 'wb')); f:write(STR_PACK(G.ARGS.save_run)); f:close()
  return string.format('ok\t%.17g\t%.17g\t%d', G.GAME.chips - chips0, G.GAME.dollars - dollars0, G.STATE)
end

for line in io.stdin:lines() do
  local input, action, ids, output = line:match('^([^\t]*)\t([^\t]*)\t([^\t]*)\t([^\t]*)$')
  local res
  if input then
    local list = {}
    for x in ids:gmatch('[^,]+') do list[#list + 1] = tonumber(x) end
    local ok, r = xpcall(run, debug.traceback, {input = input, action = action, ids = list, output = output})
    res = ok and r or ('fail\t' .. tostring(r):gsub('[\t\n]', ' | '))
  else
    res = 'fail\tbad job line'
  end
  out:write(res, '\n'); out:flush()
end
