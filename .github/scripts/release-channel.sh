#!/usr/bin/env bash
# Which channel a release tag publishes to, and whether it may (docs/operations/20261101-ci_release.md#channels):
#
#   stable  v<YYYY.MM.DD>[.<N>]           only on a commit of main; GitHub's latest release, which
#                                          every Janus.app on the stable channel installs.
#   rc      v<YYYY.MM.DD>[.<N>]-rc.<K>    on any branch (rc/<name> by convention); a prerelease, so
#                                          only apps that follow the RC channel see it. Its version
#                                          comes after the latest stable one (or nobody would get it),
#                                          and a candidate that changes data migrations is built on
#                                          the latest main, so its migrations follow main's.
#
# Usage: release-channel.sh <tag> <commit>   (needs GH_TOKEN, GITHUB_REPOSITORY and a full clone)
# Writes `channel=stable|rc` to $GITHUB_OUTPUT.
set -euo pipefail

tag="$1"
sha="$2"
out="${GITHUB_OUTPUT:-/dev/stdout}"
git fetch --quiet origin main

# Whether dotted version $1 is newer than $2, number by number.
newer() {
  python3 -I -c 'import sys
a, b = ([int(x) for x in v.split(".")] for v in sys.argv[1:3])
n = max(len(a), len(b))
sys.exit(0 if a + [0] * (n - len(a)) > b + [0] * (n - len(b)) else 1)' "$1" "$2"
}

if [[ "$tag" =~ ^v[0-9]+(\.[0-9]+)+$ ]]; then
  if ! git merge-base --is-ancestor "$sha" origin/main; then
    echo "稳定版 ${tag} 只能打在 main 的提交上：先合并到 main 再打标签；还在测试的分支打 RC 标签（例如 ${tag}-rc.1）。"
    exit 1
  fi
  echo "channel=stable" >>"$out"
  exit 0
fi

if [[ "$tag" =~ ^v([0-9]+(\.[0-9]+)+)-rc\.[0-9]+$ ]]; then
  base="${BASH_REMATCH[1]}"
  # Without a tag, `gh release view` reads the latest release, which is never a prerelease.
  latest="$(gh release view --repo "$GITHUB_REPOSITORY" --json tagName --jq .tagName 2>/dev/null || true)"
  latest="${latest#v}"
  if [ -n "$latest" ] && ! newer "$base" "$latest"; then
    echo "RC ${tag} 不比最新的稳定版 ${latest} 新，RC 渠道收不到它：用比 ${latest} 新的版本号。"
    exit 1
  fi
  if ! git diff --quiet origin/main..."$sha" -- backend/src/migrations.rs backend/src/migrations; then
    if ! git merge-base --is-ancestor origin/main "$sha"; then
      echo "RC ${tag} 改了数据迁移，但不在最新的 main 之上：先 rebase 到 main，迁移的版本号接在 main 的后面，合并回 main 时也不能变（AGENTS.md 合并前检查第 13 条）。"
      exit 1
    fi
  fi
  echo "channel=rc" >>"$out"
  exit 0
fi

echo "标签 ${tag} 既不是稳定版（v<年>.<月>.<日>[.<序号>]）也不是 RC（再加 -rc.<序号>）。"
exit 1
