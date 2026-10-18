-- F3 preview for vCard (.vcf) and iCalendar (.ics) files.
local MAX_ITEMS = 100
-- Escaping can grow text 5x; item and value caps keep the output far below the 512 KiB budget.
local MAX_VALUE_CHARS = 2000
local MAX_LINES_PER_ITEM = 60

local ESCAPES = { ["&"] = "&amp;", ["<"] = "&lt;", [">"] = "&gt;", ['"'] = "&quot;", ["'"] = "&#39;" }
local function to_utf8(s)
  if utf8.len(s) then return s end
  return (s:gsub("[\128-\255]", function(c) return utf8.char(c:byte()) end))
end
local function esc(s) return (to_utf8(tostring(s)):gsub("[&<>\"']", ESCAPES)) end

-- Splits unfolded content lines into { name, params, value }.
local function parse_lines(text)
  text = text:gsub("\r\n", "\n"):gsub("\r", "\n"):gsub("\n[ \t]", "")
  local lines = {}
  for line in text:gmatch("[^\n]+") do
    local left, value = line:match("^([^:]+):(.*)$")
    if left then
      local name, params = left:match("^([^;]+);?(.*)$")
      lines[#lines + 1] = { name = name:upper(), params = params:upper(), value = value }
    end
  end
  return lines
end

local function unescape(value)
  return (value:gsub("\\([nN,;\\])", function(c) return c:lower() == "n" and "\n" or c end))
end

-- 20260315T093000Z -> 2026-03-15 09:30 UTC; date-only values stay dates.
local function pretty_date(value)
  local y, mo, d, h, mi, z = value:match("^(%d%d%d%d)(%d%d)(%d%d)T(%d%d)(%d%d)%d*(Z?)$")
  if y then return string.format("%s-%s-%s %s:%s%s", y, mo, d, h, mi, z == "Z" and " UTC" or "") end
  y, mo, d = value:match("^(%d%d%d%d)(%d%d)(%d%d)$")
  if y then return string.format("%s-%s-%s", y, mo, d) end
  return value
end

local function field(label, value)
  if value == nil or value == "" then return "" end
  if #value > MAX_VALUE_CHARS then
    value = value:sub(1, MAX_VALUE_CHARS)
    while #value > 0 and not utf8.len(value) do value = value:sub(1, -2) end
    value = value .. "…"
  end
  return "<tr><th>" .. esc(label) .. "</th><td>" .. esc(value):gsub("\n", "<br>") .. "</td></tr>"
end

local function type_suffix(params)
  local types = params:match("TYPE=([^;]+)")
  return types and (" (" .. types:lower():gsub(",", ", ") .. ")") or ""
end

-- Collects the property lines of each top-level `kind` component. Nested components (VALARM
-- inside VEVENT, ...) are skipped so their properties never override the parent's.
local function blocks(lines, kind)
  local result, current, nested = {}, nil, 0
  for _, l in ipairs(lines) do
    if current and nested > 0 then
      if l.name == "BEGIN" then nested = nested + 1
      elseif l.name == "END" then nested = nested - 1 end
    elseif l.name == "BEGIN" and not current and l.value:upper() == kind then current = {}
    elseif l.name == "BEGIN" and current then nested = 1
    elseif l.name == "END" and current and l.value:upper() == kind then
      result[#result + 1] = current
      current = nil
    elseif current and #current < MAX_LINES_PER_ITEM then current[#current + 1] = l end
  end
  return result
end

local function render_vcard(card)
  local fn, rows = nil, {}
  for _, l in ipairs(card) do
    local v = unescape(l.value)
    if l.name == "FN" then fn = v
    elseif l.name == "ORG" then rows[#rows + 1] = field("Organization", (v:gsub(";", ", ")))
    elseif l.name == "TITLE" then rows[#rows + 1] = field("Title", v)
    elseif l.name == "TEL" then rows[#rows + 1] = field("Phone" .. type_suffix(l.params), v)
    elseif l.name == "EMAIL" then rows[#rows + 1] = field("Email" .. type_suffix(l.params), v)
    elseif l.name == "ADR" then rows[#rows + 1] = field("Address" .. type_suffix(l.params), (l.value:gsub(";+", ", "):gsub("^, ", ""):gsub(", $", "")))
    elseif l.name == "URL" then rows[#rows + 1] = field("Website", v)
    elseif l.name == "BDAY" then rows[#rows + 1] = field("Birthday", pretty_date(v))
    elseif l.name == "NOTE" then rows[#rows + 1] = field("Note", v) end
  end
  return "<h3>" .. esc(fn or "(unnamed contact)") .. "</h3><table>" .. table.concat(rows) .. "</table>"
end

local function render_event(item, kind)
  local title, rows = nil, {}
  for _, l in ipairs(item) do
    local v = unescape(l.value)
    if l.name == "SUMMARY" then title = v
    elseif l.name == "DTSTART" then rows[#rows + 1] = field("Start", pretty_date(v))
    elseif l.name == "DTEND" or l.name == "DUE" then rows[#rows + 1] = field(l.name == "DUE" and "Due" or "End", pretty_date(v))
    elseif l.name == "LOCATION" then rows[#rows + 1] = field("Location", v)
    elseif l.name == "ORGANIZER" then rows[#rows + 1] = field("Organizer", (v:gsub("^[mM][aA][iI][lL][tT][oO]:", "")))
    elseif l.name == "ATTENDEE" then rows[#rows + 1] = field("Attendee", (v:gsub("^[mM][aA][iI][lL][tT][oO]:", "")))
    elseif l.name == "STATUS" then rows[#rows + 1] = field("Status", v)
    elseif l.name == "RRULE" then rows[#rows + 1] = field("Repeats", l.value)
    elseif l.name == "DESCRIPTION" then rows[#rows + 1] = field("Description", v) end
  end
  return "<h3>" .. esc(title or ("(untitled " .. kind .. ")")) .. "</h3><table>" .. table.concat(rows) .. "</table>"
end

local function preview(file)
  local lines = parse_lines(file.content)
  local out, count = {}, 0
  local function add(items, render, kind)
    for _, item in ipairs(items) do
      count = count + 1
      if count <= MAX_ITEMS then out[#out + 1] = render(item, kind) end
    end
  end
  if file.extension == "vcf" then
    add(blocks(lines, "VCARD"), render_vcard)
  else
    add(blocks(lines, "VEVENT"), render_event, "event")
    add(blocks(lines, "VTODO"), render_event, "task")
  end
  if count == 0 then
    return { kind = "text", body = "No contacts or calendar items found." }
  end
  if count > MAX_ITEMS then out[#out + 1] = "<p>… and " .. esc(count - MAX_ITEMS) .. " more.</p>" end
  return { kind = "html", body = table.concat(out) }
end

return { preview = preview }
