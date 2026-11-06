-- Plex: partial scans of one folder per request
-- (GET <server>/library/sections/<key>/refresh?path=<folder>), sign-in with a plex.tv PIN.
local cfg = putio.config
local PRODUCT = "putio-sync"
local plex_tv = (cfg.plex_tv_url or "https://plex.tv"):gsub("/+$", "")
-- Where the code is entered (048: the UI copies the code and opens this page)
local LINK = "https://plex.tv/link"
local PIN_SECONDS = 600

-- One id per install, kept in the store; plex.tv ties the PIN and the token to it
local function client_id()
  local id = putio.store.get("client_id")
  if type(id) ~= "string" or id == "" then
    local parts = {}
    for i = 1, 4 do parts[i] = string.format("%08x", math.random(0, 0x7fffffff)) end
    id = "putio-sync-" .. table.concat(parts)
    putio.store.set("client_id", id)
  end
  return id
end

local function headers(token)
  local h = {
    ["Accept"] = "application/json",
    ["X-Plex-Product"] = PRODUCT,
    ["X-Plex-Client-Identifier"] = client_id(),
  }
  if token and token ~= "" then h["X-Plex-Token"] = token end
  return h
end

local function token()
  local t = cfg.token
  if type(t) == "string" and t ~= "" then return t end
  return nil
end

local function server_or_raise()
  local s = media.server()
  if not s then error("Set the server first", 0) end
  return s
end

local adapter = {}

function adapter.libraries()
  local body = media.json({
    url = server_or_raise() .. "/library/sections",
    headers = headers(token()),
  })
  local list = {}
  local dirs = body.MediaContainer and body.MediaContainer.Directory or {}
  for _, d in ipairs(dirs) do
    local locations = {}
    for _, l in ipairs(d.Location or {}) do locations[#locations + 1] = l.path end
    list[#list + 1] = { key = tostring(d.key), title = d.title or tostring(d.key), locations = locations }
  end
  return list
end

function adapter.refresh(groups)
  local server = server_or_raise()
  for _, g in ipairs(groups) do
    for _, folder in ipairs(g.folders) do
      media.request({
        url = server .. "/library/sections/" .. media.encode(g.library.key) ..
          "/refresh?path=" .. media.encode(folder),
        headers = headers(token()),
      })
    end
  end
end

function adapter.reachable()
  local r, err = putio.http.request({ url = server_or_raise() .. "/identity", headers = headers(nil) })
  if not r then return nil, err end
  if r.status >= 200 and r.status < 300 then return true end
  return nil, "HTTP " .. r.status
end

function adapter.authorized()
  if not token() then return "none" end
  local r, err = putio.http.request({
    url = server_or_raise() .. "/library/sections",
    headers = headers(token()),
  })
  if not r then return nil, err end
  if r.status >= 200 and r.status < 300 then return "ok" end
  return nil, "HTTP " .. r.status
end

local actions = {}

function actions.sign_in()
  local pin = media.json({
    method = "POST",
    url = plex_tv .. "/api/v2/pins",
    headers = headers(nil),
  })
  if not pin.id or not pin.code then error("plex.tv gave no PIN", 0) end
  return {
    message = "Enter this code at plex.tv/link",
    code = pin.code,
    open_url = LINK,
    next = { action = "sign_in_poll", input = { id = pin.id, started = putio.time.now() }, after_seconds = 2 },
  }
end

function actions.sign_in_poll(input)
  if not input.id then error("no PIN to check", 0) end
  local pin = media.json({
    url = plex_tv .. "/api/v2/pins/" .. media.encode(input.id),
    headers = headers(nil),
  })
  local t = pin.authToken
  if type(t) ~= "string" or t == "" then
    if putio.time.now() - (input.started or 0) > PIN_SECONDS then
      return { done = true, message = "The code expired; press Sign in with Plex again" }
    end
    return {
      message = "Waiting for you to sign in at plex.tv",
      code = pin.code,
      open_url = LINK,
      next = { action = "sign_in_poll", input = input, after_seconds = 2 },
    }
  end
  local who = "your Plex account"
  local ok, user = pcall(media.json, { url = plex_tv .. "/api/v2/user", headers = headers(t) })
  if ok and type(user) == "table" and user.username then who = user.username end
  return { done = true, message = "Signed in as " .. who, save = { token = t } }
end

function actions.load_servers()
  local t = token()
  if not t then error("Sign in with Plex first", 0) end
  local resources = media.json({
    url = plex_tv .. "/api/v2/resources?includeHttps=1",
    headers = headers(t),
  })
  local choices = {}
  for _, r in ipairs(resources) do
    if type(r.provides) == "string" and r.provides:find("server", 1, true) then
      for _, c in ipairs(r.connections or {}) do
        if c.uri and not c.relay then
          local where = c["local"] and "local" or "remote"
          choices[#choices + 1] = { value = c.uri, label = (r.name or "Plex") .. " (" .. where .. ") " .. c.uri }
        end
      end
    end
  end
  local message = #choices .. " server addresses found"
  if #choices == 0 then message = "No servers found on this account" end
  return { choices = choices, message = message }
end

local plugin

function actions.load_libraries()
  local list = plugin.libraries()
  local choices = {}
  for _, lib in ipairs(list) do
    local label = lib.title
    if #lib.locations > 0 then label = label .. " (" .. table.concat(lib.locations, ", ") .. ")" end
    choices[#choices + 1] = { value = lib.key, label = label }
  end
  return { choices = choices, message = #choices .. " libraries found" }
end

plugin = media.new(adapter, actions)
return plugin
