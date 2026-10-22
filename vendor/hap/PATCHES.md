# hap (vendored)

Vendored from https://github.com/ewilken/hap-rs at `acf45bd` (0.1.0-pre.15).

Patches:

- Replaced `get_if_addrs` with `if-addrs` 0.6; both link the native `ifaddrs`
  library, so the upstream crate does not resolve.
- Dropped the unused build-dependencies, workspace and examples.
- Silenced compiler warnings (`#![allow(warnings)]` in lib.rs); the code is
  older than the current toolchain's lints.
- Made `pointer` public so accessory handles returned by `add_accessory` can be named.
- `get_local_ip` prefers IPv4 addresses; upstream returns whatever interface
  address comes first, often a global IPv6 one.
- libmdns 0.10, restricted to the addresses of the interface that carries the
  configured host. Upstream announces every interface (Docker bridges too),
  and controllers stall trying addresses they cannot reach.
- The HAP server listens on all addresses (dual-stack) instead of only the
  configured host. Apple devices prefer the host's IPv6 address and showed
  the accessories as "Not Responding".
- `BridgeAccessory` has the HAP Protocol Information service the spec requires.
- Requests other than pair-setup, pair-verify and identify need a verified
  session (470 otherwise); upstream served and accepted them in plaintext.
- Color Temperature is declared `uint32`, as in the HAP spec.
- Accessory Information numbers its optional characteristics consecutively.
  Upstream used fixed offsets while the following service starts after the
  characteristic count, so e.g. Firmware Revision and a lightbulb's
  Brightness shared iid 10 (the Home app: "accessory out of compliance").
- A controller gets no event for its own write.
