-- Values are rendered by the host from already-loaded entry metadata.  This
-- contribution deliberately declares data only: it cannot inject WebView UI.
return {
  columns = function()
    return {{ id = "sample.fileAge", title = "Age" }}
  end,
}
