#!/usr/bin/env bash
# Builds the Windows app from Linux (this is how Claude builds it):
#   dist/GameRecorder-Setup-<version>.exe      installer (per-user, no admin)
#   dist/GameRecorder-<version>-portable.zip   unzip-and-run version
#   dist/tools/recorder-selftest.exe           built-in recorder self-test (console)
# On Windows you can instead run:  cd ui && npm ci && npm run build && cd .. && cargo build --release -p gamerecorder
set -euo pipefail
cd "$(dirname "$0")/.."
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
(cd ui && npm run build)
export RUSTC=/usr/bin/rustc-1.91 RUSTDOC=/usr/bin/rustdoc-1.91 RUSTC_BOOTSTRAP=1
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR=x86_64-w64-mingw32-ar
cargo-1.91 build --release --target x86_64-pc-windows-gnu -Zbuild-std=std,panic_abort -p gamerecorder
cargo-1.91 build --release --target x86_64-pc-windows-gnu -Zbuild-std=std,panic_abort -p gr-capture --bin recorder-selftest
OUT=target/x86_64-pc-windows-gnu/release
rm -rf dist && mkdir -p dist/app
cp "$OUT/gamerecorder.exe" dist/app/GameRecorder.exe
cp "$OUT/WebView2Loader.dll" dist/app/
cp LICENSE dist/app/
(cd installer && makensis -V2 -DVERSION="$VERSION" -DSRC=../dist/app -DOUTDIR=../dist GameRecorder.nsi)
mkdir -p dist/tools && cp "$OUT/recorder-selftest.exe" dist/tools/
(cd dist/app && zip -q -r "../GameRecorder-$VERSION-portable.zip" .)
ls -la dist
