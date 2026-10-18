# 0267 Tag and EXIF fields in multi-rename

Status: done
Priority: medium
Subsystem: backend, frontend
Depends on: 0072, 0149

## Context
Multi-rename (0072) works on names, dates and counters. Photo and music collections need tokens from metadata, e.g. `{artist} - {title}` or `{exif:date-taken:yyyy-MM-dd}`.

## Acceptance Criteria
- Rename rules accept metadata tokens (audio tags: artist, album, title, track, year; image EXIF: date taken, camera, dimensions); live preview shows resolved names and flags files missing the field.
- Unsafe characters are sanitized per platform; collisions are detected before apply, as today.
- Saved presets (0149) store the tokens.

## Implementation Notes
- Backend resolves tokens (the frontend only supplies the pattern) so the same result is produced in browser and Tauri hosts.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: Backend resolves tokens (so browser and Tauri agree): fm-metadata::photo (kamadak-exif 0.6: DateTimeOriginal, Make/Model, dimensions; header fallback via image) + audio tags (lofty; local files by path, other providers read into memory up to 16 MiB), fm-application/metadata_tokens.rs (parse/resolve/sanitise/date format; tests), POST /api/v1/files/metadata-tokens (<=500 files, <=32 tokens, roots checked) + Tauri command + client.resolveMetadataTokens. Tokens: {artist} {albumartist} {album} {title} {track}(2-digit) {disc} {year} {genre} {exif:date-taken[:yyyy-MM-dd HH mm ss format]} {exif:camera} {exif:width} {exif:height} {exif:dimensions}. Values are sanitised with the same cross-platform rules the dialog validates against (/ \\ -> '-', : * ? " < > | -> '_'), not per host. Frontend: multi-rename-rules (metadataTokensIn, missing/pending detection; plan cannot be applied while any name lacks a field or metadata is still loading), dialog debounces a backend read when masks contain {tokens}, shows 'Missing metadata: ...', unknown tokens and read errors. Presets store the masks, so tokens persist with no schema change. Camera dimensions of HEIC/TIFF without EXIF pixel tags are unavailable (image crate built without those codecs). Tag reading is not exercised on real MP3/FLAC fixtures (only WAV-with-ID3 and synthetic JPEG EXIF).
