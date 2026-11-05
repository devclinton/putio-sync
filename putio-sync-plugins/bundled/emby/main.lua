-- Emby: the API key goes in X-Emby-Token, under the /emby prefix
return embyfin.new({
  prefix = "/emby",
  auth = function(key) return { ["X-Emby-Token"] = key } end,
})
