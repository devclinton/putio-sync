-- Episodes (names with SxxEyy) go to <root>/<Show>/Season NN; other files stay put
local cfg = putio.config

return {
  folder = function(file)
    local spaced = file.name:gsub("[._]", " ")
    local show, season = spaced:match("^(.-)%s*[Ss](%d%d?)[Ee]%d")
    if not show then
      return nil
    end
    show = show:gsub("[%s%-]+$", "")
    if show == "" then
      return nil
    end
    return string.format("%s/%s/Season %02d", cfg.root or "TV", show, tonumber(season))
  end,
}
