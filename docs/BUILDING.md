# Building and dependency maintenance

Snapshot reviewed on **October 13, 2026**. Product: **IPTV**, identifier **com.devsolux.iptv**. Native packages must be produced on appropriate build hosts; this source archive contains no compiled binaries.

## Prerequisites

Use Node 22.12 or newer; `.node-version` selects 26.11.1. npm 10.9 or newer is accepted. `rust-toolchain.toml` selects Rust 1.99.0 with Rustfmt and Clippy. Install platform prerequisites from the [Tauri documentation](https://v2.tauri.app/start/prerequisites/). No project task performs global package installation.

macOS needs Xcode command-line tools; the configured application minimum is macOS 13.3. Universal builds need the `aarch64-apple-darwin` and `x86_64-apple-darwin` Rust targets. Windows requires the MSVC build tools and WebView2. Linux needs the GTK/WebKitGTK development packages and the appropriate GStreamer codecs for actual media playback. System codec availability must be tested separately from compilation.

Android needs JDK 17 or 21 (the release workflow uses Temurin 21), SDK 36 and an NDK/toolchain compatible with the selected Tauri CLI. Configure `JAVA_HOME`, `ANDROID_HOME` and `NDK_HOME`. Media3 1.11 requires Kotlin 2.2 or newer; inspect the generated host's Kotlin/AGP/Gradle versions against the [Media3 release requirements](https://developer.android.com/jetpack/androidx/releases/media3). The local plugin does not create a second conflicting AGP version authority.

## Declared main package versions

| Package | Source-delivery version |
| --- | --- |
| Ionic React | 9.0.7 |
| React / React DOM | 19.3.0 |
| React Router | 8.4.0, used without Ionic's router adapter |
| Vite / TypeScript | 8.3.4 / 7.0.2 |
| Tailwind CSS | 4.3.3 |
| Tauri Rust / JavaScript API | 2.12.3 |
| Tauri CLI | 2.12.2 |
| React typings (`@types/react`, `@types/react-dom`) | 19.3.0 |
| TanStack React Virtual / Zustand | 3.14.17 / 5.0.15 |
| hls.js / dash.js | 1.7.3 / 5.2.1 |
| Android Media3 | 1.11.1 |

All direct declarations are in `package.json`, the Cargo workspace and the player Gradle file. Upstream release information is not proof that the combined graph compiles. Every direct npm dependency is an exact version, and the resolved graphs are committed as `package-lock.json` and `src-tauri/Cargo.lock`; see [VERIFICATION.md](VERIFICATION.md) for what was built and tested against them.

## Normal installation versus dependency upgrades

Day-to-day work uses `make install` (`npm ci` and `cargo fetch --locked`). The sequence below is only for a deliberate upgrade: `make bootstrap` moves every pin to the current stable release and regenerates both lockfiles.

```sh
make doctor
make bootstrap
make fmt
make check
make test-sql
```

Bootstrap obtains all version metadata before writing manifests. It rejects prereleases and a Tauri major outside 2. It does not force incompatible peers or use `--legacy-peer-deps`. If installation fails after manifests are written, inspect the changes before retrying. A newer library API may require an implementation change; updating versions is not a compatibility test.

Review `docs/dependencies-resolved.json`, `package-lock.json`, `src-tauri/Cargo.lock`, manifests and the Rust toolchain, then commit them. Future installations use `make install` (`npm ci` and `cargo fetch --locked`). Desktop/Android release build targets fail rather than silently manufacture a different dependency graph.

## Development and packaging

`make dev` runs the desktop native application and starts Vite through Tauri's hook. `make build` builds the frontend once through Tauri, then the current platform. `make build-web` produces only application frontend assets; these require native IPC in production and are not a standalone hosted application.

`make dev-web` enables a development-only read-only preview. The production branch never silently substitutes synthetic data for a native failure.

For Android run `make android-init`, review the generated host, then `make dev-android` or `make build-android`. The configure script adds the TV launcher/banner, makes touch and Leanback optional, disables Android backup and permits Java cleartext only for `127.0.0.1` in release. Debug-only resources allow the Vite development server. Rust independently enforces upstream cleartext/LAN policy. Always inspect the merged **release** manifest and resources. The script refuses an existing custom network policy instead of overwriting it.

Android host generation is intentionally not represented by guessed Gradle boilerplate in this archive. Tauri generates it with the installed CLI, then the deterministic configuration script applies the IPTV-specific changes. Keep any deliberate native-host customizations under version control in your working repository; remove the corresponding ignore rule when doing so.

Windows users without GNU Make can use `npm run dev`, `npm run bootstrap`, `npm run build`, `npm run android:init`, `npm run build:android` and `npm run clean`. Rust checks are `cargo test --manifest-path src-tauri/Cargo.toml --workspace --locked` and `cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets --locked -- -D warnings`. Run `npm run verify:release` before packaging via npm directly.

## Verification and cleanup

`make check` runs source checks, full TypeScript checks, Biome, frontend tests, Rustfmt, Clippy and Rust tests sequentially, even under parallel Make. SQL, tooling (`make test-tools`) and browser suites are separate explicit targets. Install Playwright's Chromium before `make test-e2e` using `npx playwright install chromium` after `npm ci`.

`clean`, `clean-web`, `clean-rust` and `clean-android` use a fixed output allowlist and refuse symlink ancestors. They do not delete lockfiles, source, `node_modules` as a whole, generated native source or the app data directory. Dependency repair/removal is intentionally not hidden inside `clean`.

## Release ownership

`verify.yml` runs on every push and pull request: source policy, SQL and tooling suites, the frontend checks with browser tests, and native fmt/clippy/test plus an unbundled Tauri build on pinned Ubuntu 24.04, macOS 15 and Windows 2025 runners. `releases.yml` runs on `workflow_dispatch` or a published release: it validates identity and pins, re-runs the checks, builds desktop and Android packages from the committed lockfiles, signs them with keyless Sigstore and attaches them to the release. Neither workflow resolves or upgrades dependencies; CI does not publish by itself. Dependency-dependent jobs install the committed lockfiles with `npm ci` and Cargo `--locked`; a missing or stale lock fails them instead of resolving a different graph. Signing certificates, Android keystores, passwords and store uploads are owner-controlled release inputs outside the repository. Do not place them in `VITE_*` variables or frontend code.

No OS release, code-signing, package audit, store acceptance or device-performance claim is implied by the presence of a build target. Follow [VERIFICATION.md](VERIFICATION.md).

Primary references: [Ionic 9 migration](https://ionicframework.com/docs/updating/9-0), [React Router releases](https://github.com/remix-run/react-router/releases), [Tauri releases](https://github.com/tauri-apps/tauri/releases), [Vite releases](https://github.com/vitejs/vite/releases), [Rust stable documentation](https://doc.rust-lang.org/stable/std/), [Tauri CLI](https://v2.tauri.app/reference/cli/).
