# Migration from the supplied archives

The original `iptv.zip` and `iptv-playlist-master.zip` were inspected separately and were not modified. This repository is the new application, not a nesting of the two old repositories.

## Preserved and reimplemented

The UI's country → channel → player navigation, dark visual direction, icon and flag assets informed the new frontend. The playlist project's M3U metadata semantics informed a Rust implementation. Channel grouping, TVG identifiers, EPG feed metadata and provider headers are treated as domain data rather than publisher-specific JSON.

The original project had no complete XMLTV ingest/matching/database subsystem. Those responsibilities are newly implemented in Rust. A read-only development fixture is separate from real native data and is never a production fallback.

## Removed from the runtime repository

Capacitor and its configuration; Netlify/PWA/service-worker deployment plumbing; the Node country checker and GitHub publisher; duplicate `playlist.js`; global dependency installation and unrestricted upgrade tasks; global in-memory URL deduplication; disabled TLS verification; shell ffprobe execution; Context/Redux-like connection infrastructure; base64 URL routes; full-catalogue React state and global document geometry scans; redundant player wrappers.

No old generated country catalogue, tokens, GitHub publication configuration, build artifacts or vendored node_modules were copied into this application. The old public publisher is not a required backend.

## Identity and migration effects

Product name is `IPTV`, release version is `1.0.0`, and the application identifier is `com.devsolux.iptv`. The npm/Cargo package name remains lowercase `iptv` where ecosystem conventions require it. The previous identifier `net.devsolux.iptv` is not retained as an alias.

Changing the identifier means a different installed application/data location on relevant platforms. This source does not read or copy old credential stores and does not overwrite an existing application database. Re-add authorized sources to the new vault. Stable IDs are maintained for subsequent refreshes within this application's own source identity model.

## Passphrase vault removed

Earlier revisions of this application protected source secrets with a user passphrase. The passphrase and the unlock screen are gone: Rust now keeps a random per-profile key in a private file beside the database and opens the library directly. Records sealed under a passphrase cannot be read without it, so a profile that still contains the old `credentials.vault` is reset once at startup (its sources, channels, favorites and guide overrides are removed, the file deleted) and sources must be added again.

## Rights and release ownership

No license was invented for the supplied application artwork or flag collection. Their provenance is documented, and redistribution rights remain an owner review item. The source code is licensed under the MIT License (`LICENSE`). No production deployment, stream publication or signing operation has been performed.
