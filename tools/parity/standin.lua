-- The parity tier's stand-in for LÖVE (docs/decisions.md D17): enough of LÖVE for the game's own
-- Lua to run headless. Any LÖVE call returns a value that does nothing, except reading files: the
-- game's code from the owner's Balatro.exe (PARITY_EXE), read in place with `unzip -p` (nothing
-- extracted or written), and the owner's settings and profile from PARITY_PROFILE, plain-text
-- copies the Rust test writes (the game takes a file starting with `return` as uncompressed,
-- engine/string_packer.lua `get_compressed`). Every write is dropped. None of the game's code is
-- in this repo.
local EXE = assert(os.getenv('PARITY_EXE'), 'set PARITY_EXE')
local PROFILE = os.getenv('PARITY_PROFILE')

local function quote(s) return "'" .. s:gsub("'", "'\\''") .. "'" end

local zip_cache = {}
local function zip_read(path)
  if zip_cache[path] == nil then
    local p = io.popen('unzip -p ' .. quote(EXE) .. ' ' .. quote(path) .. ' 2>/dev/null')
    local s = p:read('*a'); p:close()
    zip_cache[path] = (s ~= '' and s) or false
  end
  return zip_cache[path] or nil
end

local function profile_read(path)
  if not PROFILE or path:find('%.lua$') then return nil end
  local f = io.open(PROFILE .. '/' .. path, 'rb'); if not f then return nil end
  local s = f:read('*a'); f:close(); return s
end

table.insert(package.loaders, 2, function(name)
  local src = zip_read(name:gsub('%.', '/') .. '.lua') or zip_read(name .. '.lua')
  if not src then return nil end
  return assert(loadstring(src, '@' .. name .. '.lua'))
end)

-- what any LÖVE call gives back: indexing or calling it gives itself, arithmetic a number
local S = setmetatable({}, {})
local SM = getmetatable(S)
SM.__index = function() return S end
SM.__call = function() return S end
for _, m in ipairs({'__add', '__sub', '__mul', '__div', '__mod', '__pow'}) do
  SM[m] = function(a, b) return (type(a) == 'number' and a) or (type(b) == 'number' and b) or 1 end
end
SM.__unm = function() return -1 end
SM.__concat = function(a, b) return (type(a) == 'string' and a or '') .. (type(b) == 'string' and b or '') end
SM.__tostring = function() return 'stub' end
SM.__len = function() return 0 end

local function stub(t) return setmetatable(t or {}, {__index = function() return S end}) end
local function num(n) return function() return n end end
local function sized(w, h) return stub({getDimensions = function() return w, h end, getWidth = num(w), getHeight = num(h)}) end

love = stub({
  graphics = stub({
    getWidth = num(1920), getHeight = num(1080), getDimensions = function() return 1920, 1080 end,
    newImage = function() return sized(64, 64) end,
    newFont = function() return sized(10, 10) end,
    getFont = function() return sized(10, 10) end,
  }),
  system = stub({getOS = function() return 'Linux' end}),
  timer = stub({getTime = os.clock, getDelta = num(1 / 60), getFPS = num(60)}),
  window = stub({getMode = function() return 1920, 1080, {} end, getDesktopDimensions = function() return 1920, 1080 end, getDisplayCount = num(1)}),
  filesystem = stub({
    getInfo = function(p) if profile_read(p) or zip_read(p) then return {type = 'file'} end end,
    read = function(p) return profile_read(p) or zip_read(p) end,
    getDirectoryItems = function() return {} end,
    getSaveDirectory = function() return '/nonexistent' end,
    getSourceBaseDirectory = function() return '/nonexistent' end,
    write = function() return true end,
    createDirectory = function() return true end,
    remove = function() return true end,
  }),
  -- every file the game reads is plain text, so nothing is decompressed
  data = stub({decompress = function() error('the parity stand-in reads plain text only') end}),
  thread = stub({}),
  audio = stub({}),
  mouse = stub({getPosition = function() return 0, 0 end}),
  joystick = stub({getJoysticks = function() return {} end}),
  math = stub({}),
})

-- LÖVE runs conf.lua, then main.lua (which requires the game's modules in its order)
assert(loadstring(assert(zip_read('conf.lua'), 'conf.lua not in PARITY_EXE'), '@conf.lua'))()
assert(loadstring(assert(zip_read('main.lua'), 'main.lua not in PARITY_EXE'), '@main.lua'))()
