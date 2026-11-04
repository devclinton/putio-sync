-- Telegram: sendMessage per notification (https://core.telegram.org/bots/api#sendmessage)
local cfg = putio.config
local api = (cfg.api_url or "https://api.telegram.org"):gsub("/+$", "")

return notifier.new(function(title, text)
  notifier.post({
    url = api .. "/bot" .. cfg.bot_token .. "/sendMessage",
    headers = { ["Content-Type"] = "application/json" },
    body = putio.json.encode({
      chat_id = cfg.chat_id,
      text = text and (title .. "\n" .. text) or title,
    }),
  })
end)
