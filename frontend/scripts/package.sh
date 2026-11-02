#!/usr/bin/env bash
# Builds Janus.app (the native workbench with the Janus server inside) and Janus-<version>.dmg
# into dist/. Used by `just app` / `just dmg` and by the release workflow.
#
#   frontend/scripts/package.sh [--version 2026.11.01] [--no-dmg]
#
# Janus.app is for Apple silicon only and is built on an Apple silicon Mac. Node.js (NODE_VERSION,
# an official nodejs.org build checked against its SHASUMS256.txt) goes into
# Contents/Resources/node, so ACP adapters need no Node on the Mac.
#
# Signing: JANUS_SIGN_IDENTITY (a certificate's name or SHA-1 in the keychain, optionally from
# JANUS_SIGN_KEYCHAIN) gives every release the same identity, so macOS keeps the app's permissions
# across updates and the in-app updater can check an update comes from the same signer. Without it
# the app is signed ad hoc (docs/operations/ci_release.md#signing).
#
# Besides the DMG, the update parts the in-app updater downloads (docs/operations/ci_release.md#update_parts):
# Janus-<version>-app.tar.gz (the bundle without Node.js), Janus-<version>-node.tar.gz and update.json.
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
(cd "$root/frontend" && swift build -c release)
bin_dir="$(cd "$root/frontend" && swift build -c release --show-bin-path)"

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
swift "$root/frontend/scripts/make-icon.swift" "$work/AppIcon.iconset"
iconutil -c icns -o "$work/AppIcon.icns" "$work/AppIcon.iconset"

# What the bundled Node.js is, file by file: an update downloads Node.js only when this changes.
node_hash="$(cd "$node_dir" && {
  find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256
  find . -type l | LC_ALL=C sort | while read -r l; do echo "link $l $(readlink "$l")"; done
} | shasum -a 256 | cut -c1-64)"

echo "==> Janus.app $version ($build)"
app="$out/Janus.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin_dir/Janus" "$app/Contents/MacOS/Janus"
cp "$work/janus-server" "$app/Contents/MacOS/janus-server"
cp "$work/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cp -R "$node_dir" "$app/Contents/Resources/node"
sed -e "s/__VERSION__/$version/" -e "s/__BUILD__/$build/" "$root/frontend/Info.plist" >"$app/Contents/Info.plist"
printf 'APPL????' >"$app/Contents/PkgInfo"
printf '{"version":"%s","node":"%s"}\n' "$version" "$node_hash" >"$app/Contents/Resources/update-parts.json"
# Signatures, inner code first.
identity="${JANUS_SIGN_IDENTITY:--}"
sign=(codesign --force --sign "$identity")
[ -n "${JANUS_SIGN_KEYCHAIN:-}" ] && sign+=(--keychain "$JANUS_SIGN_KEYCHAIN")
"${sign[@]}" "$app/Contents/MacOS/janus-server"
"${sign[@]}" "$app"
codesign --verify --deep --strict "$app"
codesign -dr - "$app" 2>&1 | sed -n 's/^designated => /==> designated requirement: /p'
"$app/Contents/MacOS/janus-server" --version
"$app/Contents/Resources/node/bin/node" --version

echo "==> Update parts"
# No AppleDouble files or extended attributes: the parts unpack to exactly the signed files.
export COPYFILE_DISABLE=1
app_part="Janus-$version-app.tar.gz"
node_part="Janus-$version-node.tar.gz"
tar --no-mac-metadata -czf "$out/$app_part" -C "$out" --exclude 'Janus.app/Contents/Resources/node' Janus.app
tar --no-mac-metadata -czf "$out/$node_part" -C "$app/Contents/Resources" node
part_json() { # name file content
  printf '{"name":"%s","file":"%s","sha256":"%s","size":%s,"content":"%s"}' "$1" "$2" \
    "$(shasum -a 256 "$out/$2" | cut -c1-64)" "$(stat -f %z "$out/$2")" "$3"
}
printf '{"version":"%s","build":%s,"minimumSystemVersion":"14.0","parts":[%s,%s]}\n' "$version" "$build" \
  "$(part_json app "$app_part" "$version")" "$(part_json node "$node_part" "$node_hash")" >"$out/update.json"
cat "$out/update.json"

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
