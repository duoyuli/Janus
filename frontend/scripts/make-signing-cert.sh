#!/usr/bin/env bash
# Creates the self-signed code-signing certificate Janus releases are signed with, once.
#
#   frontend/scripts/make-signing-cert.sh [output dir, default ~/.janus/signing]
#
# Every release signed with the same certificate is the same app to macOS: permissions granted
# to Janus survive updates, and the in-app updater accepts only updates signed with it. It is
# not an Apple Developer ID, so it does not replace Gatekeeper's first-launch approval of a
# downloaded DMG; updates installed from inside the app never need that approval
# (docs/operations/ci_release.md#signing).
#
# Nothing is uploaded: the script prints the commands that store the certificate as the
# repository's Actions secrets. Keep the output directory private; whoever has the .p12 can sign
# updates the app will install.
set -euo pipefail

dir="${1:-$HOME/.janus/signing}"
name="Janus Self-Signed"
if [ -e "$dir/janus-signing.p12" ]; then
  echo "已经有证书：$dir/janus-signing.p12（换证书会让已安装的应用拒绝之后的更新，确实要换就先移走它）" >&2
  exit 1
fi
mkdir -p "$dir"
chmod 700 "$dir"
cd "$dir"

cat >cert.cnf <<EOF
[req]
distinguished_name = dn
prompt = no
x509_extensions = ext
[dn]
CN = $name
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF
openssl req -x509 -newkey rsa:3072 -nodes -keyout key.pem -out cert.pem -days 7300 -config cert.cnf 2>/dev/null
password="$(openssl rand -hex 24)"
# macOS's `security import` reads the older PKCS#12 encryption; OpenSSL 3 needs -legacy for it.
if ! openssl pkcs12 -export -legacy -inkey key.pem -in cert.pem -name "$name" -out janus-signing.p12 -passout "pass:$password" 2>/dev/null; then
  openssl pkcs12 -export -inkey key.pem -in cert.pem -name "$name" -out janus-signing.p12 -passout "pass:$password"
fi
printf '%s' "$password" >janus-signing.password
rm -f key.pem cert.cnf
chmod 600 janus-signing.p12 janus-signing.password
sha1="$(openssl x509 -in cert.pem -noout -fingerprint -sha1 | sed 's/.*=//; s/://g')"

cat <<EOF
已生成 $dir/janus-signing.p12（证书 SHA-1 $sha1）。

1. 存为仓库的 Actions secrets，之后的 Release 都用它签名：
   gh secret set MACOS_SIGNING_P12 --body "\$(base64 -i '$dir/janus-signing.p12')"
   gh secret set MACOS_SIGNING_PASSWORD < '$dir/janus-signing.password'

2. 本机打包也想用它签名时（可选）：
   security import '$dir/janus-signing.p12' -k login.keychain-db -P "\$(cat '$dir/janus-signing.password')" -T /usr/bin/codesign
   JANUS_SIGN_IDENTITY=$sha1 just dmg
EOF
