# Ciel 3
An **integrated packaging environment** for AOSC OS.

**Ciel** /sjɛl/ uses *systemd-nspawn* container as its backend and *overlay* file system as support rollback feature.

## Manual

```bash
ciel --help
```

## Installation

```bash
cargo build --release
install -Dm755 target/release/ciel-rs /usr/local/bin/ciel
PREFIX=/usr/local ./install-assets.sh
```

## Dependencies

Building:
- Rust w/ Cargo (Rust 1.80.0+)
- C compiler
- pkg-config (for detecting C library dependencies)
- make (when GCC LTO is used, not needed for Clang)

Runtime:
- Systemd
- D-Bus
- OpenSSL
- liblzma (optional)
- libgit2 (optional)

Runtime Kernel:
- Overlay file system
