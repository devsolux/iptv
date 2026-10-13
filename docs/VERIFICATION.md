# Verification record

Date: **October 13, 2026**. This is verified source, **not a certified production release**.

## Environment

macOS on Apple Silicon (arm64), Node 24.21.0 (`.node-version` selects 26.11.1; `engines` accepts 22.12 or newer), npm 11.19.0, Rust and Cargo 1.99.0 (the `rust-toolchain.toml` pin), Python 3.11.17 and Playwright's Chromium headless shell. Dependencies were installed from the exact pins already declared in the manifests; `package-lock.json` and `src-tauri/Cargo.lock` were generated from those pins without running `make bootstrap`, so no version was upgraded.

## Executed checks

| Check | Result |
| --- | --- |
| `node scripts/project.mjs verify` (`make verify-source`) | Passed |
| `node scripts/project.mjs release` (`make verify-release`) | Passed: exact pins and both lockfiles |
| `npm run typecheck` (TypeScript 7.0.2, `tsc -b`) | Passed |
| Biome on all project sources (`src`, `tests`, `scripts`, root configs) | Passed, no diagnostics |
| `npm run lint` on the whole tree | Fails only on the vendored `public/flags/*.svg` (third-party assets); see open items |
| `npm test` | 11 frontend tests passed |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo test --workspace --locked` | 14 Rust tests passed |
| `python3 -m unittest discover -s tests/sql` | 11 SQLite invariant tests passed |
| `python3 -m unittest discover -s tests/tools` | 7 build-tool tests passed |
| `npm run build:web` | Passed |
| `npm run test:e2e` (Playwright, Chromium) | 4 browser specifications passed |
| `npm run tauri -- build --no-bundle -- --locked` | Passed: macOS arm64 release binary |
| Launch smoke test of that binary | Started and stayed up; created a `0700` profile with `0600` database and lock files; the relay listened on `127.0.0.1` only |
| `npm audit` | 0 vulnerabilities |
| `cargo audit` | 0 vulnerabilities; 2 upstream warnings via Tauri's Linux GTK stack (`proc-macro-error` unmaintained, RUSTSEC-2024-0370; `glib` 0.18 `VariantStrIter` unsound, RUSTSEC-2024-0429) |

`verification-results.json` holds the same results in machine-readable form. The SQL suite executes the shipped migration and the actual query/matching SQL extracted from `db.rs`. The Rust suite exercises the parsers, vault, network policy, manifest rewriting and the database adapter. The browser specifications run against the read-only development preview, not the native application.

## iptv-org integration (October 13, 2026)

The same environment re-ran every suite after adding iptv-org sources (official playlists and the JSON catalogue selection) beside the unchanged own-M3U sources.

| Check | Result |
| --- | --- |
| `node scripts/project.mjs verify` | Passed; the new `iptv_org_catalog` command is declared in `build.rs` and the permission file |
| `npm run typecheck` | Passed |
| Biome on `src` and `tests` | Passed, no diagnostics |
| `npm run lint` (whole tree) | Still fails only on the vendored `public/flags/*.svg` diagnostics (pre-existing open item) |
| `npm test` | 16 frontend tests passed (5 new: code lists, aliases, official playlist paths, form-to-contract building) |
| `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo test --workspace --locked` | 21 Rust tests passed, 1 ignored (7 new: official URLs, streaming JSON limits, the origin/coverage/regions matrix, filters/aliases/options, serializer round trip through the M3U parser and database, aggregator network policy, version-1 database migration) |
| `cargo test -p iptv-core --test core -- --ignored live_` | Passed once against the live `iptv-org.github.io` data: 31,562 channels, 45,155 feeds and 17,968 streams parsed; 28 DE/TR news entries selected, serialized and re-parsed; 304 entries of `countries/uk.m3u` imported through the ordinary parser. Needs network; not part of `make check` |
| `python3 -m unittest discover -s tests/sql` | 12 SQLite invariant tests passed (migration 002 applied; version-1 upgrade covered) |
| `python3 -m unittest discover -s tests/tools` | 7 passed |
| `npm run build:web` | Passed |
| `npm run test:e2e` | 5 browser specifications passed (new: the source-type toggle renders the iptv-org form in the preview with imports still disabled) |
| Preview of the Sources page (`make dev-web`, built-in browser) | Source-type toggle, both selection methods and the pickers render; no console errors |

Not executed for this change: a native build, an iptv-org import inside the running native application (vault, job, activation) and playback of iptv-org streams. The live test covers download, parsing, selection and serialization with the production network policy, not the SQLite activation path inside a Tauri process.

## Playback network fixes and channel artwork (October 13, 2026)

Playing iptv-org streams in the native application produced policy, network, cancellation and player errors, and channel logos were never shown. The stream clients now send a browser-like default User-Agent, speak HTTP/1.1, keep one cookie jar per playback session, probe with a plain GET instead of a Range request, follow up to ten re-checked redirects and may accept unverified certificates per source consent; official-playlist imports skip streams the source policy cannot fetch; closing a finished session no longer cancels the next one; playback errors are reported as stream errors; and artwork is served through a new relay route from a bounded private cache.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed (reqwest gained the `cookies` feature; `Cargo.lock` updated) |
| `cargo test --workspace --locked` | 22 passed, 3 ignored (new: artwork sniffing, cache format, image policy, `channel_logo` through the vault) |
| `cargo test -p iptv-core --test core -- --ignored live_` | 3 live checks passed once: the dataset selection check as before; a real HLS stream from a public host relayed through the loopback relay (manifest rewritten, first segment fetched; one host answered 403 upstream); `countries/de.m3u` imported inside a running `Core` (288 channels) with a PNG logo served by the `/logo` route, then from the cache, and a wrong token rejected |
| `npm run typecheck`, Biome on `src` and `tests`, `npm test` | Passed, 16 tests |
| `python3 -m unittest discover -s tests/sql` | 12 passed (migration 003 applied) |
| `node scripts/project.mjs verify`, `npm run build:web`, `npm run test:e2e` | Passed, 5 specifications |

Not executed: launching the native application against these changes, artwork rendering inside the WebView under the updated CSP, Android, and playback of streams that need cookies or unverified certificates (covered by code and unit tests only).

## Player controls and engine fallbacks (October 13, 2026)

The player page was rebuilt with an icon toolbar that auto-hides over the video, fullscreen that shows only the picture, automatic fallback to the next stream variant when a stream fails before its first frame, distinct messages for codec, format and connection failures, and mpegts.js (pinned 1.8.2) for raw MPEG-TS/FLV streams that WebKit's `<video>` rejects; the relay probe now reports `mpegts` and `flv` containers.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check`, clippy `-D warnings`, `cargo test --workspace --locked` | Passed, 22 tests, 3 ignored |
| `npm run typecheck`, Biome on `src` and `tests`, `npm test` | Passed, 16 tests |
| `npm run build:web`, `npm run test:e2e` | Passed, 6 specifications (new: player toolbar, error card and Back action in the preview) |
| Preview of the player page (built-in browser) | Top chrome, icon toolbar, volume slider and error card render |

Not executed: live playback with the rebuilt controls inside the native application, raw MPEG-TS playback through mpegts.js, and the variant fallback against a real failing stream.

## Focus styling, artwork centring and Fire OS measures (October 13, 2026)

Channel artwork is centred in its card; text fields and selects draw an accent border on focus instead of the WebView's light-blue ring while buttons and cards keep the strong ring; own styles no longer use `:has()`, `inset` or `backdrop-filter`, `Array.prototype.at` is gone, and the build lowers syntax to Chromium 84 / Safari 14. Remote behaviour: entering a page on a screen without hover no longer focuses the search field, Back first leaves a text field, the desktop player maps rewind, fast-forward and stop media keys, and the Android Media3 dialog handles the same keys (compiled only by `make build-android`).

| Check | Result |
| --- | --- |
| `npm run typecheck`, Biome on `src` and `tests`, `npm test` | Passed, 16 tests |
| `npm run build:web` with the lowered target | Passed |
| `npm run test:e2e` | Passed, 7 specifications (new: Back leaves the search field before the close dialog) |
| Preview (built-in browser) | Search field and Source select show the accent focus border |

Not executed: the Kotlin change (no local Android toolchain), any Fire OS device, and the hover-less focus behaviour on a real remote.

## Stream audit: "format not supported" on iptv-org streams (October 13, 2026)

A new ignored audit test (`live_relay_audit_of_public_streams`) drives the relay the way a native player does for 36 streams from the German, Turkish and US official playlists: plain, ranged (`Range: bytes=0-1`) and HEAD requests for the root playlist, then the first variant playlist and media segment, plus the advertised `CODECS`. An ffprobe pass over the same upstream streams showed H.264/AAC everywhere except one MPEG-2/MP2 stream that no browser can decode; codecs were not the cause.

| Finding | Before | After |
| --- | --- | --- |
| Ranged root playlist request through the relay | 21 of 30 reachable streams answered 502: the host honoured the range with a two-byte 206 and the relay tried to parse it as a manifest | 30 of 30 answer 200 |
| HEAD root playlist request through the relay | 4 of 30 answered 502 (hosts rejecting HEAD) | 30 of 30 answer 200 |
| Unreachable upstream hosts (offline, 403, timeouts) | 6 of 36 | 6 of 36, reported as a stream error and subject to variant fallback |

WebKit's native HLS player sends exactly those probes and reports any failure of the main resource as "format not supported", which produced the reported message. The relay now fetches playlist-like resources whole, re-fetches a ranged response that turns out to be a playlist, and answers HEAD from a GET. The desktop player uses hls.js first with native HLS as the fallback and maps relay and upstream failures to distinct messages. Checks: clippy, 23 Rust tests, 16 unit tests, typecheck, Biome, web build and 7 browser specifications pass; the audit and the other three live checks were run once more against the live hosts. Not executed: the native application against the rebuilt engine order.

## Defects found by this verification

Running the real toolchain for the first time exposed defects that are now fixed and covered:

- The Rust core did not compile against its declared quick-xml 0.42 (changed text, name and attribute APIs).
- A cancelled import reported `ErrorKind::Interrupted`, which std and quick-xml retry: a cancelled XMLTV import spun forever and held the single import permit (regression test in `tests/core.rs`).
- An import held a SQLite write transaction while the parser waited on the network, so toggling a favorite or editing sources during a refresh failed with a storage error (regression test in `tests/core.rs`).
- Every catalogue change reset the channel pager to an empty result, collapsing the virtual grid and returning to the top (unit and browser regression tests).
- XMLTV guide URLs containing commas were split into invalid URLs when a source was added.
- Programme titles with characters outside the Basic Multilingual Plane could exceed the UI contract's UTF-16 bound and break the guide window.
- `scripts/project.mjs` silently skipped every task, exiting 0, when the checkout path contained a symlink (for example macOS temporary directories), so the release gate could pass without running.

## Not executed / not established

Android host generation, Gradle/Media3 compilation and APK/AAB packaging; Windows and Linux builds; the macOS universal build, `.app`/`.dmg` bundling, signing and notarization; GitHub Actions runs of these workflows for this revision; real streaming, decoder and hardware-acceleration behavior; Fire OS 6/7/8 devices, 60 fps, memory, thermal and long-session behavior; dependency license review.

## Open items

- Resolved on October 13, 2026: `public/flags` is excluded in `biome.json` and the configuration was migrated to `rules.preset`, so `npm run lint` and `make check` pass; the `web` job of the verification workflow, which had failed on exactly this step since the first push, is green again. `verify.yml` now pins runner images (Ubuntu 24.04, macOS 15, Windows 2025, ahead of the announced `ubuntu-latest` move to Ubuntu 26), caches Cargo builds and keeps Playwright traces of failed specifications. `releases.yml` builds from the committed lockfiles instead of re-resolving dependencies with `make bootstrap` in CI, no longer depends on the uncommitted `docs/dependencies-resolved.json`, uses Temurin JDK 21 for the Android host and signs eleven platform packages.

## Release acceptance checklist

1. Keep dependencies pinned and both locks committed; review API changes, vulnerability and license reports after any `make bootstrap`. Run `make fmt`, `make check`, `make test-sql`, `make test-tools` and `make test-e2e` without weakening tests.
2. Build each native target on the correct OS, inspect merged Android release network/backup/TV configuration, verify macOS ATS and all platform media transports.
3. Test authorized HLS/DASH/progressive streams, provider headers, redirects, byte ranges, extensionless manifests, session cancellation, wrong credentials and unsupported formats. Test external-manifest references against the network policy.
4. Test vault create/unlock/lock, wrong passwords, fresh installs, database migrations, interrupted/empty imports, stable favorites, EPG timezone/matching ambiguity and input size limits.
5. Verify physical Fire TV 6/7/8 launch, actual WebView versions, remote input, memory, frame timing, decode behavior, background/resume and prolonged playback. Do not label an unsupported device supported merely because an APK builds.
6. Review signing, identifiers, artwork rights and store rules. Release only after these owner-controlled gates are satisfied.
