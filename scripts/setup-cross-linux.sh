#!/usr/bin/env bash
# One-time setup for scripts/build-windows.sh on Ubuntu 24.04 (no rustup Windows target needed):
# distro Rust 1.91 + its std sources, MinGW, then two fixes to the distro's std sources so
# `-Zbuild-std` works for x86_64-pc-windows-gnu.
set -euo pipefail
apt-get install -y rustc-1.91 cargo-1.91 rust-1.91-src mingw-w64 zip
LIB=/usr/lib/rust-1.91/lib/rustlib
# 1. Debian's std/Cargo.toml drops the Windows-only `windows-targets` dependency.
if ! grep -q 'dependencies.windows-targets' $LIB/src/rust/library/std/Cargo.toml; then
  sed -i 's/^\[dev-dependencies\]/[target.'"'"'cfg(windows)'"'"'.dependencies.windows-targets]\npath = "..\/windows_targets"\n\n[dev-dependencies]/' $LIB/src/rust/library/std/Cargo.toml
  sed -i 's/^windows_raw_dylib = \[\]/windows_raw_dylib = ["windows-targets\/windows_raw_dylib"]/' $LIB/src/rust/library/std/Cargo.toml
fi
# 2. The MinGW startup objects (rsbegin.o / rsend.o) aren't shipped: build them.
D=$LIB/x86_64-pc-windows-gnu/lib/self-contained
mkdir -p "$D"
for f in rsbegin rsend; do
  RUSTC_BOOTSTRAP=1 rustc-1.91 --target x86_64-pc-windows-gnu --emit=obj -C panic=abort -O -o "$D/$f.o" "$LIB/src/rust/library/rtstartup/$f.rs"
  # The linker is given a bare "rsbegin.o": with the system MinGW it looks in the target's lib dir.
  cp "$D/$f.o" "$LIB/x86_64-pc-windows-gnu/lib/"
done
echo "Cross toolchain ready."
