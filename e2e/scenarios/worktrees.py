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
    """worktree：同一个仓库上的两个任务各自在 worktree 里改同一个文件互不干扰、.worktreeinclude 带上被忽略的文件、未提交改动的提醒、验收提交到 janus/<任务> 分支；非 Git 目录上有任务在跑时提醒"""
    t = ctx.team("worktrees", members=("m1", "m2"), script={"T1": ["write:a.txt"], "T2": ["write:a.txt"]},
                 files={"README.md": "wt\n", ".gitignore": ".env\n", ".worktreeinclude": ".env\n"})
    with open(os.path.join(t.repo, ".env"), "w") as f:
        f.write("KEY=1\n")
    with open(os.path.join(t.repo, "draft.md"), "w") as f:
        f.write("half done\n")
    warnings = ctx.api("GET", f"/api/runs/preflight?workspace={t.ws['id']}")["warnings"]
    c(len(warnings) == 1 and "draft.md" in warnings[0] and "不会带进" in warnings[0],
      f"创建前提醒：未提交的 draft.md 不会带进任务（{warnings[0][:40] if warnings else '没有提醒'}）")

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

    delivered = first.finish("第一个任务完成")
    c("worktree" in delivered["data"]["summary"], "交付摘要说明改动在任务的 worktree 里，验收时提交")
    first.act("accept")
    accepted = first.wait_event("risk.accepted")["data"]
    branch = f"janus/{first.id}"
    c(accepted.get("branch") == branch and _git(t.repo, "show", f"{branch}:a.txt") == "T1\n",
      f"验收后结果提交到分支 {branch}")
    c(not os.path.exists(f"{t.repo}/a.txt") and _git(t.repo, "branch", "--show-current").strip() != branch,
      "合并由用户决定：工作区目录和当前分支都没有变")
    ctx.ui(c, first.id, lambda r, s: r["status"] == "accepted" and r.get("branch") == branch,
           "应用显示结果在哪个分支", pages=("board", "team"))
    second.finish("第二个任务完成")

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
