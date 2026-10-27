---
status: accepted
---

# Tools run without a sandbox only when the chief says so

Windows has no sandbox yet. A Windows server still refuses to start,
unless the chief starts it in insecure mode with `--insecure`. Then
agents' commands and hand tools run with the reach of the account
running the server. Linux and macOS have no insecure mode: they always
run in bubblewrap or Seatbelt, and refuse the flag. The built-in
file tools, the house store's checks and the network gate stay as they
are, and the window warns for as long as the server runs.

The flag is typed at every start and never saved, so insecure mode is
a choice the chief makes each time, not a default anyone inherits.

## Considered options

- No Windows server: the window only, as a remote client. Rejected:
  chiefs on Windows could not run Hamlet on their own machine.
- A Windows sandbox before any Windows server. Rejected for now: the
  only candidate that meets the policy, Microsoft's MXC process
  containers, needs recent Windows 11 builds and a prototype on a real
  machine first.
- Insecure mode as a setting saved in the home. Rejected: it would
  quietly become the default for every later start.
- Insecure mode on every platform. Rejected: it would let Linux test
  the unconfined path and serve hosts where bubblewrap cannot run, but
  Linux and macOS have a sandbox and should never run without it. So
  insecure mode is tested only on Windows.

## Consequences

- C18's "a server without a sandbox does not start" gains its one
  exception, a Windows server in insecure mode. C19's network switch is not enforced in insecure mode and
  decides only what waits for a chief.
- ADR 0003 still holds: tools reach files and processes only through
  the server. Insecure mode is the one place they run with no
  confinement, and the window, the banner and the prompt say so.
- A command in insecure mode can read the chief's credentials and the
  other houses, since the account running the server can.
- A Windows sandbox later is one more sandbox kind beside bubblewrap
  and Seatbelt. It changes none of this.
