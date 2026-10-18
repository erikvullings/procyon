# 0269 Archive-aware playback and preview

Status: done
Priority: low
Subsystem: backend, frontend
Depends on: 0076, 0260

## Context
Media inside archives (zip, tar, …) should play and preview without extracting first.

## Acceptance Criteria
- Audio/video/image entries inside an archive play or preview through the streaming path (0260) with seeking where the archive format allows (stored entries) and a bounded temp extraction otherwise.
- Temp data is cleaned up; encrypted archives prompt for the password as today.
- Queue (0262) accepts archive entries.

## Implementation Notes
- Compressed entries cannot seek cheaply; document the fallback and size limit.

## Agent Notes
- Created 2026-10-18 from the media/playlist feature discussion. 
- 2026-10-18 claude: fm-archive now implements read_range (RANDOM_ACCESS): unencrypted STORED zip entries are read in place at data_start+offset (true seeking, no temp data); every other entry (deflated/encrypted zip, tar family, 7z, rar, gzip) is expanded once into the existing private per-session extraction cache (zip streams straight to a temp file, others via the existing in-memory reader) and ranges are then served from that file. The cache is bounded (512 MiB / 512 entries, LRU) and its temp session dir is removed on drop / abandoned-session cleanup; an entry larger than the cache fails with ArchiveResourceLimit(seekableEntryBytes) - stored zips have no such limit, compressed ones must be extracted first. This also fixes viewer/image chunked reads of archive entries, which previously re-extracted the whole entry for every 1 MiB chunk. Encrypted archives: new frontend createMediaAccess pre-flight (1-byte readFileRange) runs before audio/video/queue playback and drives the same password prompt as browsing (viewer option prepareMedia + player option prepareMedia); cancelling the prompt makes the viewer show an error / the queue skip the track. Queue and 'Play' accept archive entries (they are ordinary file entries). Tests: crates/fm-archive/tests/archive_provider.rs (stored/deflated/tar/encrypted ranges), crates/fm-application/tests/media_archive.rs (Range over archive URIs). Not verified: seeking in real WebViews, very large (multi-GB) stored entries.
