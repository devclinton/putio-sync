-- Shared by the bundled notifiers (prepended to each main.lua when putio-sync is built):
-- which events to send, the text for each, and grouping. The first event of a type is
-- sent at once and opens a window of `group_minutes`; later ones in the window are
-- counted, and one summary is sent on the first tick (or event) after it closes.

local notifier = {}

local DEFAULT_EVENTS = {
  "download.finished", "download.failed", "transfer.failed", "transfer.stuck", "sync.completed",
  "storage.low", "doctor.problem",
}
local SUMMARY = {
  ["download.finished"] = "downloads finished",
  ["download.failed"] = "downloads failed",
  ["transfer.added"] = "transfers added",
  ["transfer.finished"] = "transfers finished",
  ["transfer.failed"] = "transfers failed",
  ["transfer.stuck"] = "transfers stuck",
  ["sync.completed"] = "sync runs failed",
  ["storage.low"] = "low storage warnings",
  ["doctor.problem"] = "doctor problems",
}
local NAMES_KEPT = 5

local function size(bytes)
  if type(bytes) ~= "number" then return nil end
  local units = { "B", "KB", "MB", "GB", "TB" }
  local i = 1
  while bytes >= 1000 and i < #units do
    bytes = bytes / 1000
    i = i + 1
  end
  if i == 1 then return string.format("%d B", math.floor(bytes)) end
  return string.format("%.1f %s", bytes, units[i])
end

local function with_size(name, bytes)
  local s = size(bytes)
  if s then return name .. " (" .. s .. ")" end
  return name
end

local function with_error(name, err)
  if err and err ~= "" then return name .. ": " .. err end
  return name
end

-- Title and text for an event, or nil to send nothing
function notifier.message(event)
  local d, t = event.data or {}, event.type
  if t == "download.finished" then
    return "Downloaded", with_size(d.name, d.size_bytes)
  elseif t == "download.failed" then
    return "Download failed", with_error(d.name, d.error)
  elseif t == "transfer.added" then
    return "Transfer added", d.name
  elseif t == "transfer.finished" then
    return "Transfer finished", with_size(d.name, d.size_bytes)
  elseif t == "transfer.failed" then
    return "Transfer failed", with_error(d.name, d.error)
  elseif t == "transfer.stuck" then
    return "Transfer stuck", string.format("%s: %s at %d%%", d.name, d.reason or "stalled",
      d.percent_done or 0)
  elseif t == "sync.completed" then
    if d.result ~= "failure" then return nil end
    return "Sync failed", d.error or "The sync run failed"
  elseif t == "storage.low" then
    return "Put.io storage low", string.format("%.1f%% free (%s of %s)",
      d.free_percent or 0, size(d.free_bytes) or "?", size(d.total_bytes) or "?")
  elseif t == "doctor.problem" then
    return "Doctor: " .. (d.title or d.check or "problem"), d.summary
  elseif t == "plugin.test" then
    return "putio-sync test", d.message or "Test event"
  end
  return nil
end

-- A name for the summary, if the event has one
local function name_of(event)
  local d = event.data or {}
  return d.name or d.title
end

-- POST and raise on a transport error or a non-2xx answer
function notifier.post(opts)
  opts.method = opts.method or "POST"
  local r, err = putio.http.request(opts)
  if not r then error(err, 0) end
  if r.status < 200 or r.status >= 300 then
    local host = opts.url:match("^%a+://([^/:?#]+)") or "server"
    error(host .. ": HTTP " .. r.status, 0)
  end
  return r
end

-- A plugin table that calls `send(title, text, event_type)` per message
function notifier.new(send)
  local cfg = putio.config
  local wanted = {}
  for _, t in ipairs(cfg.notify or DEFAULT_EVENTS) do wanted[t] = true end
  local window = (cfg.group_minutes or 5) * 60
  local groups = {}

  local function flush(t, g)
    groups[t] = nil
    if g.count == 0 then return end
    local text = table.concat(g.names, ", ")
    local rest = g.count - #g.names
    if rest > 0 then
      text = (text ~= "" and (text .. " and ") or "") .. rest .. " more"
    end
    send(g.count .. " more " .. (SUMMARY[t] or t), text ~= "" and text or nil, t)
  end

  local plugin = {}

  function plugin.on_event(event)
    local t = event.type
    if t == "plugin.test" then
      send(notifier.message(event))
      return
    end
    if not wanted[t] then return end
    local title, text = notifier.message(event)
    if not title then return end
    local now = putio.time.now()
    local g = groups[t]
    if g and now - g.opened >= window then
      flush(t, g)
      g = nil
    end
    if g then
      g.count = g.count + 1
      local name = name_of(event)
      if name and #g.names < NAMES_KEPT then table.insert(g.names, name) end
      return
    end
    if window > 0 then
      groups[t] = { opened = now, count = 0, names = {} }
    end
    send(title, text, t)
  end

  function plugin.on_tick()
    local now = putio.time.now()
    for t, g in pairs(groups) do
      if now - g.opened >= window then flush(t, g) end
    end
  end

  return plugin
end

