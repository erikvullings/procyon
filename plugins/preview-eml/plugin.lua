-- F3 preview for RFC 5322 / MIME messages (.eml). The message body is always shown as text -
-- HTML parts are reduced to text, never rendered - so a mail cannot load remote content.
local MAX_PARTS = 50
-- Escaping can grow text 5x, so the shown text and headers stay far below the 512 KiB output cap.
local MAX_TEXT_CHARS = 64 * 1024
local MAX_HEADER_CHARS = 1024
local MAX_DEPTH = 6

local ESCAPES = { ["&"] = "&amp;", ["<"] = "&lt;", [">"] = "&gt;", ['"'] = "&quot;", ["'"] = "&#39;" }

local function to_utf8(s)
  if utf8.len(s) then return s end
  return (s:gsub("[\128-\255]", function(c) return utf8.char(c:byte()) end))
end
local function esc(s) return (to_utf8(tostring(s)):gsub("[&<>\"']", ESCAPES)) end

local function clip(s, limit)
  if #s <= limit then return s end
  local cut = s:sub(1, limit)
  while #cut > 0 and not utf8.len(cut) do cut = cut:sub(1, -2) end
  return cut .. "…"
end

local B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/"
local B64_INDEX = {}
for i = 1, #B64 do B64_INDEX[B64:byte(i)] = i - 1 end

local function base64(s)
  local out, acc, bits = {}, 0, 0
  for i = 1, #s do
    local v = B64_INDEX[s:byte(i)]
    if v then
      acc = (acc << 6) | v
      bits = bits + 6
      if bits >= 8 then
        bits = bits - 8
        out[#out + 1] = string.char((acc >> bits) & 0xFF)
        acc = acc & ((1 << bits) - 1)
      end
    end
  end
  return table.concat(out)
end

local function quoted_printable(s, header)
  if header then s = s:gsub("_", " ") else s = s:gsub("=\r?\n", "") end
  return (s:gsub("=(%x%x)", function(h) return string.char(tonumber(h, 16)) end))
end

-- Charsets other than UTF-8 are approximated as Latin-1; invalid UTF-8 falls back to it too.
local function charset_to_utf8(s, charset)
  charset = (charset or "utf-8"):lower()
  if charset == "utf-8" or charset == "utf8" or charset == "us-ascii" then return to_utf8(s) end
  return (s:gsub("[\128-\255]", function(c) return utf8.char(c:byte()) end))
end

-- RFC 2047: whitespace between adjacent encoded words is dropped, and the bytes of consecutive
-- words are joined before charset conversion so a multibyte character split across two words
-- still decodes.
local function decode_words(value)
  local out, pos = {}, 1
  local pending, pending_charset = nil, nil
  local function flush()
    if pending then out[#out + 1] = charset_to_utf8(pending, pending_charset) end
    pending, pending_charset = nil, nil
  end
  while pos <= #value do
    local s, e, charset, enc, text = value:find("=%?([^?]+)%?([bBqQ])%?([^?]*)%?=", pos)
    if not s then break end
    local between = value:sub(pos, s - 1)
    if pending and not between:match("^%s*$") then flush() end
    if not pending then out[#out + 1] = to_utf8(between) end
    charset = charset:match("^[^*]+"):lower()
    local raw = enc:lower() == "b" and base64(text) or quoted_printable(text, true)
    if pending and pending_charset ~= charset then flush() end
    pending, pending_charset = (pending or "") .. raw, charset
    pos = e + 1
  end
  flush()
  out[#out + 1] = to_utf8(value:sub(pos))
  return table.concat(out)
end

local function split_message(raw)
  local head, body = raw:match("^(.-)\r?\n\r?\n(.*)$")
  if not head then head, body = raw, "" end
  local headers, order = {}, {}
  head = head:gsub("\r?\n[ \t]+", " ")
  for line in head:gmatch("[^\r\n]+") do
    local name, value = line:match("^([%w%-]+):%s*(.*)$")
    if name then
      name = name:lower()
      if headers[name] == nil then
        headers[name] = value
        order[#order + 1] = name
      end
    end
  end
  return headers, body
end

local function param(header, name)
  if not header then return nil end
  local quoted = header:match(name .. '%s*=%s*"([^"]*)"')
  return quoted or header:match(name .. "%s*=%s*([^%s;]+)")
end

local function decode_body(body, headers)
  local enc = (headers["content-transfer-encoding"] or ""):lower()
  if enc == "base64" then body = base64(body)
  elseif enc == "quoted-printable" then body = quoted_printable(body, false) end
  return charset_to_utf8(body, param(headers["content-type"], "charset"))
end

local function html_to_text(html)
  html = html:gsub("<[sS][cC][rR][iI][pP][tT].-</[sS][cC][rR][iI][pP][tT]>", "")
  html = html:gsub("<[sS][tT][yY][lL][eE].-</[sS][tT][yY][lL][eE]>", "")
  html = html:gsub("<[bB][rR]%s*/?>", "\n"):gsub("</[pP]>", "\n\n"):gsub("<[^>]*>", "")
  local entities = { amp = "&", lt = "<", gt = ">", quot = '"', nbsp = " ", apos = "'" }
  return (html:gsub("&(%w+);", function(e) return entities[e] or ("&" .. e .. ";") end))
end

-- Walks the MIME tree, collecting the first plain-text body, the first HTML body and attachments.
local function walk(headers, body, state, depth)
  state.parts = state.parts + 1
  if state.parts > MAX_PARTS or depth > MAX_DEPTH then return end
  local ctype = (headers["content-type"] or "text/plain"):lower()
  local mime = ctype:match("^%s*([^;%s]+)") or "text/plain"
  local boundary = param(headers["content-type"], "boundary")
  if mime:match("^multipart/") and boundary then
    local marker = "--" .. boundary
    local start = body:find(marker, 1, true)
    while start do
      local line_end = body:find("\n", start, true)
      if not line_end or body:sub(start + #marker, start + #marker + 1) == "--" then break end
      local next_start = body:find("\n" .. marker, line_end, true)
      local part = body:sub(line_end + 1, next_start and next_start - 1 or #body)
      local part_headers, part_body = split_message(part)
      walk(part_headers, part_body, state, depth + 1)
      start = next_start and next_start + 1 or nil
    end
    return
  end
  local disposition = (headers["content-disposition"] or ""):lower()
  local filename = param(headers["content-disposition"], "filename") or param(headers["content-type"], "name")
  if filename or disposition:match("^attachment") then
    state.attachments[#state.attachments + 1] = { name = decode_words(filename or "(unnamed)"), type = mime }
  elseif mime == "text/plain" and not state.text then
    state.text = decode_body(body, headers)
  elseif mime == "text/html" and not state.html then
    state.html = decode_body(body, headers)
  end
end

local function preview(file)
  local headers, body = split_message(file.content)
  if not headers.from and not headers.subject and not headers.date then
    return { kind = "text", body = "Not a readable email message." }
  end
  local state = { parts = 0, attachments = {} }
  walk(headers, body, state, 1)
  local out = { "<h2>", esc(clip(decode_words(headers.subject or "(no subject)"), MAX_HEADER_CHARS)), "</h2><table>" }
  for _, field in ipairs({ "from", "to", "cc", "bcc", "reply-to", "date" }) do
    if headers[field] then
      out[#out + 1] = "<tr><th>" .. esc(field:gsub("^%l", string.upper)) .. "</th><td>" .. esc(clip(decode_words(headers[field]), MAX_HEADER_CHARS)) .. "</td></tr>"
    end
  end
  out[#out + 1] = "</table>"
  if #state.attachments > 0 then
    out[#out + 1] = "<h3>Attachments</h3><ul>"
    for _, a in ipairs(state.attachments) do
      out[#out + 1] = "<li>" .. esc(clip(a.name, 200)) .. " <em>(" .. esc(clip(a.type, 100)) .. ")</em></li>"
    end
    out[#out + 1] = "</ul>"
  end
  local text = state.text or (state.html and html_to_text(state.html)) or ""
  out[#out + 1] = "<hr><pre>" .. esc(clip(text, MAX_TEXT_CHARS)) .. "</pre>"
  return { kind = "html", body = table.concat(out) }
end

return { preview = preview }
