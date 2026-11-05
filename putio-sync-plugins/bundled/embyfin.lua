-- Shared by the bundled Jellyfin and Emby plugins (after mediaserver.lua): one
-- POST <server><prefix>/Library/Media/Updated per refresh with every due folder.
-- `flavor.prefix` is "" (Jellyfin) or "/emby"; `flavor.auth(key)` gives the auth headers.

local embyfin = {}

function embyfin.new(flavor)
  local cfg = putio.config

  local function key()
    local k = cfg.api_key
    if type(k) == "string" and k ~= "" then return k end
    return nil
  end

  local function base()
    local s = media.server()
    if not s then error("Set the server first", 0) end
    return s .. flavor.prefix
  end

  local function headers(with_key)
    local h = { ["Accept"] = "application/json", ["Content-Type"] = "application/json" }
    if with_key and key() then
      for k, v in pairs(flavor.auth(key())) do h[k] = v end
    end
    return h
  end

  local adapter = {}

  function adapter.libraries()
    local folders = media.json({ url = base() .. "/Library/VirtualFolders", headers = headers(true) })
    local list = {}
    for _, f in ipairs(folders) do
      if f.Name then
        list[#list + 1] = { key = f.Name, title = f.Name, locations = f.Locations or {} }
      end
    end
    return list
  end

  function adapter.refresh(groups)
    local updates = {}
    for _, g in ipairs(groups) do
      for _, folder in ipairs(g.folders) do
        updates[#updates + 1] = { Path = folder, UpdateType = "Created" }
      end
    end
    media.request({
      method = "POST",
      url = base() .. "/Library/Media/Updated",
      headers = headers(true),
      body = putio.json.encode({ Updates = updates }),
    })
  end

  function adapter.reachable()
    local r, err = putio.http.request({ url = base() .. "/System/Info/Public", headers = headers(false) })
    if not r then return nil, err end
    if r.status >= 200 and r.status < 300 then return true end
    return nil, "HTTP " .. r.status
  end

  function adapter.authorized()
    if not key() then return "none" end
    local r, err = putio.http.request({ url = base() .. "/System/Info", headers = headers(true) })
    if not r then return nil, err end
    if r.status >= 200 and r.status < 300 then return "ok" end
    return nil, "HTTP " .. r.status
  end

  local plugin
  local actions = {}

  function actions.load_libraries()
    local choices = {}
    for _, lib in ipairs(plugin.libraries()) do
      local label = lib.title
      if #lib.locations > 0 then label = label .. " (" .. table.concat(lib.locations, ", ") .. ")" end
      choices[#choices + 1] = { value = lib.key, label = label }
    end
    return { choices = choices, message = #choices .. " libraries found" }
  end

  plugin = media.new(adapter, actions)
  return plugin
end
