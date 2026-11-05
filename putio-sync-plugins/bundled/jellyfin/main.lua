-- Jellyfin: the API key goes in the MediaBrowser Authorization header
return embyfin.new({
  prefix = "",
  auth = function(key)
    return {
      ["Authorization"] = 'MediaBrowser Client="putio-sync", Device="putio-sync", ' ..
        'DeviceId="putio-sync", Version="1", Token="' .. key .. '"',
    }
  end,
})
