-- Skip videos below a size; everything else keeps the built-in decision
local cfg = putio.config
local videos = {}
for _, ext in ipairs(cfg.extensions or {}) do
  videos[string.lower(ext)] = true
end
local min_bytes = (cfg.min_mb or 50) * 1024 * 1024

return {
  filter = function(file)
    if file.decision ~= "download" and file.decision ~= "stream" then
      return nil
    end
    if videos[file.extension] and file.size < min_bytes then
      return "skip"
    end
    return nil
  end,
}
