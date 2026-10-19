#!/bin/sh
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
sys="$(uname -s)"
case "$sys" in
MINGW* | MSYS* | CYGWIN*) rel="x86_64-pc-windows-msvc/gray-notify.exe" ;;
Darwin*)
    case "$(uname -m)" in
    arm64) rel="aarch64-apple-darwin/gray-notify" ;;
    *) rel="x86_64-apple-darwin/gray-notify" ;;
    esac ;;
*)
    case "$(uname -m)" in
    aarch64 | arm64) rel="aarch64-unknown-linux-musl/gray-notify" ;;
    *) rel="x86_64-unknown-linux-musl/gray-notify" ;;
    esac ;;
esac
if [ -x "$here/$rel" ]; then exec "$here/$rel" "$@"; fi
for cand in "$here/target/release/gray-notify.exe" "$here/target/release/gray-notify" "$here/target/debug/gray-notify.exe" "$here/target/debug/gray-notify"; do
    if [ -x "$cand" ]; then exec "$cand" "$@"; fi
done
echo "gray-notify binary not found in $here" >&2
exit 1
