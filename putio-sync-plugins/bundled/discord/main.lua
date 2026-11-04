-- Discord: one webhook message per notification
local cfg = putio.config

return notifier.new(function(title, text)
  local content = "**" .. title .. "**"
  if text then content = content .. "\n" .. text end
  notifier.post({
    url = cfg.webhook_url,
    headers = { ["Content-Type"] = "application/json" },
    body = putio.json.encode({ username = cfg.username or "putio-sync", content = content }),
  })
end)
