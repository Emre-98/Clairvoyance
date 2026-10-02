#!/usr/bin/env bash
# Cross-builds a Windows test build from Linux (this is how Claude builds and tests it):
#   dist/Clairvoyance-<version>-portable.zip   Clairvoyance.exe + WebView2Loader.dll, unzip and run
#   dist/tools/recorder-selftest.exe           built-in recorder self-test (console)
# Real releases (installer + auto-update files) are built by GitHub Actions: see RELEASING.md.
# One-time toolchain setup on a fresh Ubuntu machine: scripts/setup-cross-linux.sh
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
(cd ui && npm run build)
# The UI tests' sample video (ui/public/dev-assets, made by ui/tests/make-sample.py) never ships.
rm -rf ui/dist/dev-assets
export RUSTC=/usr/bin/rustc-1.91 RUSTDOC=/usr/bin/rustdoc-1.91 RUSTC_BOOTSTRAP=1
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR=x86_64-w64-mingw32-ar
cargo-1.91 build --release --target x86_64-pc-windows-gnu -Zbuild-std=std,panic_abort -p clairvoyance
cargo-1.91 build --release --target x86_64-pc-windows-gnu -Zbuild-std=std,panic_abort -p cv-capture --bin recorder-selftest
OUT=${CARGO_TARGET_DIR:-target}/x86_64-pc-windows-gnu/release
rm -rf dist && mkdir -p dist/app dist/tools
cp "$OUT/clairvoyance.exe" dist/app/Clairvoyance.exe
cp "$OUT/WebView2Loader.dll" dist/app/
cp LICENSE dist/app/
cp "$OUT/recorder-selftest.exe" dist/tools/
(cd dist/app && zip -q -r "../Clairvoyance-$VERSION-portable.zip" .)
ls -la dist
