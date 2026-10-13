# Security model

This is an implementation design and source review, not a completed security audit. See the unexecuted release gates in [VERIFICATION.md](VERIFICATION.md).

## Secrets and local data

There is no passphrase. On first start Rust generates a random 32-byte profile key, stores it in a private (`0600`) file next to the database and loads it on every launch, so the application opens straight into the library. XChaCha20-Poly1305 seals source configuration, stream URLs/headers and sensitive metadata with context-specific authenticated additional data; key buffers are zeroized at exit. The seal protects against accidental disclosure (copied profiles, backups, logs), not against an attacker who already runs as the same user, because the key lives beside the data. A damaged key file is rejected rather than replaced, since replacing it would orphan every sealed record. Profiles created by the former passphrase vault cannot be opened without that passphrase; such a profile is reset once at startup (sources, favorites and overrides are removed, channel data with them) and the old vault file is deleted.

Channel names, grouping, favorites and programme metadata are **not full-database encrypted**. JavaScript form values are cleared after submission, but this is not a guarantee of cryptographic memory erasure in a managed runtime. Process compromise is outside this boundary. Android backup is disabled for the generated host.

## Untrusted input and native authority

Playlist lines, XMLTV, media manifests, URLs, IPC arguments and returned DTOs are untrusted. Rust enforces sizes, intervals, source/job limits and query bounds; Zod checks the bridge payload shapes. SQL is parameterized and only the Rust database layer opens the application database. XML DTD/external resource resolution is rejected.

The main bundled window receives only core/default and the enumerated application commands. It receives no generic shell, SQL, filesystem or network plugin authority. CSP limits connections/media to app IPC and loopback resources, disallows frames and objects, and does not allow script eval. Stream credentials never enter routes or ordinary error messages. UI text is escaped rather than injected as HTML.

## Upstream network policy

HTTPS certificate verification remains enabled. Cleartext and private-network access require explicit per-source consent. Exact authorized source/CDN hosts constrain requests, including redirects. DNS resolutions are checked and pinned for the connection; loopback, metadata/link-local and transition-address bypasses are rejected. Only the explicitly supported provider Referer and User-Agent headers are carried through. User-info URLs, unsafe schemes and arbitrary header injection are not accepted.

iptv-org sources are aggregators whose streams live on arbitrary public hosts, so the exact host allowlist does not apply to them; every other control does: HTTPS unless cleartext is consented per source, no user-info URLs, DNS pinning, loopback/private/link-local/metadata/transition-address rejection, redirect re-checks, limits and deadlines. Private-network access and CDN host lists are rejected for these sources, their playlist and API bases must be plain HTTPS origins, and the metadata fetch behind the source form is confined to the API host. Dataset JSON is streamed under byte and item limits, unknown fields are ignored, and a malformed snapshot is rejected instead of activated.

Every upstream request carries a browser-like default User-Agent unless the playlist supplies its own, speaks HTTP/1.1, and a playback session keeps one cookie jar so tokens set by a playlist response reach its segments. Certificates are verified against the operating-system trust store through rustls' platform verifier. A source may explicitly opt into unverified certificates; treat that like cleartext consent, acceptable for public streams with broken certificates and never for credentials.

Imports and relay reads use limits and deadlines. Cancellation is cooperative: a blocking system DNS call can only return when the operating system resolver finishes. Do not interpret cancellation as instantaneous forced interruption of every external system call.

## Local playback transport

The relay also serves channel artwork at an unguessable `/logo/{token}/{channel}` path for the UI's image elements. Artwork is fetched through the image policy (any public host over the source's permitted transports, never private addresses), limited to 1 MiB, accepted only when its bytes sniff as PNG, JPEG, GIF, WebP, AVIF or script-free SVG, cached under the private profile directory and served with `nosniff` and a `default-src 'none'` policy. The upstream image URL never reaches the WebView.

The relay listens only on loopback and uses unguessable session/resource IDs. It does not expose an arbitrary external-URL endpoint. Resource and concurrency counts are bounded, and session cancellation stops associated streams. It has no public service, authentication bypass or DRM circumvention role. Do not expose its port through a reverse proxy or bind it to a public interface.

Android release network resources allow Java HTTP only to loopback; debug resources additionally allow the Vite host. The macOS plist requests local networking only, not arbitrary insecure remote transport. Rust authorizes any upstream HTTP separately. Verify actual platform behavior and merged native manifests before release; configuration alone is not a test of WebView/ATS enforcement.

## Reporting and release controls

Do not log complete source/stream URLs, provider credentials, the profile key or relay tokens. Keep signing secrets outside the repository. No deployment, publication, signing-key generation or production-state changes occur in the build workflow. Run a dependency audit and an adversarial redirect/DNS/manifest/IPC review on the resolved dependency graph before distributing binaries.
