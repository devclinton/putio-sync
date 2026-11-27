-- Group finished downloads and write one digest per sync run. What is grouped lives in
-- memory, so `on_unload` writes it before a reload, a turn-off or a shutdown drops it, and
-- `on_config_changed` picks up a new limit without a reload.
local plugin = {}

local held = {}
local max = tonumber(putio.config.max) or 20

local function flush(why)
  if #held == 0 then return end
  local digests = putio.store.get("digests") or {}
  digests[#digests + 1] = { at = putio.time.iso(), why = why, names = held }
  -- Keep the last 20
  while #digests > 20 do table.remove(digests, 1) end
  local ok, err = putio.store.set("digests", digests)
  if not ok then error(err) end
  putio.log.info("Digest of " .. #held .. " downloads written", { why = why })
  held = {}
end

function plugin.on_event(event)
  if event.type == "download.finished" then
    held[#held + 1] = event.data.name
    if #held >= max then flush("full") end
  elseif event.type == "sync.completed" then
    flush("sync")
  end
end

-- The plugin's own settings were saved; `config` is the new values
function plugin.on_config_changed(config)
  max = tonumber(config.max) or 20
  if #held >= max then flush("full") end
end

-- Last call before this state is dropped. It has a short time limit, so only finish
-- what is already here
function plugin.on_unload()
  flush("unload")
end

return plugin
