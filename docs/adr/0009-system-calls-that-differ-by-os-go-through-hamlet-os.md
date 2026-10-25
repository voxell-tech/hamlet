---
status: accepted
---

# System calls that differ by OS go through `hamlet_os`

Every call whose form differs between Linux, macOS and Windows lives in
one leaf crate, `crates/hamlet_os`: the house store's and the tools'
file operations, and the start, limits and end of a command's
processes. Callers hold its concrete types and write no `cfg`. Each OS
implements a private trait in a private module, rustix on Unix and the
NT API on Windows, so no caller can reach a platform call. The crate
has no C code below it, so its Windows half is type-checked on Linux
without a C compiler, which the whole server cannot be.

## Considered options

- `cfg` twins inside `papers/tree.rs` and `sandbox/env.rs`. The
  smallest change, but platform code stays spread over three files and
  nothing checks the Windows half until the whole server does.
- A store trait inside `hamlet_server`, with one implementation per OS.
  A second seam beside `Files`, a type parameter or a trait object for
  every caller, implementations any module can import, and no
  cross-check without a C compiler.
- `cap-std` on Windows. It has no rename that refuses to replace, which
  the house store relies on, rebuilds paths for its changes, and needs
  the NT API beside it anyway.
- Paths through `std::fs` on Windows. A junction needs no privilege
  there, so an agent could redirect the server.

## Consequences

- The code convention's "system calls go through rustix" becomes
  "system calls go through `hamlet_os`". `rustix` leaves `hamlet_server`.
- The crate's two Windows modules join the convention's list of
  modules that declare unsafe code.
- The house store keeps its guarantees on every platform: opens
  relative to a held folder, no link followed, no hard-linked file, and
  renames that refuse to replace.
- A third platform is a third private module, checked against the same
  contract.
