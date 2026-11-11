-- Keep the title and the episode (SxxEyy) or year; drop what follows
local cfg = putio.config
local renamed = {}
for _, ext in ipairs(cfg.extensions or {}) do
  renamed[string.lower(ext)] = true
end

local function trim(text)
  return (text:gsub("^[%s%-]+", ""):gsub("[%s%-]+$", ""))
end

local function clean(name, ext)
  local base = name:sub(1, #name - #ext - 1)
  local spaced = base:gsub("[._]", " ")
  local show, episode = spaced:match("^(.-)%s*([Ss]%d%d?[Ee]%d%d%d?)")
  if show and trim(show) ~= "" then
    return trim(show) .. " " .. episode:upper() .. "." .. ext
  end
  local title, year = spaced:match("^(.-)%s+[%(%[]?([12][09]%d%d)[%)%]]?%f[%W]")
  if title and trim(title) ~= "" then
    return trim(title) .. " (" .. year .. ")." .. ext
  end
  return nil
end

return {
  rename = function(file)
    if file.extension == "" or not renamed[file.extension] then
      return nil
    end
    local ext = file.name:match("%.([^.]+)$")
    local name = clean(file.name, ext)
    if name == file.name then
      return nil
    end
    return name
  end,
}
