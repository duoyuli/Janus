#!/usr/bin/env python3
"""Project Doc 结构检查：索引链接、成员命名、章节 ID 与 @project-doc 锚点。

只读，不修改任何文件。发现问题时逐行输出并以退出码 1 结束。

用法（在仓库根目录）：
    python3 <skill_dir>/scripts/check_library.py [--doc-root docs] [--changed]

--changed 只报告与工作区改动（相对 HEAD 的 diff 和未跟踪文件）相关的问题，
适合日常写任务；不带它即为全库审计。
"""

import argparse
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath

SNAKE = re.compile(r"^[a-z][a-z0-9_]*$")
ANCHOR = re.compile(r"@project-doc\s+(\S+)")
ID_TAG = re.compile(r'<a id="([^"]*)"></a>')
# 索引条目：「- [标题](路径)」，允许搭建期状态前缀。
ENTRY = re.compile(r"^- (?:\[(?:planned|in_progress|verified|blocked)\] )?\[[^\]]+\]\(([^)#\s]+)\)")
BOOTSTRAP = ".project_doc_bootstrap.md"


def git(root, *args):
    out = subprocess.run(["git", "-C", str(root), *args], capture_output=True, text=True)
    return out.stdout


def index_entries(root, index_path):
    """返回索引文件中规范条目指向的路径（相对仓库根）。"""
    base = PurePosixPath(index_path).parent
    entries = []
    in_fence = False
    for line in (root / index_path).read_text(encoding="utf-8").splitlines():
        if line.lstrip().startswith(("```", "~~~")):
            in_fence = not in_fence
        if in_fence:
            continue
        m = ENTRY.match(line)
        if m and not re.match(r"^[a-z]+://", m.group(1)):
            entries.append(normalize(base / m.group(1)))
    return entries


def normalize(path):
    parts = []
    for p in PurePosixPath(path).parts:
        if p == "..":
            parts.pop()
        elif p != ".":
            parts.append(p)
    return str(PurePosixPath(*parts))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--doc-root", default="docs")
    ap.add_argument("--changed", action="store_true")
    args = ap.parse_args()

    root = Path(git(".", "rev-parse", "--show-toplevel").strip() or ".")
    doc_root = args.doc_root.strip("/")
    root_index = f"{doc_root}/index.md"
    issues = []

    def report(path, msg):
        issues.append((path, msg))

    if not (root / root_index).is_file():
        print(f"{root_index}: 根索引不存在")
        return 1

    # 成员：根索引条目 + 各分类索引条目。
    members = {}  # path -> list of indexes listing it
    routed_dirs = set()
    for target in index_entries(root, root_index):
        members.setdefault(target, []).append(root_index)
        if target.endswith("/index.md"):
            routed_dirs.add(str(PurePosixPath(target).parent))
    for cat_index in sorted(t for t in list(members) if t.endswith("/index.md")):
        if not (root / cat_index).is_file():
            continue
        for target in index_entries(root, cat_index):
            members.setdefault(target, []).append(cat_index)

    for path, listed_in in sorted(members.items()):
        if not (root / path).is_file():
            report(listed_in[0], f"索引链接目标不存在：{path}")
            continue
        if len(listed_in) > 1:
            report(path, f"在多个索引中有规范条目：{', '.join(listed_in)}")
        rel = PurePosixPath(path).relative_to(doc_root)
        for part in [*rel.parts[:-1], rel.stem]:
            if not SNAKE.match(part):
                report(path, f"路径段不是小写 snake_case：{part}")

    # 分类目录中未被索引的 Markdown。
    for d in sorted(routed_dirs):
        for f in sorted((root / d).rglob("*.md")):
            rel = f.relative_to(root).as_posix()
            if rel not in members:
                report(rel, "位于分类目录但未被任何索引列出")

    # 章节 ID。
    ids = {}
    for path in members:
        p = root / path
        if not p.is_file():
            continue
        seen = ids.setdefault(path, set())
        for sid in ID_TAG.findall(p.read_text(encoding="utf-8")):
            if not SNAKE.match(sid):
                report(path, f"章节 ID 不是小写 snake_case：{sid}")
            if sid in seen:
                report(path, f"章节 ID 重复：{sid}")
            seen.add(sid)

    # 代码锚点。git grep 只看已跟踪和未忽略的文件，避开 node_modules、构建产物。
    grep = git(root, "grep", "-n", "--untracked", "-I", "-e", "@project-doc ",
               "--", ".", f":!{doc_root}", ":!.agents", ":!.claude")
    anchor_targets = set()
    for line in grep.splitlines():
        loc, _, text = line.partition(":")
        lineno, _, text = text.partition(":")
        for ref in ANCHOR.findall(text):
            where = f"{loc}:{lineno}"
            path, _, sid = ref.partition("#")
            anchor_targets.add((path, sid))
            if not sid:
                report(where, f"锚点缺少章节 ID：{ref}")
            elif not SNAKE.match(sid):
                report(where, f"锚点章节 ID 不是小写 snake_case：{ref}")
            if path not in members:
                state = "文件不存在" if not (root / path).is_file() else "不是索引成员"
                report(where, f"锚点目标{state}：{ref}")
            elif sid and sid not in ids.get(path, set()):
                report(where, f"锚点目标章节 ID 不存在：{ref}")

    if (root / doc_root / BOOTSTRAP).is_file():
        report(f"{doc_root}/{BOOTSTRAP}", "初始化备忘录仍存在；搭建完成后应删除")

    if args.changed:
        changed = set(git(root, "diff", "--name-only", "HEAD").split())
        changed |= set(git(root, "ls-files", "--others", "--exclude-standard").split())
        issues = [(p, m) for p, m in issues
                  if p.split(":")[0] in changed or any(c in m for c in changed)]

    for path, msg in issues:
        print(f"{path}: {msg}")
    scope = "改动相关" if args.changed else "全库"
    print(f"[{scope}] 成员 {len(members)} 篇，锚点目标 {len(anchor_targets)} 个，问题 {len(issues)} 个",
          file=sys.stderr)
    return 1 if issues else 0


if __name__ == "__main__":
    sys.exit(main())
