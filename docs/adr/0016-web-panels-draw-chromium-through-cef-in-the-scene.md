---
status: accepted
---

# Web panels draw Chromium, through CEF, inside the window's scene

A Web panel shows a web page: an agent's HTML, a dev server on
`localhost`, or any site. It is a full engine, Chromium through CEF,
drawn offscreen into a texture the window paints like any other panel,
so the dock clips it and a Toast covers it. It is built on our fork of
`bevy_cef`, `voxell-tech/bevy_cef`, pinned to a commit on its `hamlet`
branch. It is macOS first, behind `hamlet_app`'s `web` feature, so the
Linux and Windows builds do not carry CEF. The research behind this is
in [`docs/proposals/research/embedding-a-web-engine.md`](../proposals/research/embedding-a-web-engine.md).

## Considered options

- The OS's own web view (`wry`, WKWebView) as a native child window.
  Rejected: it sits over the window, so the dock cannot clip it and
  nothing draws over it.
- Servo, Blitz or Ultralight. Rejected: none runs the sites people build
  today. Servo is partial and draws through OpenGL, Blitz has no
  JavaScript engine of its own, and Ultralight has no WebGL or video.
- A headless Chrome streaming frames over the DevTools protocol.
  Rejected for now: every frame is compressed and decoded, and input
  has to be faked over the protocol. It stays the fallback.
- `bevy_cef` from crates.io. Rejected: it has one maintainer and runs
  behind CEF's releases, and a web panel on the open internet needs
  Chromium's security fixes when we choose. The fork is ours to bump;
  fixes that are not ours alone go back upstream.
- Our own layer on `cef-rs`, as `godot-cef` is. Kept for if the fork
  grows too far from upstream.

## Consequences

- CEF is a framework of about 340 MB that the window does not ship:
  it reads it from `CEF_PATH`, else fetches it from CEF's builds into
  the user's caches the first time a Web panel opens, and starts it
  from the next launch. Its processes run `hamlet_web_helper`, which
  is built beside the window. The macOS app is not yet signed and
  notarised for others' Macs, which CEF's helpers need.
- Chromium's own sandbox is off: turning it on needs a helper linked
  with CEF's sandbox library, signed in an app bundle. Until then a
  page that breaks out of its renderer runs as the chief.
- Standard CEF has no H.264, HEVC or AAC, so most MP4 video does not
  play. WebM, VP9 and AV1 do.
- The page is on the machine of the window, so `localhost` and files
  are the window's. Reaching an agent's dev server or files on a
  remote server is for the server to forward.
- While a Web panel is in sight the window draws every frame, as the
  page does.
- Every Web panel shares one browser session, kept in the view folder.
