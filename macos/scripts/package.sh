#!/usr/bin/env bash
# Builds Janus.app (the native workbench with the Janus server inside) and Janus-<version>.dmg
# into dist/. Used by `just app` / `just dmg` and by the release workflow.
#
#   macos/scripts/package.sh [--version 0.2.0] [--universal] [--no-dmg]
#
# --universal builds arm64 + x86_64 (needs Xcode and both Rust targets); the default builds for
# this Mac only. The app is signed ad hoc: no Apple developer certificate is involved, so the
# first launch of a downloaded copy has to be allowed in System Settings (docs/operations/deployment.md#macos_app).
# @project-doc docs/operations/deployment.md#macos_app
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
version=""
universal=0
make_dmg=1
while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2#v}"; shift 2 ;;
    --universal) universal=1; shift ;;
    --no-dmg) make_dmg=0; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[ -n "$version" ] || version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/backend/Cargo.toml" | head -1)"
build="$(git -C "$root" rev-list --count HEAD 2>/dev/null || echo 1)"

out="$root/dist"
work="$out/work"
rm -rf "$work" "$out/Janus.app"
mkdir -p "$work"

echo "==> Janus server (Rust)"
if [ "$universal" = 1 ]; then
  for t in aarch64-apple-darwin x86_64-apple-darwin; do
    (cd "$root/backend" && cargo build --release --locked --target "$t")
  done
  lipo -create -output "$work/janus-server" \
    "$root/backend/target/aarch64-apple-darwin/release/janus" \
    "$root/backend/target/x86_64-apple-darwin/release/janus"
else
  (cd "$root/backend" && cargo build --release --locked)
  cp "$root/backend/target/release/janus" "$work/janus-server"
fi

echo "==> Janus app (SwiftUI)"
swift_flags=(-c release)
[ "$universal" = 1 ] && swift_flags+=(--arch arm64 --arch x86_64)
(cd "$root/macos" && swift build "${swift_flags[@]}")
bin_dir="$(cd "$root/macos" && swift build "${swift_flags[@]}" --show-bin-path)"

echo "==> Icon"
swift "$root/macos/scripts/make-icon.swift" "$work/AppIcon.iconset"
iconutil -c icns -o "$work/AppIcon.icns" "$work/AppIcon.iconset"

echo "==> Janus.app $version ($build)"
app="$out/Janus.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin_dir/Janus" "$app/Contents/MacOS/Janus"
cp "$work/janus-server" "$app/Contents/MacOS/janus-server"
cp "$work/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
sed -e "s/__VERSION__/$version/" -e "s/__BUILD__/$build/" "$root/macos/Info.plist" >"$app/Contents/Info.plist"
printf 'APPL????' >"$app/Contents/PkgInfo"
# Ad-hoc signatures, inner code first.
codesign --force --sign - "$app/Contents/MacOS/janus-server"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
"$app/Contents/MacOS/janus-server" --version

if [ "$make_dmg" = 1 ]; then
  echo "==> Janus-$version.dmg"
  stage="$work/dmg"
  mkdir -p "$stage"
  cp -R "$app" "$stage/"
  ln -s /Applications "$stage/Applications"
  dmg="$out/Janus-$version.dmg"
  rm -f "$dmg"
  # hdiutil now and then fails with "Resource busy" on CI machines; try again.
  for i in 1 2 3; do
    hdiutil create -quiet -volname "Janus $version" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg" && break
    [ "$i" = 3 ] && exit 1
    sleep 5
  done
  echo "$dmg"
fi
rm -rf "$work"
echo "$app"
