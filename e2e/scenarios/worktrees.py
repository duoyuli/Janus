"""Runs on one repository work apart, each in its own worktree; outside git the person is reminded
of a run still working there (docs/decisions/20261119-run_worktrees.md)."""

import os
import shutil
import subprocess

from lib import wait
from scenarios import read_file, scenario, task


def _git(repo, *args):
    return subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True).stdout


@scenario("worktrees")
def worktrees(ctx, c):
    """worktree：同一个仓库上的两个任务各自在 worktree 里改同一个文件互不干扰、.worktreeinclude 带上被忽略的文件、未提交改动的提醒、验收后合并回任务开始时的分支、冲突时验收失败且不动用户目录；非 Git 目录上有任务在跑时提醒"""
    t = ctx.team("worktrees", members=("m1", "m2"), script={"T1": ["write:a.txt"], "T2": ["write:a.txt"]},
                 files={"README.md": "wt\n", ".gitignore": ".env\n", ".worktreeinclude": ".env\n"})
    with open(os.path.join(t.repo, ".env"), "w") as f:
        f.write("KEY=1\n")
    with open(os.path.join(t.repo, "draft.md"), "w") as f:
        f.write("half done\n")
    warnings = ctx.api("GET", f"/api/runs/preflight?workspace={t.ws['id']}")["warnings"]
    c(len(warnings) == 1 and "draft.md" in warnings[0] and "不会带进" in warnings[0],
      f"创建前提醒：未提交的 draft.md 不会带进任务（{warnings[0][:40] if warnings else '没有提醒'}）")

    branch = _git(t.repo, "branch", "--show-current").strip()
    start = _git(t.repo, "rev-parse", "HEAD").strip()
    first = t.run("第一个任务")
    second = t.run("第二个任务")
    first.orchestrate(task("T1", t.member("m1")))
    second.orchestrate(task("T2", t.member("m2")))
    first.wait_event("task.succeeded", task="T1")
    second.wait_event("task.succeeded", task="T2")
    a, b = first.cwd(), second.cwd()
    c(a != b and a != t.repo and b != t.repo, "两个任务各有一个 worktree，都不是工作区目录")
    c(read_file(f"{a}/a.txt") == "T1\n" and read_file(f"{b}/a.txt") == "T2\n",
      "两个任务同时改同一个文件 a.txt，各改各的")
    c(not os.path.exists(f"{t.repo}/a.txt"), "工作区目录不受影响")
    c(read_file(f"{a}/.env") == "KEY=1\n" and not os.path.exists(f"{a}/draft.md"),
      ".worktreeinclude 带上了被忽略的 .env，未提交的 draft.md 没有带进去")
    c(not first.has("task.conflict") and not second.has("task.conflict"), "两个任务之间没有冲突")
    git = first.wait_event("attempt.started", task="T1")["data"].get("git") or {}
    c(git.get("branch") == branch and git.get("head") == start and os.path.realpath(git.get("worktree", "")) == os.path.realpath(a),
      f"子任务的每次执行记下 Git 信息：分支、开始时的提交和 worktree 路径（{git}）")
    ctx.ui(c, first.id, lambda r, s: any(x["id"] == "T1" and branch in (x.get("git") or "") and os.path.basename(a) in (x.get("git") or "")
                                         for x in r["tasks"]),
           "子任务页显示 Git 仓库、分支、提交和 worktree 路径", pages=("task:T1", "board"))

    delivered = first.finish("第一个任务完成")
    c("worktree" in delivered["data"]["summary"] and branch in delivered["data"]["summary"],
      f"交付摘要说明改动在任务的 worktree 里，验收时合并回分支 {branch}")
    first.act("accept")
    accepted = first.wait_event("risk.accepted")["data"]
    c(accepted.get("branch") == branch and accepted.get("commit") == _git(t.repo, "rev-parse", "HEAD").strip()
      and _git(t.repo, "rev-parse", "HEAD^").strip() == start,
      f"验收后结果作为一个提交合并回 {branch}（快进，没有合并提交）")
    c(read_file(f"{t.repo}/a.txt") == "T1\n" and read_file(f"{t.repo}/draft.md") == "half done\n"
      and _git(t.repo, "branch", "--show-current").strip() == branch,
      "工作区目录里有了结果，用户没提交的 draft.md 原样保留，还在原来的分支上")
    c(not _git(t.repo, "branch", "--list", "janus/*").strip(), "不新建 janus/ 分支")
    ctx.ui(c, first.id, lambda r, s: r["status"] == "accepted" and r.get("branch") == branch and r.get("merged"),
           "应用显示结果已合并到哪个分支", pages=("board", "team"))

    # The second run changed the same file: its result conflicts with what the first brought back.
    second.finish("第二个任务完成")
    second.wait_status("delivered")
    code, text = second.act_error("accept")
    c(code == 400 and "冲突" in text, f"第二个任务和已合并的结果冲突，验收失败（{code} {text[:60]}）")
    c(second.status() == "delivered" and read_file(f"{t.repo}/a.txt") == "T1\n"
      and not _git(t.repo, "status", "--porcelain", "--untracked-files=no").strip(),
      "验收失败时任务保持已交付，用户的目录里没有半合并的状态")

    # Outside git there is no worktree: the person is reminded of a run still working there.
    plain = ctx.team("plain", members=("m1",), script={"P1": ["slow"]})
    shutil.rmtree(os.path.join(plain.repo, ".git"))
    run = plain.run("非 Git 目录上的任务")
    run.orchestrate(task("P1", plain.member("m1")))
    run.wait_event("attempt.started", task="P1")
    c(os.path.realpath(run.cwd()) == os.path.realpath(plain.repo), "非 Git 目录上的任务直接在目录里运行")
    warnings = ctx.api("GET", f"/api/runs/preflight?workspace={plain.ws['id']}")["warnings"]
    c(len(warnings) == 1 and "正在运行的任务" in warnings[0], "非 Git 目录上有任务在跑时，创建前提醒")
    run.act("cancel")
    wait(lambda: run.status() == "cancelled", "非 Git 目录上的任务取消", 30)
