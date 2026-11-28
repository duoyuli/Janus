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
    """worktree：同一个仓库上的两个任务各自在 worktree 里改同一个文件互不干扰、.worktreeinclude 带上被忽略的文件、未提交改动的提醒、交付时合并回任务开始时的分支、验收后负责人按要求做的提交再次交付并合并、冲突时不动用户目录并交给负责人一次；非 Git 目录上有任务在跑时提醒"""
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
    ctx.ui(c, first.id, lambda r, s: any(x["id"] == "T1" and branch in (x.get("git") or "") and start[:8] in (x.get("git") or "")
                                         and os.path.basename(a) not in (x.get("git") or "") for x in r["tasks"]),
           "子任务页显示 Git 仓库、分支和提交，不显示 Janus 放副本的路径", pages=("task:T1", "board"))

    # Both leads read the same go file, so both runs deliver now and race to the branch: the one
    # that delivers first merges, the other's change to the same a.txt conflicts with it.
    first.finish("两个任务都完成")
    wait(lambda: first.has("run.delivered") and second.has("run.delivered"), "两个任务都交付", 60)
    won, lost = (first, second) if first.has("run.merged") else (second, first)
    won_cwd, text = (a, "T1\n") if won is first else (b, "T2\n")
    won.wait_status("delivered")
    head = _git(t.repo, "rev-parse", "HEAD").strip()
    merge = won.has("run.delivered")[0]["data"]["merge"]
    c(merge["state"] == "merged" and "结果已合并到" in merge["note"] and branch in merge["note"],
      f"先交付的任务：交付说明结果已合并到分支 {branch}（{merge}）")
    merged = won.wait_event("run.merged", actor="janus")["data"]
    c(merged.get("branch") == branch and merged.get("commit") == head and _git(t.repo, "rev-parse", "HEAD^").strip() == start,
      f"交付时结果作为一个提交合并回 {branch}（快进，没有合并提交），记为 Janus 的 run.merged")
    c(read_file(f"{t.repo}/a.txt") == text and read_file(f"{t.repo}/draft.md") == "half done\n"
      and _git(t.repo, "branch", "--show-current").strip() == branch,
      "工作区目录里有了结果，用户没提交的 draft.md 原样保留，还在原来的分支上")
    c(not _git(t.repo, "branch", "--list", "janus/*").strip(), "不新建 janus/ 分支")
    ctx.ui(c, won.id, lambda r, s: r["status"] == "delivered" and r.get("branch") == branch and r.get("merged"),
           "应用显示结果已合并到哪个分支", pages=("board",))

    # The other run changed the same file: its result conflicts with what the first brought back.
    told = lambda: [e for e in lost.has("user.message", actor="janus") if "a.txt" in e["data"].get("text", "")]
    c(told() and "git rebase" in told()[0]["data"]["text"] and lost.has("run.delivered")[0]["data"]["merge"]["state"] == "resolving"
      and "负责人正在任务的副本里解决" in lost.has("run.delivered")[0]["data"]["merge"]["note"],
      "后交付的任务冲突：Janus 把冲突的文件和解决步骤交给负责人，交付说明负责人在解决")
    c(read_file(f"{t.repo}/a.txt") == text and not _git(t.repo, "status", "--porcelain", "--untracked-files=no").strip()
      and not lost.has("run.merged"),
      "冲突时用户的分支不动，目录里没有半合并的状态")
    wait(lambda: len(lost.has("run.delivered")) == 2, "负责人处理冲突的一轮结束后再次交付", 60)
    c(lost.has("run.delivered")[1]["data"]["merge"]["state"] == "conflict" and "仍然冲突" in lost.has("run.delivered")[1]["data"]["merge"]["note"] and len(told()) == 1,
      "负责人没解决时再次交付说明仍然冲突，不再自动交给负责人")
    lost.wait_status("delivered")
    lost.act("accept")
    c(not lost.has("risk.accepted") and lost.status() == "delivered" and len(told()) == 2,
      "用户点验收时仍然冲突：再交给负责人一次，不算验收")

    won.act("accept")
    accepted = won.wait_event("risk.accepted")
    c("结果已在" in accepted["text"] and accepted["data"].get("commit") is None
      and _git(t.repo, "rev-parse", "HEAD").strip() == head,
      "验收时结果已经在分支里，不再提交")
    # After the acceptance the person asks the lead for more and for a commit: committed in the
    # worktree, it is delivered again, and that delivery brings the lead's commit to the branch.
    won.act("message", agent="lead", text="再补一份说明 [write more.md] 并提交 [commit]")
    wait(lambda: len(won.has("run.delivered")) == 2, "验收后负责人提交的改动再次交付", 60)
    won.wait_status("delivered")
    c(not _git(won_cwd, "status", "--porcelain").strip() and read_file(f"{t.repo}/more.md") == "follow-up\n"
      and _git(t.repo, "log", "-1", "--format=%s").strip() == "follow-up",
      "验收后负责人按要求提交，再次交付时它的提交直接出现在用户的分支和目录里")
    c(len(won.has("run.merged")) == 2, "第二次交付也记一条 run.merged")

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
