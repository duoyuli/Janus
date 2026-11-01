#!/usr/bin/env bash
# Builds Janus.app (the native workbench with the Janus server inside) and Janus-<version>.dmg
# into dist/. Used by `just app` / `just dmg` and by the release workflow.
#
#   macos/scripts/package.sh [--version 2026.11.01] [--no-dmg]
#
# Janus.app is for Apple silicon only and is built on an Apple silicon Mac. Node.js (NODE_VERSION,
# an official nodejs.org build checked against its SHASUMS256.txt) goes into
# Contents/Resources/node, so ACP adapters need no Node on the Mac. The app is signed ad hoc: no Apple developer certificate is involved, so the
# first launch of a downloaded copy has to be allowed in System Settings (docs/operations/deployment.md#macos_app).
# @project-doc docs/operations/deployment.md#macos_app
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
# Node.js for the ACP adapters: an LTS release of at least 22.19 (the Pi adapter's minimum).
NODE_VERSION="${NODE_VERSION:-24.22.0}"
version=""
make_dmg=1
while [ $# -gt 0 ]; do
  case "$1" in
    --version) version="${2#v}"; shift 2 ;;
    --no-dmg) make_dmg=0; shift ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
[ "$(uname -m)" = arm64 ] || { echo "Janus.app 只支持 Apple 芯片，请在 Apple 芯片的 Mac 上打包" >&2; exit 1; }
[ -n "$version" ] || version="$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/backend/Cargo.toml" | head -1)"
build="$(git -C "$root" rev-list --count HEAD 2>/dev/null || echo 1)"

out="$root/dist"
work="$out/work"
rm -rf "$work" "$out/Janus.app"
mkdir -p "$work"

echo "==> Janus server (Rust)"
(cd "$root/backend" && cargo build --release --locked)
cp "$root/backend/target/release/janus" "$work/janus-server"

echo "==> Janus app (SwiftUI)"
(cd "$root/macos" && swift build -c release)
bin_dir="$(cd "$root/macos" && swift build -c release --show-bin-path)"

echo "==> Node.js $NODE_VERSION"
cache="$out/cache"
mkdir -p "$cache"
sums="$cache/node-v$NODE_VERSION-SHASUMS256.txt"
[ -s "$sums" ] || curl -fsSL -o "$sums" "https://nodejs.org/dist/v$NODE_VERSION/SHASUMS256.txt"
tgz="node-v$NODE_VERSION-darwin-arm64.tar.gz"
if [ ! -s "$cache/$tgz" ]; then
  curl -fsSL -o "$cache/$tgz.part" "https://nodejs.org/dist/v$NODE_VERSION/$tgz"
  mv -f "$cache/$tgz.part" "$cache/$tgz"
fi
want="$(awk -v f="$tgz" '$2 == f { print $1 }' "$sums")"
got="$(shasum -a 256 "$cache/$tgz" | awk '{ print $1 }')"
[ -n "$want" ] && [ "$want" = "$got" ] || { echo "Node.js 校验失败：$tgz" >&2; rm -f "$cache/$tgz"; exit 1; }
mkdir -p "$work/node-dist"
tar -xzf "$cache/$tgz" -C "$work/node-dist" --strip-components 1
# Only what the adapters use: node, npm and npx (no headers, docs or corepack).
node_dir="$work/node"
mkdir -p "$node_dir/bin" "$node_dir/lib/node_modules"
cp "$work/node-dist/bin/node" "$node_dir/bin/node"
cp -R "$work/node-dist/lib/node_modules/npm" "$node_dir/lib/node_modules/"
cp "$work/node-dist/LICENSE" "$node_dir/LICENSE"
ln -s ../lib/node_modules/npm/bin/npm-cli.js "$node_dir/bin/npm"
ln -s ../lib/node_modules/npm/bin/npx-cli.js "$node_dir/bin/npx"
"$node_dir/bin/node" --version
PATH="$node_dir/bin:/usr/bin:/bin" "$node_dir/bin/npm" --version

echo "==> Icon"
swift "$root/macos/scripts/make-icon.swift" "$work/AppIcon.iconset"
iconutil -c icns -o "$work/AppIcon.icns" "$work/AppIcon.iconset"

echo "==> Janus.app $version ($build)"
app="$out/Janus.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin_dir/Janus" "$app/Contents/MacOS/Janus"
cp "$work/janus-server" "$app/Contents/MacOS/janus-server"
cp "$work/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cp -R "$node_dir" "$app/Contents/Resources/node"
sed -e "s/__VERSION__/$version/" -e "s/__BUILD__/$build/" "$root/macos/Info.plist" >"$app/Contents/Info.plist"
printf 'APPL????' >"$app/Contents/PkgInfo"
# Ad-hoc signatures, inner code first.
codesign --force --sign - "$app/Contents/MacOS/janus-server"
codesign --force --sign - "$app"
codesign --verify --strict "$app"
"$app/Contents/MacOS/janus-server" --version
"$app/Contents/Resources/node/bin/node" --version

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
