# IPTV

**Version 1.0.0 · Application identifier `com.devsolux.iptv`**

A unified Ionic React / Tauri 2 application for user-supplied IPTV playlists and XMLTV guides. Catalogue processing, persistence, credentials and playback authorization belong to Rust; the UI owns presentation and remote interaction.

> **Delivery status:** implementation source, not a compiled or certified release. Native builds, full dependency-aware type checking and physical-device playback have not been verified. Dependency lockfiles could not be generated in the authoring environment. Read [verification](docs/VERIFICATION.md) and [compatibility](docs/COMPATIBILITY.md) before release.

## First build

Install the native prerequisites for your operating system and a stable Node/Rust toolchain, then:

```sh
make doctor
make bootstrap
make fmt
make check
make dev
```

`bootstrap` is an **explicit dependency-maintenance step**: it queries the current stable npm, Cargo, Rust and Media3 releases, pins exact versions and generates both lockfiles. Review the resulting manifest/API changes and commit the locks. Two React typing packages remain `latest` in this source delivery; release checks deliberately reject those until bootstrap resolves them. No fake lockfiles are included.

On subsequent machines, use `make install` to consume the committed dependency graphs. Ordinary builds never update packages. Equivalent npm scripts work without Make; see [building](docs/BUILDING.md).

`make dev-web` provides a read-only, 50,000-channel synthetic preview. It is not a web-hosted production player and does not expose credentials or native operations.

## Application flow

Create a local vault passphrase, add an authorized HTTP(S) M3U source, and optionally configure ordered XMLTV feeds and authorized CDN hosts. Browse countries, groups and favorites; open the guide or play a channel. Background refresh runs while the native application is open and unlocked. No sample subscription, cloud account or published channel database is bundled.

The channel grid and guide virtualize visible rows. D-pad navigation tracks logical channel/programme positions rather than scanning the whole document. Android playback uses a native Media3 surface; desktop playback lazily loads HLS or DASH support. Stream URLs and provider headers stay out of application routes.

## Workspace

| Location | Responsibility |
| --- | --- |
| `src/` | Ionic UI, routes, components, bounded state, focus and playback presentation |
| `src-tauri/src/` | Tauri commands and application lifecycle |
| `src-tauri/crates/iptv-core/` | Streaming parsers, SQLite generations, vault, network policy and media relay |
| `src-tauri/plugins/player/` | Android Media3 bridge and native TV player |
| `scripts/`, `Makefile` | Explicit dependency/bootstrap, platform configuration and safe build/cleanup tasks |
| `tests/` | Pure frontend tests, SQL invariants, fixtures and browser integration specifications |
| `docs/` | Architecture, security, migration, compatibility and verification status |

## Common commands

```sh
make build                  # Current-host desktop build
make android-init           # Generate/configure the Android host once
make dev-android            # Connected Android development device
make build-android          # ARMv7 and ARM64 Android packages
make build-macos-universal  # Both Mac architectures, on macOS
make test                   # Dependency-free frontend algorithm tests
make test-sql               # SQLite invariant tests, Python 3 required
make test-e2e               # Requires installed Playwright browser
make clean                  # Known generated outputs only
```

No command deploys, publishes, creates signing keys or erases application user data. `clean` retains source, dependency locks, credentials and native source configuration.

## Important compatibility boundary

This project follows the requested latest-stable dependency direction, including Tailwind 4. Older Fire OS WebViews may not meet that browser floor. **Fire OS 6/7/8 and 60 fps are acceptance targets, not verified support claims.** ARMv7 packaging alone does not establish compatibility.

[Architecture](docs/ARCHITECTURE.md) · [Building](docs/BUILDING.md) · [Security](docs/SECURITY.md) · [Compatibility and gaps](docs/COMPATIBILITY.md) · [Verification](docs/VERIFICATION.md) · [Migration](docs/MIGRATION.md)

This private source delivery is `UNLICENSED`. Original supplied artwork and flags retain their existing rights; redistribution rights have not been established. See `NOTICE.md` and `public/ASSET_PROVENANCE.txt`.
