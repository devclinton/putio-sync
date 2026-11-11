-- Keep a count of finished downloads per day. A dry run in the workbench works on a
-- copy of the store, so the counts shown before and after are not saved.
local plugin = {}

function plugin.on_event(event)
  local day = string.sub(putio.time.iso(), 1, 10)
  local counts = putio.store.get("per_day") or {}
  counts[day] = (counts[day] or 0) + 1
  local ok, err = putio.store.set("per_day", counts)
  if not ok then error(err) end
  putio.log.info(day .. ": " .. counts[day] .. " finished")
end

function plugin.health()
  local counts = putio.store.get("per_day") or {}
  local today = counts[string.sub(putio.time.iso(), 1, 10)] or 0
  return { { status = "ok", message = today .. " finished today" } }
end

return plugin
