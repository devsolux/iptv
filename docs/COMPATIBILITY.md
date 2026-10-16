# Compatibility, known gaps and device acceptance

## Latest packages versus old WebViews

The user explicitly requested current stable packages and removal of legacy compatibility infrastructure. This source uses Ionic 9 and Tailwind 4. Ionic's router integration supports Router 6; the application instead uses standalone React Router 8 and Ionic presentation components. It does not force incompatible router peer dependencies or ship two routers.

[Tailwind's official browser floor](https://tailwindcss.com/docs/upgrade-guide) is Chrome 111, Safari 16.4 and Firefox 128. No Tailwind 3 branch, old-browser polyfill pipeline or CSS compatibility claim is included. An Android API level does not tell us the installed Fire TV WebView version. **The requested latest stack and guaranteed support for every old Fire OS device cannot both be asserted without measurements.**

[Tauri documents Android 7 as its nominal minimum](https://v2.tauri.app/distribute/google-play/); that does not certify this application or every old device/WebView combination. The first device gate is launch + UI rendering + remote input + native playback, not a successful APK build.

## Measures for older Fire OS WebViews

Amazon publishes no Chromium version per Fire OS release, and the Silk browser build does not describe Amazon WebView, so the only reliable number is the `navigator.userAgent` reported by the WebView on the device itself. This source therefore avoids features that would exclude a whole WebView generation for little gain:

- The frontend build lowers syntax to Chromium 84 / Safari 14 (`vite.config.ts`). No polyfills are added, so the runtime must still provide ES2020 built-ins, `ResizeObserver` and `matchMedia`.
- Own styles avoid `:has()`, the `inset` shorthand, `backdrop-filter` and `Array.prototype.at`; flex `gap` (Chromium 84) remains the practical CSS floor. Tailwind 4 is imported only as a layered theme and no utility class is used in markup, so its Chromium 111 floor applies to nothing that is rendered.
- Remote behaviour: entering a page on a screen without hover focuses the first control instead of the search field, so the on-screen keyboard does not open by itself; Back first leaves a text field; the Menu key opens channel actions; the desktop player maps the rewind, fast-forward and stop media keys; the native Media3 dialog handles rewind, fast-forward and stop as well.
- Focus is drawn with the app's own rings (a strong ring on buttons and cards, an accent border on text fields and selects) rather than the WebView default.
- On screens without hover wider than 1100 px, pages and the player chrome keep a 3–4% margin for TV overscan.

Ionic 9 components keep their own browser requirements; those are not downleveled. None of this has been measured on Fire OS 6/7/8 hardware.

## Required physical test targets

| Target | Representative software/ABI | Delivery status |
| --- | --- | --- |
| Fire TV Stick 4K, first generation (2018) | Fire OS 6 / API 25 / ARMv7 app | Not tested; high WebView/runtime risk |
| Fire TV Stick Lite (2020) | Fire OS 7 / API 28 / ARMv7 app, 1 GB RAM | Not tested; primary low-memory target |
| Fire TV Stick 4K Max, second generation (2023) | Fire OS 8 / API 30 / ARMv7 app | Not tested |
| Other compatible Android devices | ARMv7 or ARM64 | Not tested |
| macOS | Configured minimum 13.3; Apple Silicon and Intel | Not compiled/tested |
| Windows | x64, MSVC/WebView2 | Not compiled/tested |
| Linux | x64, WebKitGTK/GStreamer | Not compiled/tested |

Device specifications: [Amazon's official Fire TV specification tables](https://developer.amazon.com/docs/device-specs/device-specifications-fire-tv-streaming-media-player.html). Products using Vega OS are outside this Android/Fire OS project.

Native Media3 uses the platform decoders; it does not guarantee every codec/profile/level. There is no universal hardware-acceleration flag that substitutes for playback profiling. Linux/browser engines additionally depend on installed system codecs and media APIs.

## Implemented playback scope

HLS, clear DASH and supported progressive HTTP(S) streams are the intended formats. HLS AES-128 and identity SAMPLE-AES resources can be relayed; DRM key systems (any `KEYFORMAT` other than `identity`, `SAMPLE-AES-CTR`) are rejected. RTMP, SRT and MMSH are unsupported. DASH rewriting chooses the first BaseURL and does not support external xlink, patches or DRM; HTTP `UTCTiming` sources are relayed when the source policy allows their host, `direct` sources are kept and other schemes are dropped, in which case dash.js uses the manifest's `Date` header. Native HLS audio/subtitle presentation depends on platform behavior. Desktop variant selection is implemented; the native full-screen TV dialog does not include an equivalent variant picker.

There is no guarantee that a URL that responds successfully is playable. Network, unsupported-protocol and decode failures must be checked against authorized deterministic test streams as well as representative providers.

## Remaining functional work

There is no local-file picker, source-configuration editor, manual EPG mapping editor, remote logo cache, conditional HTTP cache reuse, DRM, recording, transcoding or closed-application sync scheduler. Manual mappings have a core command but no UI. XMLTV requires UTF-8, second-precision timestamps and explicit programme stop times. No automatic import of old app credentials/data is attempted after the identifier change.

## Performance acceptance

The read-only preview generates 50,000 synthetic channels. Passing algorithm tests does not measure rendering performance. Before release, profile 10,000 and 50,000 channel imports, representative large guides and repeated channel switching on physical Lite-class hardware while playback and background import run together.

Record total process/native/player memory, peak import memory, mounted DOM counts, frame times, dropped frames, remote-key-to-focus latency, thermal behavior and resume/recovery. Check long sessions, interrupted downloads, no network, wrong credentials, oversized input, background/foreground cycles and back-button focus restoration. **60 fps is a target, not a measured result in this archive.**
