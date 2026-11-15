# 0325 Image generation pane

Status: done (two images contracts: the OpenAI-style API and Alibaba Cloud Model Studio's async task
API behind `qwen-image`/`wan*`. Local adapters such as ComfyUI/Automatic1111 remain presets to add.
The browser host's pane has been walked through in the mock build and its layout measured; the Tauri
host has not been seen, no image has been generated against a live endpoint, and the Model Studio path
has not been run against the real service.)
Priority: low
Subsystem: backend, frontend
Depends on: 0322

## Context
Image generation is a separate capability from chat: a different endpoint type (OpenAI-style images
API, or a local server such as ComfyUI or Automatic1111), different inputs, and an output the person
must review before anything is written. It gets its own role in 0322 and its own pane.

## Acceptance Criteria
- An "Image" role in Settings > LLM (AI) (0322), selecting an image-capable endpoint and model.
- A generation pane with two parts, side by side in a pane pair or stacked in one pane:
  - **Prompt**: prompt text, optional negative prompt, size, count, seed. A "Generate" button.
  - **Preview**: the generated images, each with "Save", "Discard" and "Regenerate with this prompt".
- Nothing is written to disk until "Save". Save asks for a folder and file name (name validated, no
  overwrite: conflict flow from 0045), and the saved file carries the prompt in its metadata where the
  format supports it.
- Generation is cancellable. Output size and count are bounded; very large images are downscaled for
  preview only.
- Cloud endpoints follow 0322's consent rules.
- Browser and Tauri hosts behave the same; provider calls stay in the backend.
- Tests: request bounds, save/no-overwrite, cancel, metadata embedding (where supported).

## Implementation Notes
- Backend: a generation capability next to `LlmProfileService`, with its own request and reply DTOs.
  Do not overload the chat role.
- Image bytes are returned through a short-lived backend store keyed by id, not inlined as JSON.
- Start with one endpoint type (OpenAI-style images API) and add local adapters as presets.

## Agent Notes
- 2026-11-12 claude: Created from design discussion. Not started.
- Implemented:
  - `crates/fm-application/src/image_generation.rs` validates the request (prompt and negative-prompt
    length, edges 64..=2048 in multiples of 8, at most 2 097 152 pixels, 1..=4 images, seed up to
    2^32-1), holds finished batches for 30 minutes and at most 128 MiB, and serves preview or original
    bytes by id. Previews are downscaled to a 1024 px edge for display only; a JPEG preview stays a
    JPEG and an unsupported output format is refused.
  - `crates/fm-application/src/image_generation_transport.rs` speaks two contracts, picked by what the
    endpoint's profile advertises: the OpenAI-style images API (`b64_json` replies) and Alibaba Cloud
    Model Studio's asynchronous task API, where a submit creates a task (`X-DashScope-Async: enable`)
    that is polled at `{base}/api/v1/tasks/{task_id}` until it hands back image URLs. Both read bounded
    responses, honour cancellation, and never carry the key in their `Debug` text. The task reply's URLs
    are only fetched when they stay on the endpoint's own service (same scheme, same host or a sibling
    of its registered domain, which is how Model Studio serves results from object storage) and say
    `image/*`; the prompt is submitted with `prompt_extend: false` and `watermark: false`, because the
    person already reviewed the wording and the saved file is theirs. `LlmPreset::ModelStudio` offers it
    as a preset on both hosts.
  - `LlmRole::Image` and `LlmRoles.image` in `llm_profiles.rs`: the image role resolves its own
    endpoint, model, auth header, timeout (>= 120 s) and images URL, and a cloud endpoint is refused
    unless cloud use is allowed for that role (`llmCloudImageNotAllowed`), as embeddings already are.
  - Save (`ImageGenerationService::save`) is the only path that writes: it validates the name, requires
    the stored extension, requires provider `WRITE`, and passes `overwrite` through, so an existing
    file maps to `DestinationAlreadyExists` and the 0045 conflict flow. Prompt metadata is embedded at
    save time - PNG `tEXt`/`iTXt` chunks after `IHDR`; JPEG and WebP through `fm_metadata`'s edit path -
    and a format that cannot carry it reports `prompt_embedded: false`.
  - Hosts: `POST /api/v1/images/generate|cancel|save|discard` and `GET /api/v1/images/bytes` in
    `fm-server` (bytes are served as a response body, never as JSON), and matching Tauri commands. The
    save route enforces the accessible roots.
  - Frontend: `frontend/src/features/image-generation/` is laid out like the chat pane rather than as two
    documents side by side: a header row (previous generations as crumbs, new prompt, new tab,
    favourites, and a gear that opens Settings at LLM (AI)), the preview above, the prompt composer
    below at full width, and a status row holding the style multi-select, size, lens, aperture and seed.
    There is no image-count choice: one image per generation, matching how the pane is used.
    `frontend/src/app/image-wiring.ts` drives it; the mock host mirrors bounds, consent, cancellation and
    the no-overwrite rule so both hosts behave the same.
  - Settings > LLM (AI) now exposes the Image role in the advanced view, with the cloud permission and
    consent handled as they already are for embeddings.
  - A profile no longer has to advertise the chat completions API to be saved - an images-only endpoint
    is legitimate - but binding one to the chat role is refused where the person can still act on it,
    both in `set_role` and again in `resolve_role` for bindings saved before the rule existed.
  - Tests cover request bounds, preview downscaling, save and no-overwrite, metadata embedding for PNG
    and JPEG, cancellation, transport request shape and redaction for both contracts (including the
    poll-to-ready path, a failed task, and the refusal to fetch a URL off the endpoint's own host), the
    role gate, the contract the capabilities select, and the mock host.
  - The style picker folds ten presets behind a count (`Style · 2`) because the status row has no room to
    name them: eight `medium` presets are mutually exclusive, `black and white` is a `treatment` that
    rides along with one, and the checkbox rows show labels alone - never the icon name beside them.
    `togglePreset`/`presetsAreCompatible` in `image-presets.ts` own that rule.
  - Generations are kept in `entry.history` (at most `MAX_GENERATION_CRUMBS`) so the crumb bar can show
    earlier ones; `selectGeneration` returns to one, `newPrompt` clears the composer, and `discard`/
    `forget` drop them from the store as well.
  - `LlmPreset::QwenImage` ("Qwen Image (Alibaba Cloud)", `qwen-image` filled in) lets the image role be
    bound from the preset list without typing a model id; `wan2.1` and the other Model Studio image
    models are reached by editing the same model field.
  - The pane is opened from the pane overflow menu next to Chat (`photoIcon`), the way the chat pane is.
  - 2026-11-13, browser host (mock build): the header icons, empty preview, composer, status row and the
    style popup were checked in Chrome at 1728x998. Two fixes came out of it - the status-row select and
    seed field were being stretched to the full pane width by Materialize's `select`/`input` rules (they
    now carry `browser-default` on the field only, with the row's own rules scoped under `.fm-image-status`
    to outweigh the shell's `select.browser-default { width: 100% }`), and no reachable command existed to
    open the pane at all.
  - 2026-11-13, second pass from the person's own look at the pane: the three status choices became
    drawn controls (a chrome-density trigger with a caret plus a `role=listbox` list that hangs above
    the row, since the row is the bottom of the pane). A native `select` is not usable here - Materialize
    hides every one it was not told about, and the field it substitutes is a form control several times
    the row's height. Lens and aperture gained readable labels (`85mm portrait`, `f/1.8 wide open`)
    because the ids are slugs.
  - The composer now says nothing under an empty prompt: the "Nothing yet - describe what you want to
    see." line and the red "Describe what you want to see first." were both removed, since a disabled
    send key already says a subject is missing and failures reach the preview's alert. The wording that
    will be sent still appears once there are words, because the presets add words the person did not
    type. The leave-out field is named on its left and its placeholder is marked as an example, so
    "text, watermark" is not read as a feature of its own. The send key is the Enter glyph, as in chat.
  - 2026-11-14, after the person's own look at the same pane:
  - The image role is no longer hidden behind "use different endpoints or models per feature", and its
    model is typed rather than picked from discovery: discovery answers with chat models, and most chat
    models cannot draw. The row says so - "Most models cannot draw. Name the model that can; the chat
    model of an endpoint usually cannot." It is named "Image generation" everywhere a person meets it
    (the roles list, the command palette action, the pane's overflow item) so it is not read as a chat
    model that can see pictures, and its cloud permission says what leaves the device - the words, and
    the pictures that come back - instead of borrowing the sentence about indexing.
  - Any provider the app knows can serve the image role, `mlx-serve` and OpenAI-compatible endpoints
    included: the contract comes from what the profile advertises (`ImageGenerations` or
    `DashscopeTasks`), not from whether the base URL happens to look like a chat endpoint.
  - The status row carries five single-choice controls - Style, Finish, Size, Quality, Steps - each a
    drawn trigger with a `role=listbox` list. Checkboxes invited combinations that cannot be drawn, since
    the mediums exclude one another; `black and white` is a finish that rides along with a medium.
  - Quality is said in the person's words - fast, good, quality, super quality - and each contract maps
    them to its own vocabulary where it has one. Steps stay optional: an endpoint with no sampler step
    count is sent none rather than a guessed number.
  - A picture can start the drawing. It is taken from the cursor of the other pane rather than typed as a
    path, and it travels as a location - the backend reads the bytes through the VFS provider and checks
    them (PNG, JPEG or WebP, at most 12 MiB). Endpoints that cannot take a starting picture say which
    problem it was - cannot take one, not a usable picture, too large - rather than sharing one refusal.
  - The seed field's bottom border had been cut off by the row's own height; the header and status rows
    now allow 2px more, measured in Chrome rather than guessed.
  - A refused save now blames the destination ("That folder or file name does not fit this location.")
    because at that point the folder and the name are the only things in the person's hands - the prompt
    has already been accepted. This came from driving the save form in the browser with an edited folder.
  - 2026-11-14, browser host (mock build) walked in Chrome: the pane opens from the overflow item, the
    gear lands on Settings at LLM (AI), the image row binds to a saved endpoint, Generate returns a
    picture and the preview shows it from a blob, and the save form arrives holding the other pane's
    folder and a name built from the prompt. The earlier "the role never stuck" was the probe never
    committing a choice in the Materialize dropdown, not the app refusing the binding.
  - Still unverified: the "Saved to ..." receipt in the browser host (the mock file tree holds no picture
    to start from, and the save was reached while a concurrent session's in-flight frontend edits had the
    shell on "Loading..."), the Tauri host, and a live endpoint of either contract. The write itself, the
    no-overwrite rule and the metadata are covered by wiring and mock-client tests.
  - The tool is offered only where it can work. The pane's command and its overflow entry wait for an
    image model to be assigned (`imageGenerationAvailable`, read at startup, re-read when Settings
    closes), because a pane that can only say "no image model is set" is not worth a keystroke.
  - The picture model's provider is chosen beside it. An endpoint that answers chat is rarely one that
    can draw, so the image row names a provider from every server the app knows - Ollama, LM Studio,
    vLLM, SGLang, OMLX, mlx-serve, Unsloth Studio, OpenAI-compatible, Azure, Model Studio, Qwen Image.
    A server already added is bound; one not added yet fills the endpoint editor above and says which
    provider is missing, because a role may not be bound to an endpoint that does not exist. The
    decision itself is `providerChoice`, tested apart from the widget: the endpoint's saved model wins
    over the preset's, and a typed model wins over both.
  - 2026-11-14, browser host again: choosing `LM Studio` with no endpoint for it filled the editor and
    named the missing provider; after saving that endpoint, choosing it again bound the role, `Save
    role` enabled and the pane opened from the overflow item; with the role cleared the item read
    disabled and clicking it opened nothing.

## 2026-11-14: a slow local model was being blamed on the prompt

- Tested against a live `mlx-serve` (`ddalcu/Qwen-Image-2.1-MLX-Serve-8bit`). It answers the
  OpenAI-shaped body Procyon sends at `POST /v1/images/generations`, and accepts `negative_prompt`,
  `quality`, `seed` and `steps` beside the required fields: one 1024x1024 picture came back after
  **261 s** at the model's own default step count, and after **12 s** at `steps: 2`.
- The person was told "The prompt, size or seed is outside what the model accepts." for a request the
  model was still drawing correctly. Two faults stacked up:
  - Every rejection reaches the client under one `invalidRequest` code with the reason the person
    should see in `details.reason`. The pane read the top-level `reason` - which is never there -
    and fell back to the code, so a timeout, a cloud rule and a misconfigured endpoint all read as a
    bad prompt. It reads `details` first now.
  - `entry.failure` held the backend's reason while its two readers (`isSaveConflict`,
    `failureNeedsSettings`) compared message keys, so neither the "Open AI settings" hint nor the
    "Replace the existing file" offer had ever appeared. `failure` holds an `ImageFailureKey` now,
    which is what both readers were always comparing against.
- A timeout is its own outcome: `ImageGenerationError::TimedOut`, reported as the existing
  `llmTimeout` reason and worded as running out of time with a way out (fewer steps, or the
  endpoint's timeout in Settings) instead of "could not be reached", which sends the person to check
  wiring that is fine.
- The shortest timeout a generation may run went from 120 s to **600 s**, measured against the run
  above, and the Steps row gained 2, 4 and 8 next to 20, 30 and 50: on this machine a quick draft was
  12 s and a full one 260 s, and a chat-length timeout was refusing work the model was doing right.
- The mock's prompt list ordered by a millisecond clock, so two prompts saved in the same
  millisecond came back oldest-first and the Prompts list order depended on machine speed. Saving
  later now wins a tie.
- Known browser-host limit found on the way: `LlmHostPolicy::server` sets `allow_loopback: false`, so
  an `fm-server` host refuses to reach a model on its own localhost and says `permissionDenied`. The
  desktop host allows loopback and is unaffected; a browser-host person with a local model needs the
  host allowed by the administrator, and the pane now has a sentence for that (`hostNotAllowed`).
- Verified: 41 `image_generation` Rust tests (including an endpoint that accepts the request and
  never answers), 53 `llm_profiles` tests, 247 frontend tests across the pane, the wiring, the mock
  client and i18n; clippy and rustfmt clean for `fm-application`. Still unverified in this host: the
  Tauri window and a live cloud endpoint of either contract.

### The pane beside the chat pane (UI pass, 2026-11-15)

- The two breadcrumb bars were measured rather than eyeballed: at 620 px both `.fm-image-header` and
  `.fm-chat-header` compute to `--fm-row-height` (20 px) with no child crossing the row. Their trailing
  actions are the same three, in the same order - a new prompt or chat, **AI settings**, a new tab - and
  the favourites heart is gone from both, where it had never done anything. The chat's settings icon now
  opens Settings at the LLM (AI) section, where its role, endpoint and consent live; it used to land on
  the semantic section, which is not what "Open AI settings" promises. `chat-tab.test.ts` pins all of it.
- The prompt box ends its leave-out row with the send and stop keys, the stop drawn filled (an open
  square reads as a checkbox), and flips to the wording the presets, lens and aperture added instead of
  the repeated "Sent to the model" note.
- Materialize's `input[type="text"]` outranks a single class, so the negative-prompt field arrived at its
  40 px default and towered over the keys beside it; it carries `browser-default` like the pane's other
  fields. The status bar no longer leaves the pane when narrowed: the size and seed fields are only as
  wide as four digits, and below about 340 px the row wraps onto a second line instead of pushing
  Discard out of sight (0 px of spill measured at 380 px and 320 px).
- Saving: the extension is part of the field but not of what can be edited, folders read `~/Downloads`
  rather than `file:///Users/…`, the folders worth saving to are one-click chips, a suggestion is counted
  past what the folder already holds and made again when the folder changes, and the stem comes from the
  wording typed for that generation - each adaptation reads as itself, and a repeat is numbered - because
  the endpoint echoes a composed prompt that is cut mid-word at 40 characters.
- The read-only extension follows the format the endpoint returned, agreed with the person: Procyon
  writes the bytes it was given, there is no re-encode step, and the save path refuses a destination
  whose extension disagrees with those bytes. Forcing `.png` would mean decoding and re-encoding, a
  quality cost nobody asked for. Pinned: a JPEG batch offers `.jpg` and the form never says `.png`.
- The bar and the tab header read "Image"; the tool entry in the pane's overflow menu and the command
  palette keep the full "Image generation" (`image.title` versus `image.actionTitle`).
- The picture model is picked from what the endpoint reports, in the same pulldown the chat model uses,
  with "Type another name…" for a model nobody listed and a plain field when the endpoint reports
  nothing to pick from. Nothing is filtered out: `mlx-serve` marks its drawing model
  `capabilities: ["image"]`, but model discovery returns names only, and hiding a model the person
  needs is worse than listing ones that cannot draw. A capability-aware filter needs the discovery
  DTO to carry the fields it currently drops.
- Where a picture lands is stated rather than asked. The save form shows one line - `Folder
  ~/Downloads   Change folder` - and the folder is resolved in order: the other pane's folder, a
  folder that pane has open while it shows a tool, a folder of the pane's own, where the last picture
  went, and finally the OS pictures folder. Somewhere else stays one click away, chips and all.
- The browser host's loopback rule now has an operator opt-in: `PROCYON_LLM_ALLOW_LOOPBACK`
  (1, true, yes, on) opens the loopback branch of `LlmHostPolicy` for `fm-server`, announced at startup
  with an `llmLoopbackAllowed` warning notification. Private LAN addresses stay under
  `PROCYON_LLM_ALLOWED_HOSTS` either way. README, "AI endpoints, and which hosts the server may reach".
- Verified: 1640 Rust tests across `fm-application`, `fm-events` and `fm-server` (55 `llm_profiles`),
  3793 frontend tests; `tsc --noEmit`, Biome, rustfmt and clippy clean. Both states of the save form -
  the stated folder and the opened choice - were looked at in Chrome at pane width, and the figures come
  from the real components. Still unverified in this host: the Tauri window and a live cloud endpoint of
  either contract.
- The opt-in was proven against the live `mlx-serve` rather than a fixture. With the default posture
  the server answered `403 permissionDenied` before anything left the process; with
  `PROCYON_LLM_ALLOW_LOOPBACK=1` the same request drew a 512x512 picture in 3 s at `steps: 2` - on the
  person's own `mlx-serve` profile, which advertises only `chatCompletions` and `modelDiscovery`, so
  an unadorned profile does get the OpenAI-style contract. Saving that picture wrote 468 700 bytes as
  468 889 with `promptEmbedded: true`; saving it again was refused with `destinationAlreadyExists`,
  and the third attempt, with `overwrite: true`, went through. The file on disk has a valid PNG
  signature and carries both the prompt and the parameters text chunks.

### The wait has a shape now (progress, centring, saving, 2026-11-15)

- Waiting is no longer a bare sentence. `mlx-serve` reports each step over SSE
  (`data: {"type":"progress","stage":"Generating","step":1,"total":2}` then
  `{"type":"complete","data":[{"b64_json":…}]}`), and the transport now asks for that: the
  OpenAI-style request carries `stream: true`, the reply is read as an event stream, and every
  progress event is stored against the `request_id` while the generation is in flight. A server
  that rejects the `stream` parameter is asked again without it, once; one that rejects the ask
  itself is not asked twice. `GET /api/v1/images/progress?request_id=` (and the
  `get_image_generation_progress` command) hand the same numbers to either host, answering `204`
  when the endpoint has said nothing - which is how most of them answer, and the pane then keeps its
  own waiting sentence rather than inventing a percentage.
- Procyon repeats the endpoint's own words, nothing else. `stage` is kept as the provider wrote it,
  bounded to 80 characters and stripped of control characters, and is rendered as text; a step
  counter only appears when `total` is above zero. The pane polls while the drawing runs, and clears
  what it saw when the generation answers, so a finished batch is never still shown as working.
  Proven against the live `mlx-serve`, not a fixture: nine reports for an eight-step 512x512 draw -
  `Encoding prompt 0/8`, `Generating 1/8` … `7/8`, `Decoding image 8/8` - in 10.6 s.
- A fitted picture now sits in the middle of its pane. `object-position` was `left top`, so a
  preview smaller than the pane hugged the top-left corner; `contain` with `center` centres it, and
  the full-width mode is untouched.
- Saving writes the file the person named. The save request sent the *folder* URI as the destination,
  so changing the folder, or letting the returned extension decide the suffix, produced a destination
  with no file name in it and the save was refused. The destination is built from the folder and the
  name together (`destinationFor`), which is what the backend's own name and extension checks were
  already expecting. Pinned by a test that reads the destination out of the request.
- Every folder the person has open is offered when a picture is saved, not only the two panes either
  side: the other pane's folder first, then this pane's, then every folder still open in any other
  pane, then where recent saves went - deduplicated, `search://` left out. A save that goes to a third
  pane's folder is now one click rather than a typed path.
- Tab is a first-class key in a tool pane. A tool pane is a form, so Tab walks its own controls and
  the browser moves focus; the pane hands the keystroke back to the shell at its first and last stop,
  where it switches panes as it always did. The viewer's find box is deliberately left out of the
  walk, because leaving it with Tab is what the box is for. `tab-traversal.ts` holds the two helpers
  (`tabStops`, `tabStaysWithin`) and the layout handler asks it per keystroke. Note for anyone
  extending these tests: jsdom ignores `preventDefault()` on an event created without
  `cancelable: true`, so a synthetic keydown must ask for it.
- Verified: 46 `image_generation` Rust tests (including a server that reports steps and still returns
  images, one that refuses `stream` and is retried without it, and one that refuses the ask itself),
  3809 frontend tests (three for the bar, one for the mock's own counting, three for Tab in a tool
  pane, one for the destination, one for the folders on offer); `tsc --noEmit`, Biome, rustfmt,
  clippy for `fm-application`/`fm-transport-dto`/`fm-server`, and `cargo check -p fm-desktop` clean.
  Still unverified in this host: the Tauri window and a live cloud endpoint of either contract - a
  cloud server that streams progress has never been talked to here.

### "The seed had no impact" (2026-11-15)

- Measured, not assumed. Straight to `mlx-serve`: `seed: 7` → `ef8dcef9…` twice, `seed: 8` →
  `7f38298c…`, no seed → `ac6229ae…`. Through Procyon with the pane's own ask (composed prompt,
  negative prompt, `stream: true`, quality, 1024×768): `seed: 7` → `0f94d52c…` three times,
  `seed: 8` → `d0f100da…`, no seed → `69690beb…`. A seed is honoured, and the same seed draws the
  same picture. `generator_seed`, which diffusers users reach for, is ignored by this server - the
  OpenAI name `seed` is the one that works, and it is the one sent.
- Why it looked like nothing happened: at two steps the model has drawn no structure yet. The two
  seed-7 and seed-8 pictures at `steps: 2` are both a featureless gradient - different bytes,
  indistinguishable to a person. At eight steps the same two seeds give two different foxes. A cheap
  step count, which is what the pane now offers for quick checks, is therefore the one setting where
  a seed cannot show, and nothing in the pane said so.
- So the seed is now named where it belongs. Under the picture: `seed 7`, or `seed chosen by the
  model`. What the endpoint echoed back wins, since it knows what it used; `mlx-serve` echoes
  nothing, so the ask remembered with the generation is shown, and going back through the crumb bar
  brings that seed back into the field beside the prompt. The remembered seed is kept on the
  generation the first time it is archived: archiving again behind a crumb must not overwrite it with
  a field that has moved on to the next ask.
- The steps list ends with one line of its own, which cannot be picked: "A couple of steps only
  checks the endpoint answers. A picture needs eight or more."
- Both status rows are now the row height they were promised. They measured 22px against the
  breadcrumb's 20px: a `min-height` of row-height-plus-a-hairline, plus `padding-block`, made the row
  taller than the header above it. The hairline that separates them from the rows around them is now
  an inset shadow, which costs no height, and their padding is gone - crumb bar, header, status row
  and "More" row all measure 20px, with a 20px seed field sitting flush inside the "More" row and
  `scrollHeight` equal to `clientHeight`, so no border is clipped either.
- Verified: 3820 frontend tests (the seed readout, the note, `seedUsedFor`'s three cases, the seed
  returning with its generation, and the seed reaching the request both typed and reused);
  `tsc --noEmit` and Biome clean.
