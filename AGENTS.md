# AGENTS.md

Guidance for AI coding agents (and people) changing this repository. Read it before editing; read
[ARCHITECTURE](docs/ARCHITECTURE.md) and [SECURITY](docs/SECURITY.md) before moving a boundary.

## Project

IPTV `1.0.0` (`com.devsolux.iptv`) is an Ionic React + Tauri 2 application for user-supplied M3U
playlists, iptv-org selections and XMLTV guides, built for desktop and Android/Fire TV remotes. **Rust owns** ingestion,
persistence, secrets, network policy and playback authorization. **The UI owns** presentation,
routing and D-pad focus. Media bytes never cross JSON IPC; the UI only receives loopback relay URLs.

| Path | Responsibility |
| --- | --- |
| `src/` | React UI: `pages/`, `components/`, `hooks/` (focus, paging, player with hls.js / dash.js / mpegts.js engines), `lib/` (bridge, contracts, pager, `playback.ts` failure messages and reconnection budget, iptv-org forms, preview data), `store/` (Zustand UI state) |
| `src-tauri/src/lib.rs` | Tauri commands, app state, lifecycle |
| `src-tauri/crates/iptv-core/` | M3U/XMLTV parsers, SQLite generations (`db.rs`), sealed key store (`vault.rs`), network policy (`network.rs`), loopback relay (`relay.rs`) and HLS/DASH rewriting (`manifest.rs`), artwork cache (`logos.rs`), iptv-org integration (`iptv_org.rs`) |
| `src-tauri/plugins/player/` | Android Media3 player plugin (Kotlin) |
| `scripts/` | `project.mjs` (verify/clean/doctor), `bootstrap.mjs` (dependency upgrades), `android-configure.mjs` |
| `tests/` | `unit/` (Node), `e2e/` (Playwright), `sql/` and `tools/` (Python `unittest`) |
| `docs/` | Architecture, security, compatibility, building, verification record |

## Fixed identity: do not change

- Version is `1.0.0` everywhere: `package.json`, `src-tauri/tauri.conf.json`, the three `Cargo.toml`
  files, `AppStatus` in `src-tauri/src/lib.rs`, `statusSchema` in `src/lib/contracts.ts` and
  `src/lib/preview.ts`. `scripts/project.mjs verify` and `.github/workflows/releases.yml` enforce it.
- Product `IPTV`, identifier `com.devsolux.iptv`, package name `iptv`. Tauri stays on major 2.
- Do not bump versions, rename the identity or create release tags unless asked.

## Branch policy: `main` only

- `main` is the only branch, locally and on `origin`. Work, commit and push directly on `main`.
- Do not create feature branches, worktrees, forks or pull requests, and do not push any other
  branch. If a tool or session starts on another branch or worktree, move the change to `main`
  instead of opening a PR.
- Commit only when asked; push only when asked, with `git push origin main`. Never force-push,
  rewrite published history or delete `origin/main`.
- Before pushing, `git fetch origin main`; if `origin/main` moved, update with `make github-pull`
  (fast-forward only) and stop on divergence rather than merging or rebasing on your own.
- Stage files by name. `make github-push` stages everything with `git add -A` and a timestamp
  message; it is for the owner, not for agents.

## Commands

```sh
make install      # npm ci + cargo fetch --locked (committed lockfiles)
make dev-web      # browser preview with 50,000 synthetic channels (no native features)
make dev          # native desktop app
make check        # verify-source, typecheck, Biome, unit tests, rustfmt, clippy -D warnings, cargo test
make test-sql test-tools   # SQLite invariants and build-tool safety (Python 3)
make test-e2e     # Playwright; run `npx playwright install chromium` once
make fmt          # Biome --write + cargo fmt
make verify-release        # exact pins + lockfile gate (metadata only, not a certification)
make android-init build-android   # generated Android host and APK (needs the Android SDK, JDK 21)
```

Every `make` target has an npm/cargo equivalent (see `docs/BUILDING.md`). Before committing run
`make check` and `make test-sql test-tools`; add `make test-e2e` when UI behavior changes.

## Conventions

- TypeScript is strict (`noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`). Biome formats
  with 2 spaces, single quotes, 100 columns. Rust is edition 2024, default rustfmt, clippy with
  `-D warnings`. Keep comments sparse and about *why*.
- Dependencies are exact versions only (`.npmrc` has `save-exact`); ranges or `latest` fail
  `make verify-release`. Upgrades are a deliberate `make bootstrap` run (it moves **every** dependency
  to latest); review API changes and commit `package-lock.json` and `src-tauri/Cargo.lock` together.
- Suppress a lint rule only with a `biome-ignore` comment that states the reason.
- `public/flags/*.svg` are vendored third-party assets: keep them byte-identical. Asset rights are
  recorded in `public/ASSET_PROVENANCE.txt`; the MIT license covers the source code.

## Invariants that are easy to break

1. **Command allowlist (four places).** A native command must appear in `src-tauri/build.rs`,
   `src-tauri/permissions/application.toml` and `tauri::generate_handler!` in `src-tauri/src/lib.rs`,
   and be wrapped in `src/lib/bridge.ts` with a zod schema in `src/lib/contracts.ts`.
   `make verify-source` checks that `build.rs` and the permission file agree.
2. **Native authority.** `capabilities/main.json` grants no shell, SQL, HTTP or filesystem plugin and
   no remote URLs. Never disable the CSP in `tauri.conf.json`.
3. **Secrets.** Provider URLs, headers and stream tokens are sealed by `vault.rs` under the per-profile
   key (`credentials.key`, created on first start; there is no passphrase or unlock flow). Never put them
   in routes, logs, UI DTOs, error messages or `VITE_*` variables. UI errors go through `Error::public()`.
4. **Network.** Every upstream request goes through `NetworkPolicy` / `MediaNetwork` (host allowlist,
   DNS pinning, loopback/link-local/metadata blocks, redirect re-checks). Do not add a bypassing client.
5. **SQL.** Parameterized queries only. Schema changes need a new migration and `user_version`;
   `Database::open` rejects unknown versions. `tests/sql/test_catalogue.py` extracts `CHANNEL_FILTER`
   and the `match_epg` statements from `db.rs` source, so keep them as single string literals.
6. **Imports.** `Import` queues rows and writes 256-row batches in short transactions. Never hold a
   SQLite write transaction while the parser waits on the network: concurrent favorites fail.
7. **Cancellation.** `LimitedRead` must never report `io::ErrorKind::Interrupted`; std and quick-xml
   retry that kind, so a cancelled import would spin forever and keep the single import permit.
8. **quick-xml 0.42.** Names and content are `str`: use `normalized_value(XmlVersion::Implicit1_0)` and
   `xml10_content()`. DTDs and external entities stay rejected.
9. **Catalogue paging.** `CataloguePager.reset()` is for new filters; `refresh()` is for catalogue
   changes and keeps the total and scroll position. Caches stay bounded (6 pages of 80, at most two
   requests in flight). Never load the whole catalogue into React state.
10. **Contracts.** zod lengths count UTF-16 units while Rust limits bytes or chars; keep bounds
    compatible (a 512-char title can be 1,024 units).
11. **Remote focus.** Arrow keys are handled by containers (`useDpadFocus`, `dpadScope`). Scrollable
    regions keep `tabIndex={0}`; key-handling containers are labelled `<section>`s or
    `role="toolbar"`. Back/Escape flows through `NavigationLifecycle` in `src/App.tsx`.
12. **Preview data.** `src/lib/preview.ts` runs only when `import.meta.env.DEV && !isTauri()`. It is
    never a production fallback.
13. **Two source kinds.** `SourceInput.iptv_org` is `None` for own M3U playlists, whose import path
    (`Core::import_playlist`, `m3u::parse`) must keep its behaviour. iptv-org sources go through
    `iptv_org.rs`: official playlist URLs are built only by `official_path`, catalogue selections run
    through the single `select` policy and are serialized with `serialize` into M3U that the same
    parser ingests. Keep `select` deterministic and covered by the matrix in `tests/core.rs`.
14. **Aggregator network policy.** Only sources with `iptv_org` set get `open_hosts`; the IP, scheme,
    redirect and DNS checks still apply, LAN access and CDN lists are rejected, and
    `NetworkPolicy::public` (single host, HTTPS) is the only policy for the catalogue command.
15. **Migrations.** `db::MIGRATIONS` is ordered; each file ends with `PRAGMA user_version = N`.
    `tests/sql/test_catalogue.py` runs them all, so a new migration must also keep that suite green.
16. **Artwork.** Logos reach the UI only through `/logo/{token}/{channel}` on the relay
    (`Core::logo`, `logos.rs`, `NetworkPolicy::images`). Never put upstream image URLs in DTOs or
    widen the CSP `img-src` beyond `'self' data: http://127.0.0.1:*`.
17. **Stream clients.** `MediaNetwork` and `get_blocking` send `DEFAULT_USER_AGENT`, speak
    HTTP/1.1, keep a per-session cookie jar and honour `accept_invalid_certs` only when the source
    consented. Playback errors surface as `Error::Stream` / `Error::StreamPolicy`, never raw text.
    The relay never forwards `Range` or HEAD upstream for playlist resources (`playlist_like`,
    `manifest_prefix` in `relay.rs`); `live_relay_audit_of_public_streams` is the audit to re-run
    after touching `dispatch`.
18. **Relay sessions.** The probe and every relay response classify streams with the single
    `detect_kind` rule (playlist text, MPEG-TS sync at any packet offset, FLV, content type, address).
    The probe's open response is parked (`Parked`, 20 s) for the player's first plain GET of the
    root; never reintroduce a second upstream connection for it. Registered resources live for the
    session under `MAX_RESOURCES` with least-recently-used eviction, never a time-to-live: on-demand
    playlists are fetched once. Every media response, errors included, goes through `finish` so the
    WebView gets CORS headers and can read 403/413/415. Only `SegmentTemplate` addresses carry
    `$Identifier$` placeholders (`register(.., template)`).
19. **Manifests.** In a multivariant HLS playlist, `manifest::hls` drops only optional entries
    (variants, renditions, hints, session data) the policy rejects and fails when no variant
    remains; media playlists never skip segments. Keys pass only for `METHOD` NONE, AES-128 or
    SAMPLE-AES with the `identity` key format. DASH `UTCTiming` HTTP sources are relayed under the
    source policy and the upstream `Date` header stays forwarded and CORS-exposed: without a clock
    source dash.js stops every live DASH stream.
20. **Player recovery.** Failure messages and engine error mapping live in `src/lib/playback.ts`
    (dependency-free, unit tested). dash.js clock, subtitle and pruning errors (16, 33, 21) are not
    fatal. A stream that never played fails so `PlayerPage` can try the next variant; one that
    played and dropped reconnects through a fresh relay session within `ReconnectBudget` (three
    per two minutes). The Android plugin rejoins the live edge on `ERROR_CODE_BEHIND_LIVE_WINDOW`
    and reconnects within the same bound.
21. **WebView floor.** Keep the frontend within Chromium 84: no `:has()`, `inset`, `backdrop-filter`
    or `Array.prototype.at` in own code, and keep `build.target` in `vite.config.ts` at
    `chrome84`/`safari14`. See `docs/COMPATIBILITY.md` before using a newer platform feature.

## Testing notes

- `npm test` runs `tests/unit/*.test.ts` with Node type stripping: import sources with explicit `.ts`
  extensions and keep cross-file imports type-only or dependency-free.
- Rust integration tests live in `src-tauri/crates/iptv-core/tests/core.rs`, including the iptv-org
  selection matrix (origin versus coverage, regions, subdivisions, cities, blocklist, labels,
  aliases, deduplication), the serializer round trip through the M3U parser and database, manifest
  rewriting and relay CORS. Private relay internals (registry eviction, placeholders) are tested
  in the `#[cfg(test)]` module of `relay.rs`.
- The network policy blocks loopback, so the relay cannot be tested against a local upstream.
  Live checks are `#[ignore]`d: `cargo test -p iptv-core --test core -- --ignored --nocapture
  live_relay` (run from `src-tauri/`; macOS has no `timeout` command, so do not wrap it).
- E2E specs run against the `dev:web` preview, not the native app.
- The Android plugin (`src-tauri/plugins/player/android`) is not covered by `make check`; change it
  conservatively. The `android` job of `verify.yml` builds a debug aarch64 APK on every push. After `make android-init`, compile it alone with JDK 21:
  `cd src-tauri/gen/android && ANDROID_HOME=~/Library/Android/sdk ./gradlew --offline
  :tauri-plugin-iptv-player:compileReleaseKotlin`. Device playback still needs `make build-android`.
- Record what actually ran in `docs/VERIFICATION.md`. Never claim device, signing or playback results
  that were not executed, and never fabricate lockfiles.

## Never commit

Keystores, signing certificates, `.env*` files, key stores or databases (all gitignored), and generated
output (`dist/`, `src-tauri/target/`, `src-tauri/gen/`, `src-tauri/permissions/autogenerated/`).
