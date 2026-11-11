-- Post each event it subscribes to as JSON. Run it from the workbench with the
-- download.finished sample: a dry run shows the request without sending it.
local plugin = {}

function plugin.on_event(event)
  local body = putio.json.encode({
    type = event.type,
    name = event.data.name,
    time = event.time,
  })
  putio.log.info("posting " .. event.type)
  local r, err = putio.http.request({
    method = "POST",
    url = putio.config.url,
    headers = { ["Content-Type"] = "application/json" },
    body = body,
  })
  if not r then error(err) end
  if r.status >= 300 then error("HTTP " .. r.status) end
end

return plugin
