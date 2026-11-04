-- ntfy: one JSON publish per message (https://docs.ntfy.sh/publish/#publish-as-json)
local cfg = putio.config
local server = (cfg.server or "https://ntfy.sh"):gsub("/+$", "")

return notifier.new(function(title, text)
  local headers = { ["Content-Type"] = "application/json" }
  if cfg.token and cfg.token ~= "" then
    headers["Authorization"] = "Bearer " .. cfg.token
  end
  notifier.post({
    url = server,
    headers = headers,
    body = putio.json.encode({
      topic = cfg.topic,
      title = title,
      message = text or title,
      priority = cfg.priority or 3,
    }),
  })
end)
