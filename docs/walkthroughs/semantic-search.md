# Semantic search and optional answers

Semantic capabilities are optional; ordinary file browsing and Find files
remain available without them. Your deployment must offer a supported
semantic worker, model, and runtime before this guide's optional features
appear. Browser users can only access capabilities provisioned by the server
administrator.

1. In **Settings → Semantic**, check component availability and install or
   enable a supported local component where your host permits it. Follow the
   displayed consent and download information; do not assume a model is
   installed by default.
2. Choose the folders to include. Review exclusions and consent before
   enrolment: Procyon indexes only included scope. Allow indexing to finish
   (or inspect its status in Operations / Semantic settings); results can be
   incomplete while it is running.
3. Open **Search Knowledge** and describe what you are looking for. Inspect
   the matched documents and their source locations before acting on them.
   If you do not see the search action, check component availability and
   whether the relevant folder was enrolled.

**Optional: ask a question about sources.** If you want generated answers,
configure an **LLM profile** in Settings with the model endpoint and
credentials you trust, review the data-sharing and filename-redaction
settings, and test the profile. Then use the assistant/ask action and verify
its cited source passages. An LLM profile is **not required** to index files
or find them by meaning. Never paste a private API key into a guide or clip.
