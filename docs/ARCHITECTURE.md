# Architecture and responsibility boundaries

## Data flow

User action → typed Tauri command → validated Rust job → bounded streaming parser → staged SQLite generation → atomic activation → bounded query → virtualized UI.

Playback follows a separate path: channel ID + variant → Rust session authorization → loopback media resources → native Android Media3 or desktop protocol engine. Media bytes never cross JSON IPC. Catalogue M3U and XMLTV parsing never run in the React main thread. The media engine still parses its own HLS/DASH protocol manifests; that is a different responsibility.

| Component | Authority |
| --- | --- |
| React/Ionic | Navigation, rendering, transient filters and logical focus |
| Tauri commands | Narrow validated application operations and lifecycle bridging |
| Rust core | Ingestion, source policy, matching, jobs, secrets and playback-session authorization |
| SQLite | Persistent source revisions, catalogue, guide and user preferences |
| Player implementation | Manifest consumption, buffering, decoder interaction and track controls |

There is no Node publisher, second catalogue database, arbitrary SQL frontend API, cloud service or model-based reasoning component.

## Catalogue and imports

M3U is read incrementally with bounded lines and input sizes. TVG identifiers/names, groups, countries, logos, shifts, catch-up attributes and approved provider headers are retained. URL suffixes and EXTVLCOPT headers become stream metadata, not browser route parameters. Playlist URLs use verified TLS and explicit network policy.

Logical channel identity is source-scoped and based on TVG ID/name or normalized name. Stream variants are distinct from logical channels; groups/countries are relationships. A token change can replace a stream variant without replacing the logical channel or favorite. A channel rename or source deletion may change identity; this implementation does not guess cross-source equivalence.

Each refresh writes a staging generation in short batch transactions, validates that it is usable, then switches the active generation transactionally. Rows are queued while the parser reads from the network, so the SQLite write lock is never held across network reads and concurrent favorites or source edits are not blocked. Readers continue to see the previous generation while import proceeds. Cancellation, an empty import or parsing/network failure does not activate partial data. Old generations are removed after successful activation. SQLite uses WAL and parameterized queries.

Imports have bounded admission and one active import worker. Per-source coordination prevents duplicate refreshes and deletion races. Progress is throttled and queryable by job ID; events are an optimization, not the sole source of truth. Automatic due-source refresh runs only while the process is alive and unlocked. No closed-app Android scheduler is included.

## iptv-org sources

A source is either an own M3U playlist or an iptv-org selection. The kind is fixed when the source is created, and own playlists keep the exact pipeline above. iptv-org sources are handled by `iptv_org.rs` in one of two methods:

- **Official playlists.** The configuration names published playlists (countries, categories, languages, regions, subdivisions, cities, upstream sources, master indexes). Each is validated into a URL under the playlist base, downloaded through the source policy and parsed by the ordinary M3U importer into the same staging generation. An empty official playlist is tolerated while the merged result must contain at least one channel. Country playlists supply the country code the official files omit; `GB` is canonicalized to the published `UK`, subdivision codes are never rewritten.
- **Catalogue selection.** The ten JSON datasets (channels, feeds, streams, logos, categories, countries, regions, subdivisions, cities, blocklist) are streamed item by item under byte and item limits into lean records, joined locally and reduced by one deterministic policy: country mode (`origin`, `coverage-strict`, `coverage-with-regions`), the `c/`, `s/`, `ct/`, `r/` broadcast-area grammar with `r/WW` and `r/INT` as an explicit opt-in, blocklist, NSFW and closed-channel exclusions, optional `Geo-blocked` and `Not 24/7` label exclusions, category and language intersections (OR within a field, AND across fields), the single-feed fallback for streams without a feed, in-use HTTPS logo choice, quality-first ordering and per-(country, channel, feed, URL) deduplication. The result is serialized as extended M3U (`tvg-id="Channel@Feed"`, `tvg-name`, `tvg-logo`, `tvg-country`, `group-title`, `#EXTVLCOPT` hints) and parsed by the same importer as user playlists, so every limit and identity rule applies uniformly. Alternative streams of one channel feed share a name and become stream variants of one logical channel instead of duplicate cards.

The selection emits only streams the source network policy accepts, so the catalogue never lists entries playback would refuse. `countries.json`, `categories.json`, `languages.json` and `regions.json` also back the source-form pickers through a read-only command that is confined to the API host and cached for a day. XMLTV remains a separate, user-configured feed; `guides.json` is a directory of guide sites and is not followed. Each import is one coherent snapshot activated atomically, a failed refresh keeps the last good generation, and iptv-org sources refresh on their configured cadence (24 hours by default) while own playlists keep six hours.

## Guide

XMLTV uses a bounded streaming XML reader. External entities and DTDs are rejected. The initial implementation accepts UTF-8 and second-precision programme timestamps, requires an explicit stop and treats a missing offset as UTC. Store times in UTC; render in the viewer's local time. Retain the previous day and next fourteen days; guide requests cover at most six hours and forty channels.

Matching priority is explicit manual mapping, exact source-qualified TVG ID with ordered EPG feed priority, then an unambiguous normalized name. Ambiguous name matches remain unresolved. Matching and shifts are deterministic Rust/SQL operations, not repeated in the UI. The mapping command is available, but a manual mapping editor is not included in this delivery.

## Bounded UI and remote input

The channel pager stores at most six pages of eighty rows and limits concurrent fetches to two. It discards stale filter/revision results and uses finite retries. Zustand holds transient UI state and a bounded focus bookmark set, not the entire catalogue. The guide requests visible channel/time windows.

Focus tracks logical row/item identity and position. Moving beyond mounted content requests/scrolls that window before focusing it. Dialogs, text inputs, page toolbars and the player have scoped key ownership. Vertical guide movement preserves time when possible. React Router is used independently of Ionic's router adapter, so routed pages have ordinary React mount/unmount lifecycle rather than hidden retained player pages.

## Playback and transport

Rust resolves one active playback session, probes a bounded prefix to identify extensionless media, validates configured hosts and supplies opaque local resource URLs. The in-process relay rewrites HLS and DASH references, supports ranges and streams media with backpressure. Provider headers are applied at the network boundary, not leaked through frontend configuration. Session replacement, locking and source removal invalidate the previous session; closing one finished session never cancels the next one being prepared. Channel artwork is served by the same relay from a bounded private cache (see [SECURITY](SECURITY.md)); the catalogue DTO carries only the loopback address.

Android uses one native Media3 instance with a full-screen dialog and remote controls. Desktop lazily loads hls.js for HLS, dash.js for DASH and mpegts.js for raw MPEG-TS or FLV streams (the relay probe reports the container); native HLS is preferred when supported, and a progressive stream WebKit rejects is remuxed through mpegts.js once. A stream that fails before its first frame is retried on the next untried variant automatically before an error is shown. The player draws its chrome over the video: a top strip with Back and the title, and a bottom toolbar with icon controls (play, seek, live edge, volume, variant, audio, subtitles, fullscreen) plus a timeline for on-demand or DVR streams. Both hide after a few seconds of inactivity, so fullscreen shows only the picture; the first remote key reveals them without acting. There is no stacked Video.js wrapper, ffprobe process or transcoder. Recovery is bounded; codec support is determined by the actual device.

## Explicitly deferred

Conditional HTTP/ETag reuse, local-file picking, source editing, mapping-editor UI, recording, DRM, cloud sync and closed-app scheduling are not implemented. These are not hidden background services. The application retains authorized source data locally but is not a live-stream availability monitoring service.
