-- A pipeline step: `[[hooks]]` names this plugin and a stage, and `on_item` runs for each
-- item that reaches the stage. `item` is the JSON a command step gets on stdin.
--
-- Return nothing, true or "done" to pass, "skip" to skip the item (pre_download only), or
-- { fail = "why" } to fail the step. An error fails it too.
local plugin = {}

function plugin.on_item(item, stage)
  local min_mb = tonumber(putio.config.min_mb) or 50
  local bytes = 0
  for _, file in ipairs(item.files or {}) do
    bytes = bytes + (file.size or 0)
  end
  local mb = math.floor(bytes / (1024 * 1024))
  if mb >= min_mb then return "done" end
  local why = item.name .. " is " .. mb .. " MB, under " .. min_mb .. " MB"
  putio.log.info(why, { stage = stage })
  -- Before the download there is still time to leave the item out; later the step fails
  if stage == "pre_download" then return "skip" end
  return { fail = why }
end

return plugin
