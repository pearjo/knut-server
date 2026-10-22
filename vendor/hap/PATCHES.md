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
- libmdns 0.10, restricted to the address the HAP server listens on. Upstream
  announces every interface address (Docker bridges, IPv6), and controllers
  stall trying addresses nothing listens on.
