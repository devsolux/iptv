# Verification record

Date: **October 13, 2026**. This is an implementation-source delivery, **not a verified production release**.

## Executed checks

The authoring environment has Node 22.16.0, npm 10.9.2, Python 3.13.5 and a global TypeScript 5.8.3 parser. It has no Rust/Cargo toolchain and cannot reach package registries for installation.

| Check | Result |
| --- | --- |
| `npm test` | 10 frontend algorithm tests passed |
| `python -m unittest discover -s tests/sql -v` | 11 SQLite invariant tests passed |
| TypeScript 5.8.3 parser scan | 33 TS/TSX files; no syntax diagnostics (not semantic type checking) |
| Node syntax checks | All build/configuration scripts parsed |
| Source identity and declared authority checks | Passed |
| `python -m unittest discover -s tests/tools -v` | 7 build-tool safety/invariant tests passed |

`verification-results.json` records final repeat checks and their actual outputs. The SQL suite executes the shipped migration and extracts the actual Rust query/matching SQL. It does not execute the Rust database adapter. The Node tests cover deterministic focus/paging behavior; they are not React component or native tests.

## Not executed / not established

Dependency installation and peer compatibility; full TypeScript 7 semantic checking; Biome and Rustfmt; Cargo compilation, Clippy and the 11 written Rust tests; the 3 written Playwright browser specifications; Android host generation/Gradle/Media3 compilation; native desktop packages; signing; real streaming/decoder behavior; hardware acceleration; Fire OS support; 60 fps; memory/thermal/long-session behavior; package vulnerability/license audits.

The lockfile/release gate is expected to fail in this source delivery: `package-lock.json` and `src-tauri/Cargo.lock` are absent, and two React typing dependencies await exact resolution. Lockfiles were not fabricated. The explicit bootstrap performs that resolution on a networked machine; follow it with formatting, type checking, tests and actual native builds. Build/API fixes may be necessary once the real dependency graph is installed.

The CI configuration is a verification pipeline to run on the resolved repository, not evidence that CI has already passed. Dependency-dependent jobs intentionally fail until reviewed lockfiles are committed.

## Release acceptance checklist

1. Resolve/pin dependencies; review APIs and license/vulnerability reports; commit both locks and generated toolchain changes. Run `make fmt`, `make check`, `make test-sql` and `make test-e2e` without weakening tests.
2. Build each native target on the correct OS, inspect merged Android release network/backup/TV configuration, verify macOS ATS and all platform media transports.
3. Test authorized HLS/DASH/progressive streams, provider headers, redirects, byte ranges, extensionless manifests, session cancellation, wrong credentials and unsupported formats. Test external-manifest references against the network policy.
4. Test vault create/unlock/lock, wrong passwords, fresh installs, database migrations, interrupted/empty imports, stable favorites, EPG timezone/matching ambiguity and input size limits.
5. Verify physical Fire TV 6/7/8 launch, actual WebView versions, remote input, memory, frame timing, decode behavior, background/resume and prolonged playback. Do not label an unsupported device supported merely because an APK builds.
6. Review signing, identifiers, artwork rights and store rules. Release only after these owner-controlled gates are satisfied.
