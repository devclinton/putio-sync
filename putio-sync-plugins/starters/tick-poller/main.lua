-- Check a URL every few minutes, and after each sync, and log when its status changes.
-- Run on_tick from the workbench; a dry run answers with the reply you set (200 by default).
local plugin = {}

local function check()
  local r, err = putio.http.request({ url = putio.config.url, timeout = 10 })
  local status = r and r.status or 0
  local last = putio.store.get("status")
  if status ~= last then
    if r then
      putio.log.info("status " .. tostring(last) .. " -> " .. status)
    else
      putio.log.warn("unreachable: " .. tostring(err))
    end
    putio.store.set("status", status)
  end
end

plugin.on_tick = check
plugin.on_event = check

return plugin
