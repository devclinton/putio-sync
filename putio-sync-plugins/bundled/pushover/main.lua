-- Pushover: one form POST per message (https://pushover.net/api)
local cfg = putio.config
local api = (cfg.api_url or "https://api.pushover.net"):gsub("/+$", "")

local function encode(s)
  return (tostring(s):gsub("[^%w%-_%.~]", function(c)
    return string.format("%%%02X", string.byte(c))
  end))
end

return notifier.new(function(title, text)
  local fields = {
    { "token", cfg.app_token },
    { "user", cfg.user_key },
    { "title", title },
    { "message", text or title },
    { "priority", cfg.priority or 0 },
  }
  local parts = {}
  for _, f in ipairs(fields) do
    parts[#parts + 1] = encode(f[1]) .. "=" .. encode(f[2])
  end
  notifier.post({
    url = api .. "/1/messages.json",
    headers = { ["Content-Type"] = "application/x-www-form-urlencoded" },
    body = table.concat(parts, "&"),
  })
end)
