-- F3 preview for BitTorrent metainfo (.torrent). Pure function of the file prefix it is handed:
-- no host services. Output is HTML that the host sanitizes again before display.
local MAX_DEPTH = 32
local MAX_FILES = 500

local function to_utf8(s)
  if utf8.len(s) then return s end
  return (s:gsub("[\128-\255]", function(c) return utf8.char(c:byte()) end))
end

local ESCAPES = { ["&"] = "&amp;", ["<"] = "&lt;", [">"] = "&gt;", ['"'] = "&quot;", ["'"] = "&#39;" }
local function esc(s) return (to_utf8(tostring(s)):gsub("[&<>\"']", ESCAPES)) end

-- Bencoded numbers are attacker-controlled: only non-negative whole numbers are trusted.
local function whole(v)
  return math.type(v) == "integer" and v >= 0 and v or nil
end

local function human_size(n)
  local units = { "B", "KiB", "MiB", "GiB", "TiB", "PiB" }
  local i = 1
  while n >= 1024 and i < #units do
    n = n / 1024
    i = i + 1
  end
  if i == 1 then return string.format("%d B", n) end
  return string.format("%.1f %s", n, units[i])
end

local function utc_date(ts)
  local days, secs = ts // 86400, ts % 86400
  local z = days + 719468
  local era = (z >= 0 and z or z - 146096) // 146097
  local doe = z - era * 146097
  local yoe = (doe - doe // 1460 + doe // 36524 - doe // 146096) // 365
  local y = yoe + era * 400
  local doy = doe - (365 * yoe + yoe // 4 - yoe // 100)
  local mp = (5 * doy + 2) // 153
  local d = doy - (153 * mp + 2) // 5 + 1
  local m = mp < 10 and mp + 3 or mp - 9
  if m <= 2 then y = y + 1 end
  return string.format("%04d-%02d-%02d %02d:%02d UTC", y, m, d, secs // 3600, secs % 3600 // 60)
end

-- Tolerant bencode decoder. Tables are linked into their parent as soon as they are created, so
-- when the file prefix ends mid-structure (large `pieces` string) everything parsed so far
-- survives. Returns the root and an error message (nil when the whole document parsed).
local function decode(s)
  local root, pos = nil, 1
  local function fail() error("truncated or malformed bencode", 0) end
  local function integer()
    local stop = s:find("e", pos, true)
    local text = stop and s:sub(pos + 1, stop - 1)
    local n = text and text:match("^-?%d+$") and tonumber(text)
    if not n then fail() end
    pos = stop + 1
    return n
  end
  local function bytes()
    local colon = s:find(":", pos, true)
    local len = colon and tonumber(s:sub(pos, colon - 1))
    if not len or len < 0 or colon + len > #s then fail() end
    local v = s:sub(colon + 1, colon + len)
    pos = colon + len + 1
    return v
  end
  local value
  function value(depth, attach)
    if depth > MAX_DEPTH then fail() end
    local c = s:sub(pos, pos)
    if c == "i" then return integer() end
    if c == "l" or c == "d" then
      local t = {}
      if attach then attach(t) end
      if not root then root = t end
      pos = pos + 1
      while s:sub(pos, pos) ~= "e" do
        if pos > #s then fail() end
        if c == "l" then
          local index = #t + 1
          t[index] = value(depth + 1, function(child) t[index] = child end)
        else
          local key = bytes()
          t[key] = value(depth + 1, function(child) t[key] = child end)
        end
      end
      pos = pos + 1
      return t
    end
    if c:match("%d") then return bytes() end
    fail()
  end
  local ok, err = pcall(value, 1, nil)
  return root, (not ok) and err or nil
end

local function row(label, html) return "<tr><th>" .. esc(label) .. "</th><td>" .. html .. "</td></tr>" end

local function preview(file)
  local meta, err = decode(file.content)
  if type(meta) ~= "table" or type(meta.info) ~= "table" then
    return { kind = "text", body = "Not a readable torrent file." .. (err and (" (" .. err .. ")") or "") }
  end
  local info = meta.info
  local out = { "<h2>", esc(type(info.name) == "string" and info.name or file.name), "</h2><table>" }
  local files, total = {}, whole(info.length)
  if type(info.files) == "table" then
    total = 0
    for _, f in ipairs(info.files) do
      if type(f) == "table" and type(f.path) == "table" then
        local parts = {}
        for _, part in ipairs(f.path) do
          if type(part) == "string" then parts[#parts + 1] = part end
        end
        local length = math.min(whole(f.length) or 0, 1 << 62)
        total = math.min(total + length, 1 << 62)
        files[#files + 1] = { path = table.concat(parts, "/"), length = length }
      end
    end
  end
  if total then out[#out + 1] = row("Size", esc(human_size(total))) end
  if whole(info["piece length"]) then
    out[#out + 1] = row("Piece length", esc(human_size(info["piece length"])))
  end
  if #files > 0 then out[#out + 1] = row("Files", esc(#files)) end
  local created = whole(meta["creation date"])
  if created and created < 253402300800 then out[#out + 1] = row("Created", esc(utc_date(created))) end
  if type(meta["created by"]) == "string" then out[#out + 1] = row("Created by", esc(meta["created by"])) end
  if type(meta.comment) == "string" then out[#out + 1] = row("Comment", esc(meta.comment)) end
  if info.private == 1 then out[#out + 1] = row("Private", "yes") end
  local trackers = {}
  if type(meta.announce) == "string" then trackers[#trackers + 1] = meta.announce end
  if type(meta["announce-list"]) == "table" then
    for _, tier in ipairs(meta["announce-list"]) do
      for _, url in ipairs(type(tier) == "table" and tier or {}) do
        if type(url) == "string" then trackers[#trackers + 1] = url end
      end
    end
  end
  if #trackers > 0 then
    local items = {}
    for i = 1, math.min(#trackers, 20) do items[#items + 1] = "<li>" .. esc(trackers[i]) .. "</li>" end
    out[#out + 1] = row("Trackers", "<ul>" .. table.concat(items) .. "</ul>")
  end
  out[#out + 1] = "</table>"
  if #files > 0 then
    out[#out + 1] = "<h3>Files</h3><table><tr><th>Path</th><th>Size</th></tr>"
    for i = 1, math.min(#files, MAX_FILES) do
      out[#out + 1] = "<tr><td>" .. esc(files[i].path) .. "</td><td>" .. esc(human_size(files[i].length)) .. "</td></tr>"
    end
    out[#out + 1] = "</table>"
    if #files > MAX_FILES then
      out[#out + 1] = "<p>… and " .. esc(#files - MAX_FILES) .. " more files.</p>"
    end
  end
  if err and not file.truncated then out[#out + 1] = "<p>Metadata is incomplete: " .. esc(err) .. ".</p>" end
  return { kind = "html", body = table.concat(out) }
end

return { preview = preview }
