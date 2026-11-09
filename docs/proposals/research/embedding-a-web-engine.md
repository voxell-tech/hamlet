<!-- Researched 2026-11-09 from primary sources only: GitHub/crates.io APIs and repo files, vendor docs and pricing pages, Apple/Microsoft/Electron/CEF documentation. It extends and challenges the earlier note `~/Downloads/webview-research-full.md`. Nothing was built or benchmarked, so performance and size figures are marked (estimate) or (unverified). -->

# Embedding a full web engine as an in-scene dockable panel: is there anything more mature than `bevy_cef`?

**Requirement recap.** The panel must render a full web engine (open internet, `localhost` dev servers, modern JS/WebGL/video) into a texture that hamlet's dock UI clips and z-orders. A native child-window overlay is not acceptable. macOS comes first.

## 1. Short answer

**No. I found nothing more mature that meets all three requirements on macOS: in-scene compositing, a full web engine, and Bevy integration.**

- The shared stack under every serious option is **CEF** (Chromium Embedded Framework), either as the C++ library ([chromiumembedded/cef](https://github.com/chromiumembedded/cef): 4.9k stars, created 2017-04) or through the Rust bindings [tauri-apps/cef-rs](https://github.com/tauri-apps/cef-rs).
- `bevy_cef` is a young, single-maintainer wrapper over that stack. It is the only Bevy-native option that already composites into `bevy_ui` on Metal. Its maturity is low, but it sits on a mature core.
- The more mature *components* are the layers beneath it, not a competing Bevy crate:
  - CEF and `cef-rs`.
  - `godot-cef`, a Rust/`cef-rs` Godot extension using the same IOSurface-to-Metal technique, with CI, coverage and 2.x releases.
  - A commercial closed option for Unity (Vuplex).
- The best genuinely different alternative is an **Electron sidecar that exports IOSurfaces and shares them with the host over Syphon**. It is real but experimental and unproven at our scale (§4.2).

See §7 for the ranking.

## 2. How mature is `bevy_cef`, really?

### 2.1 Facts

| Signal | Finding | Source |
|---|---|---|
| Age and size | Created 2025-08-10. 59 stars, 9 forks, 1 watcher. | [GitHub API](https://api.github.com/repos/not-elm/bevy_cef) |
| Issues | **3 issues ever opened, all closed**: [#3](https://github.com/not-elm/bevy_cef/issues/3) Windows `Failed to create browser`, [#9](https://github.com/not-elm/bevy_cef/issues/9) Linux example fails, [#10](https://github.com/not-elm/bevy_cef/issues/10) "Release app example". None concern macOS. | [issue search](https://api.github.com/search/issues?q=repo:not-elm/bevy_cef+type:issue) |
| PRs | 71 PR numbers, almost all by the maintainer `not-elm`. External PRs are few: `alexdlm` (Linux support, [#49](https://github.com/not-elm/bevy_cef/pull/49)) and `rohitsangwan01` ([#71](https://github.com/not-elm/bevy_cef/pull/71)). | [PR list](https://github.com/not-elm/bevy_cef/pulls?q=is%3Apr) |
| Releases | v0.11.0 on 2026-06-21, v0.12.0 on 2026-07-08, v0.13.9 on 2026-09-21, v0.14.0 on 2026-10-15, v0.15.0 on 2026-10-21. | [releases](https://github.com/not-elm/bevy_cef/releases) |
| Crates.io reverse deps | `bevy_cef` has **0** reverse dependencies. `bevy_cef_core` has 3, all its own sibling crates. About 3.4k total downloads. | [crates.io reverse deps](https://crates.io/api/v1/crates/bevy_cef/reverse_dependencies) |
| GitHub users (code search) | Only three outside repos: [`not-elm/orzma`](https://github.com/not-elm/orzma) (same author, 55 stars), [`not-elm/desktop-homunculus`](https://github.com/not-elm/desktop-homunculus) (same author, 89 stars), and [`vmux-ai/vmux`](https://github.com/vmux-ai/vmux) (10 stars). | [code search](https://api.github.com/search/code?q=bevy_cef+language:TOML) |
| Forks | 8 forks with 0 stars each. The most recent are `slyedoc` (2026-11-06) and `rohitsangwan01` (2026-10-24). No notable divergent fork. | [forks](https://api.github.com/repos/not-elm/bevy_cef/forks) |

### 2.2 Downstream users are mostly behind or patched

- **desktop-homunculus** (Bevy 0.18) pins `bevy_cef = "0.8.1"`, far behind 0.15 ([Cargo.toml](https://github.com/not-elm/desktop-homunculus/blob/main/engine/Cargo.toml)).
- **vmux** (Bevy 0.19) uses `bevy_cef = "0.5.2"` through a **vendored local patch** (`[patch.crates-io] bevy_cef = { path = "patches/bevy_cef-0.5.2" }`) ([Cargo.toml](https://github.com/vmux-ai/vmux/blob/main/Cargo.toml)). It is the closest product analogue to hamlet: a Bevy-native "browser + IDE" for agents. It is early (v0.0.35 on 2026-09-30, macOS 13+ per its [README](https://github.com/vmux-ai/vmux)). Its choice to patch rather than track upstream is a signal worth weighing.
- **orzma** (Bevy 0.20, `bevy_cef = "0.14"`) is the only visible user at current versions ([Cargo.toml](https://github.com/not-elm/orzma/blob/main/Cargo.toml)). It is the author's own product, and its README says "orzma is not notarized" ([README](https://github.com/not-elm/orzma#installation)). So even the maintainer's own app has not solved macOS notarization.

### 2.3 CI covers less than it looks like

[`.github/workflows/ci.yml`](https://github.com/not-elm/bevy_cef/blob/main/.github/workflows/ci.yml):

- It runs only on `pull_request`.
- It uses a `windows`/`ubuntu`/`macos-latest` matrix.
- The macOS job runs `cargo test --workspace --no-default-features --tests` plus a Bevy-free render-process dependency check. It never launches CEF or a GPU.
- So the macOS IOSurface path is **exercised by hand, not CI** (consistent with the earlier note's reading of `CLAUDE.md`).

### 2.4 Engine freshness (security)

- `bevy_cef` 0.15 pins CEF 154.5.0. Current CEF stable on macOS arm64 is **156.0.26 (Chromium 156.0.8078.88)** ([cef-builds index](https://cef-builds.spotifycdn.com/index.json)). `cef-rs` published 156.4.0 on 2026-11-08 ([releases](https://github.com/tauri-apps/cef-rs/releases)).
- The author has tracked Bevy bumps within about a week, and CEF in monthly-ish jumps (PRs [#63](https://github.com/not-elm/bevy_cef/pull/63), [#64](https://github.com/not-elm/bevy_cef/pull/64), [#67](https://github.com/not-elm/bevy_cef/pull/67)). Being roughly two Chromium majors behind is normal between bumps, but it is a **security-patch-latency risk for open-internet browsing** (my inference).
- Mitigation is cheap if we own the dependency: bump the `cef` pin ourselves.

### 2.5 Verdict on maturity

The code quality evidence is good: a 71-PR history, a render-process-without-Bevy refactor, secure-by-default CEF flags ([#61](https://github.com/not-elm/bevy_cef/pull/61)), and a GPU path for macOS. The **adoption and operational maturity evidence is thin**: no independent production users at the current version, no macOS CI run, no notarization story, and a bus factor of 1. Treat it as a **good reference implementation and a fork candidate**, not a vendored black box.

## 3. The CEF layer beneath it (more mature than `bevy_cef`)

### 3.1 CEF itself

- Offscreen rendering with GPU shared textures is a documented CEF feature. `OnAcceleratedPaint` hands you an IOSurface on macOS ([cef_render_handler.h](https://github.com/chromiumembedded/cef/blob/master/include/cef_render_handler.h)).
- The CEF docs now include a **code-signing and notarization guide**, which closes the earlier note's entitlements gap ([docs/code_signing.md](https://github.com/chromiumembedded/cef/blob/master/docs/code_signing.md)).
  - Enable the Hardened Runtime.
  - Start from Chromium's per-helper entitlement plists. As of October 2026, the Renderer and GPU helpers need `com.apple.security.cs.allow-jit`.
  - Sign inside-out, and avoid `--deep`.
  - Do not add `allow-unsigned-executable-memory`, `disable-library-validation` and similar entitlements indiscriminately.
- `bevy_cef_bundle_app` signs ad-hoc by default and verifies with `codesign --verify --deep --strict` (per the earlier note), and I found no entitlements or notarization handling in it. The previous note found the same. The CEF guide above is the checklist to implement.
- On macOS, the CEF sandbox now requires distributing `libcef_sandbox.dylib` and loading it dynamically (M138+) ([sandbox_setup.md](https://github.com/chromiumembedded/cef/blob/master/docs/sandbox_setup.md)). I did not check how `bevy_cef` handles this beyond the earlier note's `SandboxMode` finding.

### 3.2 `tauri-apps/cef-rs`: more mature, and relevant

- Created 2025-01-10, with 490 stars, 79 forks and a push on 2026-11-08. It has about 370k downloads, with 16 crates.io reverse dependencies ([repo](https://github.com/tauri-apps/cef-rs), [crates.io](https://crates.io/crates/cef)).
- It is Tauri-org owned. Tauri's own CEF runtime lives on the `feat/cef` branch of `tauri-apps/tauri`, **not on `dev`** (`crates/tauri-runtime-cef` is absent on `dev`, present on `feat/cef`) ([branches](https://github.com/tauri-apps/tauri/branches)).
  - The same org merged "feat(cef): add essential OSR support" on 2026-10-14, and says it "works on macOS" ([tauri#16225](https://github.com/tauri-apps/tauri/pull/16225)).
  - That PR says IME, keyboard and events "should be handled outside tauri", so Tauri is not building the in-scene UX for us.
  - Open bugs in the Tauri CEF runtime include macOS 12.7 errors ([#16481](https://github.com/tauri-apps/tauri/issues/16481)).
- **New and important:** `cef-rs` has its own `accelerated_osr` feature with `objc2-io-surface` and `objc2-metal` dependencies ([cef/Cargo.toml](https://github.com/tauri-apps/cef-rs/blob/main/cef/Cargo.toml)), and an `osr_texture_import/` module containing `iosurface.rs`, `d3d11.rs`, `dmabuf.rs` and `vulkan.rs` ([tree](https://github.com/tauri-apps/cef-rs/tree/main/cef/src/osr_texture_import)).
  - Upstream therefore owns the platform handle-to-texture import. A hamlet-owned integration could build on it instead of copying `bevy_cef`'s importer (I have not read how it maps to wgpu, so this is unverified).
  - `godot-cef` uses exactly this: `cef = { version = "156.2.0", features = ["sandbox", "accelerated_osr"] }` ([Cargo.toml](https://github.com/dsh0416/godot-cef/blob/main/Cargo.toml)).
- Older competing Rust CEF crates are stale: `wef` (longbridge, 45 stars, last push 2026-01-01; crate 0.6.0 on 2025-06-13) ([crates.io](https://crates.io/crates/wef)).

### 3.3 JetBrains JCEF (precedent)

[JetBrains/jcef](https://github.com/JetBrains/jcef) (376 stars, pushed 2026-10-30) is a CEF fork maintained by an IDE vendor. It supports offscreen rendering and has JetBrains-scale production use. I did not verify which JetBrains features use OSR mode, so treat it as a maturity signal for CEF-in-a-native-host, not a reusable component for Rust.

## 4. Other approaches, with precedent and maturity

### 4.1 Game engines and app frameworks (precedent, not drop-in components)

| Engine / option | What it is | Maturity signals | In-scene texture | Full web | macOS | Cost | Evidence |
|---|---|---|---|---|---|---|---|
| **Unreal Web Browser widget** | First-party `WebBrowserWidget` plugin and `WebBrowser` module. `IWebBrowserWindow::GetTexture` returns an `FSlateShaderResource` (a texture representation for Slate). | Shipped by Epic; docs are in UE 5.8. I did not confirm from the fetched pages that it is CEF-based. Epic is widely known to use CEF, but treat that as unverified. | Yes (Slate texture) | Chromium-class (CEF presumed, unverified) | Presumed (unverified) | Engine licence | [UWebBrowser](https://dev.epicgames.com/documentation/en-us/unreal-engine/API/Plugins/WebBrowserWidget/UWebBrowser), [IWebBrowserWindow](https://dev.epicgames.com/documentation/en-us/unreal-engine/API/Runtime/WebBrowser/IWebBrowserWindow) |
| **Godot: `dsh0416/godot-cef`** | Rust GDExtension on `cef-rs`. `CefTexture` node with GPU "accelerated OSR". | Created 2025-12-31. 246 stars, 28 forks. Releases v2.1.1 to v2.1.3 in Oct–Nov 2026. 52 issues, 42 closed. Has build/test/Codecov CI. MIT. | Yes. Metal supported on macOS (Vulkan on macOS ❌, [#4](https://github.com/dsh0416/godot-cef/issues/4)). Windows/Linux GPU path uses hooking, x86_64 only. | Yes (Chromium 156). README shows WebGPU and WebGL demos. | Yes, universal framework | Free (MIT) | [README](https://github.com/dsh0416/godot-cef#platform-support), [releases](https://github.com/dsh0416/godot-cef/releases) |
| **Godot: `Lecrapouille/gdcef`** | C++ GDExtension for CEF. | Created 2022-05-30. 446 stars. v0.21.0-godot4 on 2026-10-29. Its README says macOS support needs contributors. | Yes (Linux/Windows) | Yes | **Effectively no** | Free (MIT) | [README](https://github.com/Lecrapouille/gdcef#readme), [releases](https://github.com/Lecrapouille/gdcef/releases) |
| **Godot: `doceazedo/godot_wry`** | Wraps the OS webview (`wry`). | 532 stars. | **No** (native overlay) | OS engine | Yes | Free | [repo](https://github.com/doceazedo/godot_wry) |
| **Unity: Vuplex 3D WebView (Windows and macOS)** | Closed-source commercial plugin. Renders to a `Texture2D`. | "Trusted by thousands", 240+ reviews. **Chromium v137 currently**, with an optional WKWebView mode on macOS. Native source not included. | Yes. Metal on macOS, D3D11/12 on Windows. | Yes (Chromium) | Yes (x64 + arm64) | **$159.99 per platform package.** Chromium adds 360 MB (reducible to 250 MB). | [store](https://store.vuplex.com/webview/windows-mac), [overview](https://developer.vuplex.com/webview/overview), [StandaloneWebView](https://developer.vuplex.com/webview/StandaloneWebView) |
| **O3DE** | No web or CEF gem in the engine's `Gems/` directory (checked by directory listing). | n/a | n/a | n/a | n/a | n/a | [o3de Gems](https://github.com/o3de/o3de/tree/development/Gems) |
| **Qt WebEngine** | Chromium-based, composites through the **Qt Quick scene graph on Qt's RHI** (including Metal). | Mature, commercial-grade, ships with Qt. | Yes, but only inside Qt's renderer. It is not extractable as a texture for wgpu from public docs I found. | Yes (Chromium) | Yes | LGPL/GPL/commercial (licence terms not verified here) | [overview](https://doc.qt.io/qt-6/qtwebengine-overview.html) |

Takeaways:
- The "CEF → engine texture" pattern is the industry norm (Unity via Vuplex, Godot via `godot-cef`, Unreal's Web Browser). That validates `bevy_cef`'s approach.
- The Rust-based `godot-cef` is the **most comparable maintained reference**: same `cef-rs`, same IOSurface/Metal handoff, with CI and tests.
- **Vuplex ships an older Chromium (137)** than the current stable (156). Even a mature commercial product lags, which supports the "plan to own CEF bumps" conclusion. Vuplex is Unity-only and unusable for Bevy.

### 4.2 Electron sidecar handing IOSurfaces to our process

Can a sidecar Electron process hand IOSurfaces to a Bevy process? **Plausibly yes, on macOS, and a third-party precedent exists. It is experimental.**

- Electron offscreen rendering can emit a **GPU shared-texture** instead of a bitmap, via `webPreferences.offscreen.useSharedTexture` (marked *Experimental*) ([offscreen-rendering.md](https://github.com/electron/electron/blob/main/docs/tutorial/offscreen-rendering.md), [web-preferences.md](https://github.com/electron/electron/blob/main/docs/api/structures/web-preferences.md)). The tutorial says frames are copied directly in GPU textures with no CPU-GPU copy, but that it "requires a native node module to work with your own code".
- The handle struct exposes an `ioSurface` Buffer on macOS, and explicitly warns that "this IOSurface is local to current process (not global)" ([shared-texture-handle.md](https://github.com/electron/electron/blob/main/docs/api/structures/shared-texture-handle.md)).
- Electron's own design doc explains that crossing processes on macOS means a `mach_port` through IPC, "making it significantly more complex than Windows" ([shared_texture README](https://github.com/electron/electron/blob/main/shell/common/api/shared_texture/README.md)).
- Apple's **public** APIs for this are [`IOSurfaceCreateMachPort`](https://developer.apple.com/documentation/iosurface/iosurfacecreatemachport(_:)), meant "to atomically or securely pass an IOSurface to another task without making the surface global", and [`IOSurfaceCreateXPCObject`](https://developer.apple.com/documentation/iosurface/iosurfacecreatexpcobject(_:)). `kIOSurfaceIsGlobal` is [deprecated](https://developer.apple.com/documentation/iosurface/kiosurfaceisglobal) ("Global surfaces are insecure").
- **Precedent:** [`kyeshmz/electron-syphon`](https://github.com/kyeshmz/electron-syphon) (6 stars, created 2026-06-03, tiny) takes Electron's OSR IOSurface, wraps it as a Metal texture and publishes it to **[Syphon](https://github.com/Syphon/Syphon-Framework)** with no CPU readback.
  - Its README claims about 0.13 ms per frame for the publish at 1080p on Apple Silicon (self-reported, not independently verified) and requires Electron 33+.
  - Syphon is an established macOS inter-process IOSurface-sharing framework (531 stars, `SyphonMetalServer` and `SyphonMetalClient` in the repo). Its last tagged release is v5 from 2019-03-02, although the repo was pushed 2026-09-21 ([releases](https://github.com/Syphon/Syphon-Framework/releases)). Its licence is shown as `NOASSERTION` by GitHub; check before depending on it.
- Current Electron is v45.2.3 (2026-11-04) ([releases](https://github.com/electron/electron/releases)), so the engine stays current for free.

**Fit for hamlet (my analysis):** a hamlet-side `SyphonMetalClient` yields a Metal texture, which `bevy_cef`-style `create_texture_from_hal` code could wrap into Bevy. Input would go the other way via Electron IPC (`webContents.sendInputEvent`) or CDP.
- Pros: no CEF bundling or signing inside hamlet's own app bundle. A full, current Chromium. Strong process isolation. Free agent control through CDP.
- Cons: ships a second app (Electron) with its own signing and notarization, experimental API, Syphon from Rust needs ObjC FFI, no IME/cursor/popup/drag story beyond what we synthesize, and latency is unmeasured.
- Linux and Windows would need different mechanisms. It is Apple-first only.

### 4.3 Commercial embeddable engines

| Option | Full web engine? | In-scene texture | macOS | Cost | Notes and evidence |
|---|---|---|---|---|---|
| **Coherent Gameface / Prysm** | **No.** It is a game-UI HTML5/CSS/JS renderer ("cohtml") that lets the host engine retain control over rendering, memory, resource loading and threading. It has its own supported-features lists and a "Differences to traditional browsers" page. Networking is the host's job ("Responding to resource requests asynchronously"), and WebSockets and video are separate optional features. | Yes, designed for it | Mac OS X 10.13+ listed as a runtime target | Quote-only. Licence is per title **and per platform**, with source access as an add-on. | [product](https://coherent-labs.com/products/coherent-gameface/), [pricing](https://coherent-labs.com/pricing/), [docs](https://docs.coherent-labs.com/cpp-gameface/), [requirements](https://docs.coherent-labs.com/cpp-gameface/what_is_gfp/requirements/). Not suitable for arbitrary sites or dev servers (my judgement from its scope). |
| **Ultralight** (updated check) | **No** (per the earlier note). | Yes (custom GPUDriver) | Platform listed; `ul-next` Rust crate 0.5.4 only tested on Win/Linux | Pricing unchanged: Free (under $200K turnover and funding), Indie $395 one-time per title, Indie Pro $4,500, then custom ([pricing](https://ultralig.ht/pricing)). | The upstream repo's last push is **2024-04-22** (5.0k stars) ([repo](https://github.com/ultralight-ux/Ultralight)). The WebGL/WebRTC/video/IME gap issue [#178](https://github.com/ultralight-ux/Ultralight/issues/178) is still open, last updated 2024-03. It looks dormant. |
| **Sciter** | **No.** It is a HTML/CSS UI engine with its own script. I did not research it further, so this is unverified. | | | | Mention only for completeness. |
| **Awesomium** | Dead (not researched, unverified). | | | | |
| **Vuplex** | Yes (Chromium 137) but Unity-only. | | | $159.99 | §4.1. |

### 4.4 Native OS engines composited offscreen

#### macOS WKWebView (public vs private)

- **Public:** [`WKWebView.takeSnapshot(with:completionHandler:)`](https://developer.apple.com/documentation/webkit/wkwebview/takesnapshot(with:completionhandler:)) returns a platform-native image asynchronously (macOS 10.13+). It is a one-shot CPU-image API, not a frame stream.
  - I did not measure it, but a per-frame snapshot loop is not a plausible interactive-rate path (my inference, **unverified**).
- **Public, via a real window:** [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit/sccontentfilter) can filter to a single window ([`init(desktopIndependentWindow:)`](https://developer.apple.com/documentation/screencapturekit/sccontentfilter/init(desktopindependentwindow:))). It outputs BGRA frames up to 60 fps, with a configurable queue ([sample](https://developer.apple.com/documentation/screencapturekit/capturing-screen-content-in-macos)), and Apple's own sample casts the frame's surface reference to an `IOSurface`.
  - But it needs a real, composited `NSWindow` hosting the `WKWebView`, plus the user's **Screen Recording permission** (the sample states the system prompts for it and requires an app restart after granting). That reintroduces a native window. It is a workaround rather than a true offscreen texture, and a permission prompt is poor UX for a dev tool.
- **Private/SPI:** WebKit renders the page in the WebContent process and shows it in the UI process via `CALayerHost` layers bound to a remote `contextId`. The declaration sits in WebKit's SPI header ([QuartzCoreSPI.h](https://github.com/WebKit/WebKit/blob/main/Source/WebCore/PAL/pal/spi/cocoa/QuartzCoreSPI.h)). Chromium uses the same mechanism ([remote_layer_api.h](https://github.com/chromium/chromium/blob/main/ui/base/cocoa/remote_layer_api.h)).
  - These are private, unstable and App Store-hostile. They display remote content in the compositor, and I found no documented route to read that content into a Metal texture from them.
  - [`CARenderer`](https://developer.apple.com/documentation/quartzcore/carenderer) is public, but it renders a *local* layer tree, and Apple's docs describe it for rendering into an OpenGL context (macOS 10.5+, so legacy-flavoured). Whether it can render a `WKWebView`'s remote layer content is **unverified**. Reports suggest it does not, but I did not check a primary source.
- Vuplex advertises an optional WKWebView-based macOS plugin with a `SetTargetFrameRate` API and a `GetNativeWebView` handle ([MacWebKitWebView](https://developer.vuplex.com/webview/MacWebKitWebView)), so a commercial vendor does ship a WKWebView-to-texture path. Its mechanism is not documented in what I could fetch. Worth a trial of the Vuplex demo to learn what it does, if WKWebView ever matters.
- **Bottom line:** no *public, supported, interactive-rate WKWebView-to-Metal-texture API* exists that I could find. It is not worth pursuing for this requirement.

#### Windows WebView2

- WebView2 has three hosting modes. **Visual hosting** outputs to a `DirectComposition` or `Windows.UI.Composition` Visual, and the host forwards input itself ([Windowed vs. Visual hosting](https://github.com/MicrosoftDocs/edge-developer/blob/main/microsoft-edge/webview2/concepts/windowed-vs-visual-hosting.md)).
- That gives compositor-level embedding (Windows-only). I found **no texture-export API** in that doc, so wgpu/Bevy would need a DirectComposition interop layer that is not a texture handoff. This is out of scope for macOS-first. CEF's D3D11 shared-texture path is the better Windows route.

#### WPE WebKit

- WPE is Linux-oriented: since 2.54 its `WPEPlatform` bundles Wayland, DRM/KMS and **headless** backends ([get-wpe](https://wpewebkit.org/about/get-wpe.html)). It has no macOS port that I found, so it is not an option for this brief. At most it is a Linux-only fallback, with unverified texture export.

### 4.5 Servo, Blitz, Ladybird (update to the earlier note)

- **Servo:** `RenderingContext` is still GL/surfman-based, with `read_to_image` and `create_texture`, and no wgpu hook (I re-checked [rendering_context.rs](https://github.com/servo/servo/blob/main/components/shared/paint/rendering_context.rs)). The latest GitHub release is v0.7.0 (2026-10-05), and the repo has one embedding example, `winit_minimal.rs` ([examples](https://github.com/servo/servo/tree/main/components/servo/examples)). I found no Bevy or wgpu embedding crate on crates.io or GitHub ("bevy servo", "servo wgpu" searches). The earlier note's conclusion stands: promising, not ready for arbitrary sites.
- **Ladybird:** pre-alpha ("only suitable for use by developers"), with a multi-process UI/WebContent architecture, not an embeddable library ([README](https://github.com/LadybirdBrowser/ladybird#readme)). 67k stars, but out.
- **`bevy_chromite`:** unchanged since the earlier note. Still only 0.0.1 (2026-10-23), and its repo `JosepBove/chromite` returns 404 ([crates.io](https://crates.io/crates/bevy_chromite)). Description claims "Chromium web views in Bevy: on meshes, in UI, fully interactive". **Not usable.**

### 4.6 CDP/screencast fallback (brief)

- **Playwright** (99.6k stars, v1.65.0 on 2026-11-05) now has a first-class `page.screencast.start({ onFrame })` API (since v1.59) that delivers **JPEG** frames ([class-screencast.md](https://github.com/microsoft/playwright/blob/main/docs/src/api/class-screencast.md)). **Puppeteer** has 95.8k stars ([repo](https://github.com/puppeteer/puppeteer)). Both are Node, so they are only usable from a Rust host as an external process.
- The Rust CDP clients are `chromiumoxide` 0.9.1 (last release 2026-02-25, but about 2.3M recent downloads) and `headless_chrome` 1.0.22 (2026-06-11) ([crates.io](https://crates.io/crates/chromiumoxide), [crates.io](https://crates.io/crates/headless_chrome)).
- It is the most mature *software* path and gives agent control "for free". The earlier note's caveats hold: JPEG artifacts, base64 over WebSocket per frame, and synthesized input. It is a fallback, not the primary.

## 5. Mature-vs-young at a glance

| Option | Age / scale | Maintainers | Release cadence | Notable users |
|---|---|---|---|---|
| `bevy_cef` | 2025-08, 59★ | 1 (`not-elm`) | ~monthly | vmux (patched 0.5.2), orzma, homunculus (0.8.1) |
| `cef-rs` | 2025-01, 490★, ~370k dl | Tauri org plus `wravery`, `csmoe`, others | Tracks every CEF stable (156.4.0 on 2026-11-08) | `godot-cef`, `bevy_cef`, Tauri CEF runtime branch |
| `godot-cef` | 2025-12, 246★ | 1 primary (`dsh0416`) | v2.1.x weekly | Godot community |
| Vuplex | long-lived commercial | Vendor | Chromium 137 | "Thousands of developers" (vendor claim) |
| Electron OSR (shared texture) | Electron is mature. The shared-texture API is experimental. | OpenJS/Electron team | v45.2.3 on 2026-11-04 | Syphon bridge: 6★ |
| Servo | 38k★ | Many contributors | Monthly | embedding is still unproven for us |

## 6. Comparison table

| Option | Maturity (see §5) | True in-scene composite | Full web | macOS | Licence / cost | Rust/Bevy effort (estimate) |
|---|---|---|---|---|---|---|
| **`bevy_cef` 0.15** | Young, 1 maintainer, ~3 outside users | **Yes** (`MaterialNode<WebviewUiMaterial>`, IOSurface→Metal blit; per the earlier note) | **Yes** (Chromium 154) | Yes, Metal only. Packaging and notarization unsolved. | Apache-2.0 or MIT | **Low–medium**: already Bevy 0.20. Focus, keyboard, loop and packaging bridging. |
| **Own integration on `cef-rs` (`accelerated_osr`)** | `cef-rs` is mature. Our code would be new. | Yes (we build it) | Yes (Chromium 156) | Yes | MIT/Apache | **High** (several weeks, estimate): re-implement `bevy_cef`, though upstream now supplies the IOSurface import. |
| **Electron sidecar + Syphon** | Electron mature. Shared-texture API experimental. Bridge is 6★. | Yes (Syphon Metal texture, then wrapped in wgpu) | Yes (current Chromium) | Yes (macOS only) | Electron MIT. Syphon licence unverified. | **Medium–high**: ObjC FFI, IPC and input forwarding, second app to sign. |
| **CDP/Playwright screencast** | Very mature tooling | Yes (JPEG to `Image`) | Yes | Yes | Apache-2.0/MIT | **Medium** (latency/input limits) |
| **WKWebView (snapshot / SCK / SPI)** | Apple-supported, but wrong API | **No** (public API) / needs a real window | Yes (WebKit) | Yes | Free | n/a, not recommended |
| **Servo `servo` 0.8** | Active, partial web compat | Possible via `read_to_image` (CPU) or GL interop (unproven) | Partial | Yes | MPL-2.0 | **High** |
| **Blitz** | Beta | Yes (wgpu texture example) | **No** (Boa JS experimental) | Yes | MIT/Apache | n/a (wrong scope) |
| **Ultralight** | Repo dormant since 2024-04 | Yes | **No** | Listed | Free tier / $395+ | n/a |
| **Coherent Gameface / Prysm** | Mature in AAA | Yes | **No** (game-UI subset) | Listed | Quote, per title and platform | n/a |
| **Vuplex (Unity)** | Mature commercial | Yes | Yes (Chromium 137) | Yes | $159.99 | **Not usable** (Unity only) |
| **`wry` overlay / `bevy_webview_wry`** | Mature `wry`, archived Bevy glue | **No** (native child window) | Yes (OS engine) | Yes | MIT/Apache | rejected by requirement |
| **`bevy_chromite`** | Name reservation | claimed | claimed | claimed | n/a | not usable |

## 7. Ranked recommendation

**Is there something more mature than `bevy_cef` that meets the in-scene compositing, full web and macOS requirements?** No single product does. The only options that do meet all three are in the same CEF family, or are an experimental Electron bridge. Of those, `bevy_cef` is the only one that is already Bevy-native and already on Bevy 0.20.

1. **Adopt `bevy_cef` as the baseline, but treat it as a fork-ready dependency, not a black box.** Pin it, then own the `cef` version bump (it is currently two stable majors behind).
   - Reduce lock-in by reading how its importer relates to `cef-rs`'s `osr_texture_import` (§3.2).
   - Mirror vmux's pattern of keeping a patchable copy in the workspace if upstream falls behind.
   - Spike order: (a) notarized `.app` using the CEF [code-signing checklist](https://github.com/chromiumembedded/cef/blob/master/docs/code_signing.md), since even `orzma` ships un-notarized; (b) the reactive-loop, input-focus and dock-clipping questions from the earlier note.
2. **Plan B: a hamlet-owned thin layer on `cef-rs` (`accelerated_osr`)**, copying `godot-cef`'s structure as a proven reference (tests, CI, 2.x releases). Pick this if `bevy_cef` development stalls or its design blocks hamlet's focus model. It is more work, but depends only on the best-maintained component.
3. **Plan C: CDP/Playwright screencast** for a cheap, current-Chromium, agent-controllable preview path. It is the most mature software path but has lower fidelity. It is the right first MVP if the packaging spike fails.
4. **Plan D (research spike only): Electron sidecar + Syphon.** Avoids bundling CEF in the main app bundle and keeps Chromium current, but relies on an experimental API and a 6-star bridge. Only try it if CEF signing or maintenance proves intractable. Measure end-to-end latency first.
5. **Not recommended:** WKWebView routes (no supported texture API), Servo and Ladybird (not ready), Blitz and Ultralight and Gameface (not full web), Vuplex (Unity only), `wry` overlays (rejected by requirement).

## 8. Gaps and unverified items

- Nothing here was built or benchmarked. Latency, frame rate, memory, and Electron/Syphon throughput are unmeasured. The Syphon "0.13 ms" figure is the project's own claim.
- `bevy_cef`'s importer was not re-read this session. The overlap with `cef-rs`'s `osr_texture_import` is inferred from file names and Cargo features.
- Unreal's Web Browser being CEF-based, and its macOS support, were **not confirmed** in the pages I could fetch.
- The Qt WebEngine licence terms, Syphon's licence, and WPE's texture-export capability were not verified.
- Whether `CARenderer` can capture a `WKWebView`'s remote layer content, and how Vuplex's WKWebView plugin renders to a texture, are unverified.
- Sciter and Awesomium were not researched (§4.3).
- Which JetBrains products use JCEF's offscreen mode was not verified.
- The claim that the Chromium version is "two stable majors behind" compares the `bevy_cef` pin (154.x) with the cef-builds stable list (156.x). How long Google backports security fixes to older majors was not checked.
