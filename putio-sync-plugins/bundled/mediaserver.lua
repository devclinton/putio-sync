-- Shared by the bundled media server plugins (prepended to each main.lua when putio-sync
-- is built): which folder to rescan for a finished download, grouping per folder, the
-- library a folder belongs to, and health items.
--
-- A finished file's folder is mapped with the path rules and waits in `pending` until
-- no file has landed in it for `delay_seconds`, or it has waited `max_wait_seconds`, or
-- a sync run completes. Then each waiting folder inside a chosen library is sent to the
-- server; a failed refresh is tried once more after another delay, then dropped. The
-- waiting folders are kept in the store, so a restart does not lose them.
--
-- A server adapter has:
--   adapter.libraries()          -> { {key, title, locations = {...}}, ... } or raises
--   adapter.refresh(groups)      -> raises; groups = { {library, folders = {...}}, ... }
--   adapter.reachable()          -> true or nil, reason
--   adapter.authorized()         -> "ok" | "none" (no token saved) | nil, reason

local media = {}

local LIBRARY_CACHE_SECONDS = 600
local MAX_PENDING = 500
local MAX_TRIES = 2

-- Percent-encode everything but unreserved characters
function media.encode(s)
  return (tostring(s):gsub("[^%w%-%._~]", function(c)
    return string.format("%%%02X", c:byte())
  end))
end

-- Raise on a transport error or a non-2xx answer, naming only the host
function media.request(opts)
  local r, err = putio.http.request(opts)
  if not r then error(err, 0) end
  if r.status < 200 or r.status >= 300 then
    local host = opts.url:match("^%a+://([^/:?#]+)") or "server"
    error(host .. ": HTTP " .. r.status, 0)
  end
  return r
end

-- The decoded JSON body of a 2xx answer
function media.json(opts)
  local r = media.request(opts)
  local value, err = putio.json.decode(r.body)
  if value == nil then
    local host = opts.url:match("^%a+://([^/:?#]+)") or "server"
    error(host .. ": answer is not JSON (" .. tostring(err) .. ")", 0)
  end
  return value
end

-- `server` without trailing slashes, or nil when unset
function media.server()
  local s = putio.config.server
  if type(s) ~= "string" or s == "" then return nil end
  return (s:gsub("/+$", ""))
end

local function count(name, n)
  if n == nil or n > 0 then putio.metrics.count(name, n) end
end

local function clamp(v, lo, hi, default)
  if type(v) ~= "number" then return default end
  if v < lo then return lo end
  if v > hi then return hi end
  return v
end

-- A plugin table for `adapter`; `actions` are the plugin's own on_action handlers by id
function media.new(adapter, actions)
  local cfg = putio.config
  local delay = clamp(cfg.delay_seconds, 0, 600, 20)
  local max_wait = clamp(cfg.max_wait_seconds, 10, 3600, 300)
  local rules = cfg.path_map or {}
  local chosen = {}
  local any_chosen = false
  for _, key in ipairs(cfg.libraries or {}) do
    chosen[tostring(key)] = true
    any_chosen = true
  end

  local pending = {}
  local cache = { at = nil, list = nil }
  local store = putio.store

  local function save()
    if store then store.set("pending", pending) end
  end

  local function libraries(fresh)
    local now = putio.time.now()
    if fresh or not cache.list or now - cache.at > LIBRARY_CACHE_SECONDS then
      cache.list = adapter.libraries()
      cache.at = now
    end
    return cache.list
  end

  -- The chosen library holding `folder`, by longest location
  local function library_of(folder, list)
    local best, best_len
    for _, lib in ipairs(list) do
      if not any_chosen or chosen[tostring(lib.key)] then
        for _, loc in ipairs(lib.locations or {}) do
          if putio.path.within(folder, loc) and (not best_len or #loc > best_len) then
            best, best_len = lib, #loc
          end
        end
      end
    end
    return best
  end

  local function due(p, now, all)
    return all or now - p.last >= delay or now - p.first >= max_wait
  end

  -- Send every due folder; `all` sends every waiting one
  local function flush(all)
    local now = putio.time.now()
    local ready = {}
    for folder, p in pairs(pending) do
      if due(p, now, all) then ready[#ready + 1] = folder end
    end
    if #ready == 0 then return end
    table.sort(ready)

    local ok, list = pcall(libraries, false)
    if not ok then
      putio.log.warn("Could not list the libraries", { error = tostring(list) })
      list = nil
    end
    local groups, by_key = {}, {}
    if list then
      for _, folder in ipairs(ready) do
        local lib = library_of(folder, list)
        if not lib then
          -- Libraries may have changed; look once more before skipping
          local ok2, fresh = pcall(libraries, true)
          if ok2 then list = fresh; lib = library_of(folder, list) end
        end
        if lib then
          local g = by_key[lib.key]
          if not g then
            g = { library = lib, folders = {} }
            by_key[lib.key] = g
            groups[#groups + 1] = g
          end
          g.folders[#g.folders + 1] = folder
        else
          pending[folder] = nil
          count("skipped")
          putio.log.info("Folder is in no chosen library; not refreshed", { folder = folder })
        end
      end
    end

    local function failed(folders, err)
      for _, folder in ipairs(folders) do
        local p = pending[folder]
        if p then
          p.tries = (p.tries or 0) + 1
          if p.tries >= MAX_TRIES then
            pending[folder] = nil
            count("refresh_failures")
            putio.log.warn("Refresh failed twice; dropped", { folder = folder, error = err })
          else
            p.first, p.last = now, now
          end
        end
      end
    end

    if not list then
      failed(ready, "libraries unavailable")
    else
      for _, g in ipairs(groups) do
        local ok3, err = pcall(adapter.refresh, { g })
        if ok3 then
          for _, folder in ipairs(g.folders) do pending[folder] = nil end
          count("refreshes", #g.folders)
          putio.log.info("Refresh sent", {
            library = g.library.title, folders = #g.folders,
          })
        else
          failed(g.folders, tostring(err))
        end
      end
    end
    save()
  end

  local function add(event)
    local d = event.data or {}
    if type(d.local_path) ~= "string" or d.local_path == "" then return end
    local folder = putio.path.map(putio.path.parent(d.local_path), rules)
    local now = putio.time.now()
    local p = pending[folder]
    if p then
      p.last = now
      p.count = p.count + 1
      count("folders_grouped")
    else
      local n = 0
      for _ in pairs(pending) do n = n + 1 end
      if n >= MAX_PENDING then
        count("skipped")
        putio.log.warn("Too many folders waiting; this one is not refreshed", { folder = folder })
        return
      end
      pending[folder] = { first = now, last = now, count = 1, tries = 0 }
    end
    save()
  end

  -- What `health()` returns; `test` logs each item too
  local function health()
    local items = {}
    local function item(status, message)
      items[#items + 1] = { status = status, message = message }
    end
    if not media.server() then
      item("fail", "No server set")
      return items
    end
    local up, why = adapter.reachable()
    if up then
      item("ok", "Server reachable")
    else
      item("fail", "Server not reachable: " .. tostring(why))
      return items
    end
    local auth, reason = adapter.authorized()
    if auth == "ok" then
      item("ok", "Token accepted")
    elseif auth == "none" then
      item("warn", "No token saved")
      return items
    else
      item("fail", "Token refused: " .. tostring(reason))
      return items
    end
    local dir = putio.download_dir
    if dir == nil or dir == "" then return items end
    local ok, list = pcall(libraries, true)
    if not ok then
      item("fail", "Could not list the libraries: " .. tostring(list))
      return items
    end
    local mapped = putio.path.map(dir, rules)
    local names = {}
    for _, lib in ipairs(list) do
      if not any_chosen or chosen[tostring(lib.key)] then
        for _, loc in ipairs(lib.locations or {}) do
          if putio.path.within(loc, mapped) or putio.path.within(mapped, loc) then
            names[#names + 1] = lib.title
            break
          end
        end
      end
    end
    if #names > 0 then
      item("ok", "Downloads (" .. mapped .. " on the server) reach " .. table.concat(names, ", "))
    else
      item("warn", "Downloads map to " .. mapped ..
        " on the server, which is in no chosen library; add a path rule or pick a library")
    end
    return items
  end

  local plugin = {}

  function plugin.init()
    if not store then return end
    local saved = store.get("pending")
    if type(saved) == "table" then
      for folder, p in pairs(saved) do
        if type(p) == "table" and type(p.first) == "number" then pending[folder] = p end
      end
    end
  end

  function plugin.on_event(event)
    local t = event.type
    if t == "download.finished" then
      add(event)
    elseif t == "sync.completed" then
      flush(true)
    elseif t == "plugin.test" then
      local bad = {}
      for _, it in ipairs(health()) do
        local log = it.status == "ok" and putio.log.info or putio.log.warn
        log(it.message)
        if it.status == "fail" then bad[#bad + 1] = it.message end
      end
      if #bad > 0 then error(table.concat(bad, "; "), 0) end
    end
  end

  function plugin.on_tick()
    flush(false)
  end

  plugin.health = health

  function plugin.on_action(id, input)
    local handler = actions[id]
    if not handler then error("unknown action " .. tostring(id), 0) end
    return handler(input or {})
  end

  -- For the plugins' own library loaders
  plugin.libraries = function() return libraries(true) end
  return plugin
end
