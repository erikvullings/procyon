-- Copy Markdown Path (spec §20 sample plugin 1): builds
-- `[name](file-uri)` from the current single selection and asks the host to
-- copy it to the clipboard. Selection metadata and the clipboard write are
-- both permission-gated host calls; this script performs no filesystem I/O
-- itself.

local MARKDOWN_ESCAPE = { ["\\"] = true, ["["] = true, ["]"] = true }

-- Escapes characters that would otherwise break `[text]` link syntax.
-- Operates byte-by-byte, which is safe for UTF-8 input: continuation bytes
-- are always >= 0x80 and never collide with these ASCII escape targets.
local function markdown_escape_text(text)
  local escaped = {}
  for i = 1, #text do
    local char = text:sub(i, i)
    if MARKDOWN_ESCAPE[char] then
      escaped[#escaped + 1] = "\\" .. char
    else
      escaped[#escaped + 1] = char
    end
  end
  return table.concat(escaped)
end

local UNRESERVED = {}
for char in ("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~/:"):gmatch(".") do
  UNRESERVED[char] = true
end

-- Percent-encodes a raw file URI byte-by-byte, so a space, a parenthesis or
-- a multi-byte UTF-8 character can never be mistaken for Markdown link
-- syntax or break the URI.
local function percent_encode_uri(uri)
  local encoded = {}
  for i = 1, #uri do
    local char = uri:sub(i, i)
    if UNRESERVED[char] then
      encoded[#encoded + 1] = char
    else
      encoded[#encoded + 1] = string.format("%%%02X", uri:byte(i))
    end
  end
  return table.concat(encoded)
end

return {
  actions = function()
    return {
      {
        id = "sample.copyMarkdownPath",
        title = "Copy Markdown Path",
        description = "Copies the selected file or directory as a Markdown link.",
        requires_single_selection = true,
      },
    }
  end,

  invoke = function(action_id)
    local entries = host.selected_entry_metadata()
    local entry = entries[1]
    local link = "[" .. markdown_escape_text(entry.name) .. "](" .. percent_encode_uri(entry.uri) .. ")"
    host.clipboard_write(link)
  end,
}
