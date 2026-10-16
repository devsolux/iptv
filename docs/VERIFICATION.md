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

## Passphrase vault removed (October 13, 2026)

The unlock screen, the Lock button, the `unlock`/`lock` commands, the `unlocked`/`vaultExists` status fields, `Error::Locked`, the Argon2 dependency and the vault page styles are gone. Secrets stay sealed with XChaCha20-Poly1305 under a random per-profile key (`credentials.key`, `0600`) that `Core::open` creates on first start; a profile still holding the former `credentials.vault` is reset once. Checks: clippy, Rust tests (new: key persistence, context binding, zeroization, damaged-key rejection, legacy-profile reset through `Core::open`), unit, SQL, tooling, typecheck, Biome, web build and browser specifications pass; `make check` passes.

## Label filters on both iptv-org methods (October 13, 2026)

Live `streams.json` carries exactly two labels, `Not 24/7` (1,805 streams) and `Geo-blocked` (1,202), which the catalogue selection already matched; the gap was that official playlists mark the same labels in channel names (`[Geo-blocked]`, `[Not 24/7]`) and were never filtered, and the toggles were hidden for that method. Both exclusions now default to on, apply to official playlists through the name markers (`Filters::excludes_official_name`) and are offered for both methods. Checks: clippy, Rust tests (defaults, name markers, catalogue exclusion), unit, typecheck, Biome, web build and browser specifications pass.

## Release workflow repair (October 13, 2026)

Release run 38437449018 (tag `v1.0.0`) passed the prepare, verify, Linux and Windows jobs and failed in two places, both fixed in `releases.yml`:

| Finding | Cause | Fix |
| --- | --- | --- |
| macOS universal: `lipo: unknown architecture` after the `.dmg` and `.app` had been built | `-verify_arch` received the binary after the architecture list, and Xcode 27's lipo rejects the documented order as well (`-verify_arch requires exactly one input file`) | The step reads `lipo -archs` and requires exactly the `arm64` and `x86_64` slices |
| Android armv7 and aarch64: `Failed to find package 'platforms;android-37'` | The Tauri CLI 2.12.2 host template declares `compileSdk = 37`, and Google publishes API 37 only as minor-versioned platforms (`platforms;android-37.0`, `37.1`, `37.2`); no channel offers `platforms;android-37` | The host step resolves every declared level to `platforms;android-N` or `platforms;android-N.0` from the stable listing and fails with that listing when neither exists; the SDK step installs only the NDK and build tools and no longer reinstalls `cmdline-tools;latest` into a second directory |

Reproducing the Android job on this machine (Tauri CLI 2.12.2 host, AGP 9.3.1, Gradle 9.6.1, Kotlin 2.2.10, JDK 21, NDK 30.0.16138531, platforms `android-37.0` and `android-36`) exposed a third defect the run had not reached: the player plugin imports `androidx.activity.ComponentActivity` and `OnBackPressedCallback`, but `tauri-android` keeps appcompat as an `implementation` dependency, so `compileReleaseKotlin` failed with unresolved references. The plugin now declares `androidx.activity:activity:1.13.0` (minimum compileSdk 36, AGP 8.9.1).

| Check | Result |
| --- | --- |
| `npm run tauri -- android init --ci`, `scripts/android-configure.mjs` and `--check` | Passed; the generated host declares `compileSdk = 37` / `targetSdk = 37` and compiled against the installed `android-37.0` platform (no `android-37` directory exists) |
| `npm run tauri -- android build --ci --target aarch64 --apk -- --locked` (macOS host) | Passed: `app-universal-release-unsigned.apk` (16.4 MB) containing only `lib/arm64-v8a/libiptv_lib.so`, which is what the ABI check of the collect step expects |
| `lipo -archs` slice check | Accepted a freshly linked arm64+x86_64 test binary and rejected a system binary carrying `arm64e` slices |
| `ruby -ryaml` parse of `releases.yml`, `node scripts/project.mjs verify` and `release`, `python3 -m unittest discover -s tests/tools` | Passed |
| `make check`, `make test-sql test-tools` after the change (the plugin's Gradle output is now gitignored, which Biome honours) | Passed: 24 Rust tests, 12 SQL and 7 tooling tests |

Not executed: the armv7 build, AAB packaging, APK signing, the GitHub Actions re-run of the release workflow and installation on a device.

## Streaming gaps (October 16, 2026)

An audit of the playback path (relay, manifest rewriting, network clients, desktop engines and the Android plugin) found these defects; each is fixed and, where a local test can reach it, covered:

| Defect | Effect | Fix |
| --- | --- | --- |
| The relay removed DASH `UTCTiming`; dash.js then tried its default clock server, which the CSP blocks, and the manifest `Date` header was not exposed through CORS | dash.js reported `TIME_SYNC_FAILED` (16) and the player stopped **every live DASH stream** with "cannot decode" | HTTP clock sources are relayed under the source policy, `direct` sources kept; upstream `Date` is forwarded and exposed; dash.js errors are mapped by code and clock, subtitle and buffer-pruning errors are not fatal |
| Relay error responses carried no CORS headers | The WebView saw every refusal as status 0, so the 403 (policy) and 415 (format) messages were unreachable | CORS headers on every media response, errors included, for the application's origins only (Rust test) |
| Resources expired 300 s after registration whenever another playlist was registered, under a hard cap of 8,192 with an O(n) prune per registration | On-demand HLS segments answered 410 after five minutes once a level, audio or subtitle playlist loaded; long DVR windows hit 413; large playlists rewrote in quadratic time | Resources live with their session under a 16,384 least-recently-used bound (Rust test) |
| The probe response was discarded and the player opened a second upstream connection | Slower start (second handshake); single-connection providers and one-time addresses refused the player | The probe response is parked for 20 s and serves the player's first plain request for the root |
| MPEG-TS was recognised only at offset 0; `video/mp2t` and `.ts` were ignored; `.m3u8` addresses were always parsed as playlists | Live TS starting mid-packet went through the failing `<video>` path first; a `.m3u8` address serving TS failed | `detect_kind` (playlist text, TS sync at any packet offset, FLV, content type, address) used by the probe and every relay response (Rust test) |
| One policy-rejected variant or rendition failed a whole HLS multivariant playlist; `$` in an HLS address was parsed as a DASH template | Streams with a plain-HTTP or off-host alternative failed; such addresses answered 415 | Rejected variants, renditions and hints are dropped, the playlist fails only when no variant remains; placeholders are parsed only in `SegmentTemplate` (Rust tests) |
| `METHOD=SAMPLE-AES` with the identity key format was rejected; key checks matched substrings | Clear sample-encrypted streams answered 415 | Attribute-exact key checks: AES-128 and identity SAMPLE-AES relayed, other key formats and `SAMPLE-AES-CTR` rejected (Rust test) |
| URL fragments in stream or redirect addresses failed the address policy | Such streams were reported as policy blocks | Fragments are dropped before the check |
| The HTTP client cache held its lock across DNS resolution | One slow lookup stalled every request of the session | Resolution happens outside the lock |
| Desktop player: a network failure after playback started was final, a live stream whose connection ended froze on its last frame, and the "Press Play" prompt stayed in the loading overlay | Live channels stopped on transient outages | Bounded reconnection through a fresh relay session (three per two minutes), `ended` handling, `capLevelToPlayerSize` (unit tests for the error mapping and budget) |
| Android Media3: falling behind the live window showed an error panel, a dropped live stream stayed ended, errors showed only the code name, playback started after 0.5 s of buffer | Stalled live channels required manual retries; frequent rebuffering | Rejoin the live edge on `ERROR_CODE_BEHIND_LIVE_WINDOW`, bounded reconnection on IO errors and live end, readable messages, 1.5 s / 3 s start thresholds |

| Check | Result |
| --- | --- |
| `make check` (verify-source, typecheck, Biome, unit tests, rustfmt, clippy `-D warnings`, cargo test) | Passed: 21 unit tests, 31 Rust tests (2 relay unit tests, 29 integration) |
| `make test-sql test-tools` | Passed |
| `make test-e2e` | 7 browser specifications passed |
| `./gradlew --offline :tauri-plugin-iptv-player:compileReleaseKotlin` (JDK 21, generated Android host) | Passed; the changed plugin compiled against Media3 |
| `live_relay_audit_of_public_streams` (three runs) and `live_relay_serves_a_public_iptv_org_stream` | 30 of 36 streams relay with root, ranged and HEAD requests answering 200, as before; the same 6 hosts are offline or refuse; one run saw a single transient HEAD 502 from a redirecting host that answered 200 in the other two. The live check relayed an HLS stream; its first candidate answered 403 because every variant of that playlist is plain HTTP on another host, which the policy correctly rejects |

Not executed: the native desktop application, a live DASH stream in the WebView, playback on an Android or Fire TV device and the full Android APK build.

## Workflow update (October 16, 2026)

Release run 38444833311 (tag `v1.0.0`, commit `750808b`) completed every job and attached eleven packages (Linux AppImage/deb/rpm, Windows NSIS/MSI, macOS universal `.dmg` and `.app.tar.gz`, unsigned Android APK/AAB for armv7 and aarch64), `SHA256SUMS` and a Sigstore bundle per file to the release. Those packages predate the streaming fixes; commits after `750808b` have no release packages until the workflow runs again (`workflow_dispatch` produces Actions artifacts only). Annotations on the latest verify and release runs contain only GitHub's macOS capacity notice.

Changes: every action stays pinned to the commit of its latest release, checked on October 16, 2026 through the GitHub API: `actions/checkout` v7.0.1, `actions/setup-node` v7.1.0, `actions/upload-artifact` v7.0.2, `actions/download-artifact` v8.0.2, `Swatinem/rust-cache` v2.9.2 and `sigstore/cosign-installer` v4.1.2 were current; `actions/setup-java` moved from v6.0.1 to v6.1.0 (no interface change for Temurin 21); Cosign's latest release is v3.1.3, inside the workflow's v3 gate. `verify.yml` gains `workflow_dispatch`, the npm download cache for jobs that run `npm ci` (the release workflow stays cache-free) and an `android` job that builds a debug aarch64 APK, so the Media3 plugin is compiled on every push instead of only at release time. The README documents installing releases, including the mandatory `xattr -cr /Applications/IPTV.app` step for the ad-hoc-signed macOS app; `docs/BUILDING.md` names the four Android signing secrets.

| Check | Result |
| --- | --- |
| Ruby YAML parse of both workflows | Passed |
| `node scripts/project.mjs verify`, Biome, `python3 -m unittest discover -s tests/tools` | Passed |
| Verify run 38907639373 (commit `af17b9d`, the streaming fixes and agent guides) | Passed: source, frontend and native checks on Ubuntu 24.04, macOS 15 and Windows 2025 |
| Verify run 38916981621 (commit `1bc9a83`, this change) | Passed all six jobs: source policy (under 1 min), frontend and browser tests (3 min), native checks on Ubuntu 24.04 (3 min), macOS 15 (3 min) and Windows 2025 (10 min), and the new Android debug build (aarch64, 6 min) |

Not executed: a run of `releases.yml` with setup-java v6.1.0 (the verify `android` job already uses that pin).

## Version 1.1.0 (October 16, 2026)

The version moves from `1.0.0` to `1.1.0` in every pinned place (`package.json` and the root entries of `package-lock.json`, `tauri.conf.json`, the three `Cargo.toml` files and their `Cargo.lock` entries, `AppStatus`, `statusSchema`, the preview status, the bootstrap User-Agent) and in both gates that hard-code it: `scripts/project.mjs verify` and the identity check of `releases.yml`, which accepts only the tags `1.1.0` or `v1.1.0`. Tauri derives the Android `versionCode` (1001000) from it. Release `v1.1.0` contains the streaming fixes, the workflow changes and the documentation since `v1.0.0`.

| Check | Result |
| --- | --- |
| `cargo metadata --locked --offline` | Passed: `Cargo.lock` matches the bumped manifests |
| `make verify-release` | Passed |
| `make check` | Passed: 21 unit tests, 31 Rust tests |
| `make test-sql test-tools` | Passed |
| `make test-e2e` | 7 browser specifications passed |

The packaging, signing and asset upload of `v1.1.0` run in the release workflow on GitHub; their result is shown on the release page and in the workflow run, not here.

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

Installation-signed Android packages (no signing secrets are configured, so the release carries `unsigned` APK/AAB files) and installing them on a device; macOS Developer ID signing and notarization (the `.dmg` is ad-hoc signed and needs `xattr -cr`); Windows Authenticode; release packages for commits after `750808b`; real streaming, decoder and hardware-acceleration behavior; Fire OS 6/7/8 devices, 60 fps, memory, thermal and long-session behavior; dependency license review.

## Open items

- Resolved on October 13, 2026: `public/flags` is excluded in `biome.json` and the configuration was migrated to `rules.preset`, so `npm run lint` and `make check` pass; the `web` job of the verification workflow, which had failed on exactly this step since the first push, is green again. `verify.yml` now pins runner images (Ubuntu 24.04, macOS 15, Windows 2025, ahead of the announced `ubuntu-latest` move to Ubuntu 26), caches Cargo builds and keeps Playwright traces of failed specifications. `releases.yml` builds from the committed lockfiles instead of re-resolving dependencies with `make bootstrap` in CI, no longer depends on the uncommitted `docs/dependencies-resolved.json`, uses Temurin JDK 21 for the Android host and signs eleven platform packages.
- Release run 38437449018 (`v1.0.0`) failed on the macOS `lipo` check and the Android `platforms;android-37` install; fixed on October 13, 2026 (see "Release workflow repair"). Resolved: release run 38444833311 for `v1.0.0` completed green the same day (see "Workflow update").

## Release acceptance checklist

1. Keep dependencies pinned and both locks committed; review API changes, vulnerability and license reports after any `make bootstrap`. Run `make fmt`, `make check`, `make test-sql`, `make test-tools` and `make test-e2e` without weakening tests.
2. Build each native target on the correct OS, inspect merged Android release network/backup/TV configuration, verify macOS ATS and all platform media transports.
3. Test authorized HLS/DASH/progressive streams, provider headers, redirects, byte ranges, extensionless manifests, session cancellation, wrong credentials and unsupported formats. Test external-manifest references against the network policy.
4. Test fresh installs (key file creation), the one-time reset of legacy passphrase profiles, database migrations, interrupted/empty imports, stable favorites, EPG timezone/matching ambiguity and input size limits.
5. Verify physical Fire TV 6/7/8 launch, actual WebView versions, remote input, memory, frame timing, decode behavior, background/resume and prolonged playback. Do not label an unsupported device supported merely because an APK builds.
6. Review signing, identifiers, artwork rights and store rules. Release only after these owner-controlled gates are satisfied.
