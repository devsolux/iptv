# Security model

This is an implementation design and source review, not a completed security audit. See the unexecuted release gates in [VERIFICATION.md](VERIFICATION.md).

## Secrets and local data

The user chooses a vault passphrase of at least twelve characters. Argon2id derives the key using a random salt (32 MiB, three passes, one lane). XChaCha20-Poly1305 encrypts source configuration and stream secrets with context-specific authenticated additional data. Rust key buffers are zeroized on release. The passphrase is not stored and there is no password recovery.

Channel names, grouping, favorites and programme metadata are **not full-database encrypted**. Source URLs, stream URLs/headers and sensitive associated metadata are encrypted. A locked application cannot refresh or resolve new playback sessions. JavaScript form values are cleared after submission, but this is not a guarantee of cryptographic memory erasure in a managed runtime. Process compromise and an attacker controlling the same user account are outside this vault boundary. Android backup is disabled for the generated host.

## Untrusted input and native authority

Playlist lines, XMLTV, media manifests, URLs, IPC arguments and returned DTOs are untrusted. Rust enforces sizes, intervals, source/job limits and query bounds; Zod checks the bridge payload shapes. SQL is parameterized and only the Rust database layer opens the application database. XML DTD/external resource resolution is rejected.

The main bundled window receives only core/default and the enumerated application commands. It receives no generic shell, SQL, filesystem or network plugin authority. CSP limits connections/media to app IPC and loopback resources, disallows frames and objects, and does not allow script eval. Stream credentials never enter routes or ordinary error messages. UI text is escaped rather than injected as HTML.

## Upstream network policy

HTTPS certificate verification remains enabled. Cleartext and private-network access require explicit per-source consent. Exact authorized source/CDN hosts constrain requests, including redirects. DNS resolutions are checked and pinned for the connection; loopback, metadata/link-local and transition-address bypasses are rejected. Only the explicitly supported provider Referer and User-Agent headers are carried through. User-info URLs, unsafe schemes and arbitrary header injection are not accepted.

iptv-org sources are aggregators whose streams live on arbitrary public hosts, so the exact host allowlist does not apply to them; every other control does: HTTPS unless cleartext is consented per source, no user-info URLs, DNS pinning, loopback/private/link-local/metadata/transition-address rejection, redirect re-checks, limits and deadlines. Private-network access and CDN host lists are rejected for these sources, their playlist and API bases must be plain HTTPS origins, and the metadata fetch behind the source form is confined to the API host. Dataset JSON is streamed under byte and item limits, unknown fields are ignored, and a malformed snapshot is rejected instead of activated.

Imports and relay reads use limits and deadlines. Cancellation is cooperative: a blocking system DNS call can only return when the operating system resolver finishes. Do not interpret cancellation as instantaneous forced interruption of every external system call.

## Local playback transport

The relay listens only on loopback and uses unguessable session/resource IDs. It does not expose an arbitrary external-URL endpoint. Resource and concurrency counts are bounded, and session cancellation stops associated streams. It has no public service, authentication bypass or DRM circumvention role. Do not expose its port through a reverse proxy or bind it to a public interface.

Android release network resources allow Java HTTP only to loopback; debug resources additionally allow the Vite host. The macOS plist requests local networking only, not arbitrary insecure remote transport. Rust authorizes any upstream HTTP separately. Verify actual platform behavior and merged native manifests before release; configuration alone is not a test of WebView/ATS enforcement.

## Reporting and release controls

Do not log complete source/stream URLs, provider credentials, passphrases or relay tokens. Keep signing secrets outside the repository. No deployment, publication, signing-key generation or production-state changes occur in the build workflow. Run a dependency audit and an adversarial redirect/DNS/manifest/IPC review on the resolved dependency graph before distributing binaries.
