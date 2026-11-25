#!/usr/bin/env python3
"""The screenshots in docs/assets/screenshots (docs/operations/20261031-testing.md#screenshots).

Stages demonstration data on a scratch Janus home with the end-to-end test's drivers, in English
and in Chinese, and pictures four pages of the app in the light and the dark appearance: the
inbox, a delivered run, a collaborator's session at work and a workspace. The scripted agent
says and writes what `content.json` gives it, so the pages show plausible work, not test strings.
The pictures are converted to sRGB and reduced to 256 colours with ImageMagick.

Usage: e2e/screenshots.py [--lang en|zh ...] [--out DIR] [--keep]
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time

# Imports from e2e/ must not leave __pycache__ in the checkout (AGENTS.md rule 9).
sys.dont_write_bytecode = True
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from lib import ROOT, App, Fake, Failure, Registry, Server, free_addr, git_repo, http, say, stand_in_adapters, wait  # noqa: E402
from product import build  # noqa: E402

# Workspace paths show in the pictures, so the scratch directory is a short one that names nobody.
BASE = "/tmp/demo"
MARK = os.path.join(BASE, ".janus-screenshots")
SRGB = "/System/Library/ColorSync/Profiles/sRGB Profile.icc"
# The English delivery stops under "To Look At": the line below it is written by the server and is
# still Chinese in the English interface. Pixel rows kept, the last 40 fading out.
CUT = {("delivery", "en"): 956}

EXPORT_PY = '''"""Invoices as CSV rows."""
import csv
import io

COLUMNS = ["number", "issued", "customer", "net", "tax", "total", "currency"]


def invoice_rows(invoices):
    """Yield CSV lines for `invoices`, header first. Amounts keep two decimals."""
    buf = io.StringIO()
    out = csv.writer(buf)
    out.writerow(COLUMNS)
    for inv in invoices:
        out.writerow([inv.number, inv.issued.isoformat(), inv.customer,
                      f"{inv.net:.2f}", f"{inv.tax:.2f}", f"{inv.total:.2f}", inv.currency])
        yield buf.getvalue()
        buf.seek(0)
        buf.truncate()
'''
TEST_EXPORT_PY = '''from decimal import Decimal

from ledger.export import invoice_rows
from tests.factories import invoice


def test_amounts_keep_two_decimals():
    rows = "".join(invoice_rows([invoice(net=Decimal("1280"), currency="EUR")]))
    assert "1280.00" in rows and "EUR" in rows


def test_fields_are_escaped():
    rows = "".join(invoice_rows([invoice(customer='Acme, "North"')]))
    assert '"Acme, ""North"""' in rows
'''
API_OLD = '''from fastapi import APIRouter

from ledger.models import invoices_between

router = APIRouter()


@router.get("/invoices")
def list_invoices(start: str, end: str):
    return [i.as_dict() for i in invoices_between(start, end)]
'''
API_NEW = '''from fastapi import APIRouter, HTTPException
from fastapi.responses import StreamingResponse

from ledger.export import invoice_rows
from ledger.models import count_between, invoices_between

router = APIRouter()


@router.get("/invoices")
def list_invoices(start: str, end: str):
    return [i.as_dict() for i in invoices_between(start, end)]


@router.get("/invoices/export")
def export_invoices(start: str, end: str):
    if count_between(start, end) > 10_000:
        raise HTTPException(422, "range has more than 10000 invoices")
    return StreamingResponse(invoice_rows(invoices_between(start, end)), media_type="text/csv")
'''
TEST_API_OLD = "def test_list(client):\n    assert client.get('/invoices', params={'start': 'a', 'end': 'b'}).status_code == 200\n"
TEST_API_NEW = TEST_API_OLD + '''

def test_export_cap(client, many_invoices):
    r = client.get("/invoices/export", params={"start": "2020-01-01", "end": "2026-12-31"})
    assert r.status_code == 422
'''
DOCS_OLD = "# API\n\n## GET /invoices\n\nLists invoices issued between `start` and `end`.\n"
DOCS_NEW = DOCS_OLD + "\n## GET /invoices/export\n\nStreams the invoices issued between `start` and `end` as CSV. Returns `422` above 10,000 invoices.\n"
LEDGER_FILES = {
    "README.md": "# ledger\n\nInvoices and tax for the back office.\n",
    "ledger/__init__.py": "",
    "ledger/api.py": API_OLD,
    "ledger/models.py": "def invoices_between(start, end):\n    return []\n\n\ndef count_between(start, end):\n    return 0\n",
    "docs/api.md": DOCS_OLD,
    "CHANGELOG.md": "# Changelog\n\n## Unreleased\n",
    "tests/test_api.py": TEST_API_OLD,
}
# What each subtask writes, how long it works, and what its session reports.
FILES = {
    "T1": {"ledger/export.py": EXPORT_PY, "tests/test_export.py": TEST_EXPORT_PY},
    "T2": {"ledger/api.py": API_NEW, "tests/test_api.py": TEST_API_NEW},
    "T3": {"docs/api.md": DOCS_NEW, "CHANGELOG.md": "# Changelog\n\n## Unreleased\n\n- `GET /invoices/export` streams invoices as CSV.\n"},
    "H1": {"docs/onboarding.md": "# Onboarding\n\nStart with the [handbook](https://example.com/handbook).\n"},
    "S1": {"config/flags.yaml": "checkout_v2:\n  rollout: 5\n"},
}
TOKENS = {"T1": 38400, "T2": 52100, "T3": 41700, "V1": 29300, "H1": 12800, "H2": 9400, "S1": 17600}
CONTEXT = {"T1": 41000, "T2": 58000, "T3": 36000, "V1": 47000, "H1": 22000, "H2": 19000, "S1": 26000}
COST = {"T1": 0.21, "T2": 0.29, "T3": 0.23, "V1": 0.16, "H1": 0.07, "H2": 0.05, "S1": 0.1}
SECONDS = {"T1": 34, "T2": 46, "T3": 26, "V1": 30}

TEXTS = {
    "en": {
        "language": "en",
        # The default digital employees are written in Chinese: the English pictures name them in English.
        "roles": {
            "pmo": ("PMO", "Lead: breaks the goal down, sequences and assigns the work, tracks progress and risk, delivers"),
            "developer": ("Engineer", "Designs, writes code, fixes bugs and adds tests; code and configuration changes go here"),
            "qa": ("QA Engineer", "Designs and runs tests and independently verifies others' changes; reads, never edits, while verifying"),
        },
        "brief": "Python 3.12 with pytest. Money is Decimal, never float. Public API changes need an entry in docs/api.md.",
        "old_goal": "Fix rounding in tax totals",
        "old_report": "## Conclusion\nTax totals now round half-up per line before summing; the three invoices from the bug report come out right.",
        "goal": "Add CSV export for invoices, with tests and API docs",
        "report": ("## Conclusion\nInvoices can now be exported as CSV: `GET /invoices/export` streams the file, with tests and API docs. "
                   "QA verified it independently on the result.\n\n"
                   "## Needs Your Decision\n- Exports are capped at 10,000 rows per request. Keep the cap, or add paging for larger ranges?\n\n"
                   "## What was done\n\n- **T1** Engineer added the CSV serializer in `ledger/export.py`, with unit tests.\n"
                   "- **T2** Engineer exposed `GET /invoices/export`; it streams rows and rejects oversized ranges with 422.\n"
                   "- **T3** Engineer documented the endpoint and added a changelog entry.\n"
                   "- **V1** QA Engineer ran the full suite and checked a real export against the database.\n"),
        "tasks": [
            {"id": "T1", "title": "CSV serializer for invoices", "assignee": "developer", "owns": ["ledger/export.py", "tests/test_export.py"],
             "instructions": "Add ledger/export.py that turns invoices into CSV rows, as a generator so the endpoint can stream it.",
             "accept": ["Amounts keep two decimals and the currency code", "Fields with commas, quotes or newlines are escaped"]},
            {"id": "T2", "title": "GET /invoices/export endpoint", "assignee": "developer", "deps": ["T1"], "owns": ["ledger/api.py", "tests/test_api.py"],
             "instructions": "Expose the export over HTTP for a date range. Stream it; refuse ranges that are too large.",
             "accept": ["Rows are streamed, not collected in memory", "Ranges over 10,000 rows are rejected with 422"]},
            {"id": "T3", "title": "API docs and changelog", "assignee": "developer", "deps": ["T2"], "owns": ["docs/api.md", "CHANGELOG.md"],
             "instructions": "Document the new endpoint in docs/api.md and add a changelog entry.",
             "accept": ["docs/api.md lists the parameters, the columns and the 422 error", "CHANGELOG.md has an entry under Unreleased"]},
            {"id": "V1", "title": "Independent verification of the export", "assignee": "qa", "verifies": "T2",
             "instructions": "Verify the export on your own: run the suite and check a real export against the data.",
             "accept": ["The full test suite passes", "An exported file has the right columns and amounts"]},
        ],
        "results": {
            "T1": {"thought": "The endpoint will stream, so this should be a generator over the csv module rather than one big string.",
                   "plan": ["Read ledger/models.py", "Write ledger/export.py", "Add unit tests", "Run the tests"],
                   "summary": "Added `ledger/export.py`: `invoice_rows()` yields CSV lines from any iterable of invoices, header first.",
                   "handoff": "Call `invoice_rows(invoices)`. It is a generator, so the endpoint can pass it straight to a streaming response.",
                   "checks": [{"method": "test", "evidence": "pytest tests/test_export.py::test_amounts_keep_two_decimals: passed"},
                              {"method": "test", "evidence": "pytest tests/test_export.py::test_fields_are_escaped: passed"}],
                   "evidence": ["pytest tests/test_export.py -q: 2 passed in 0.08s"]},
            "T2": {"thought": "Count first so an oversized range fails before any row is sent.",
                   "plan": ["Read ledger/api.py", "Add the export route", "Add API tests", "Run the tests"],
                   "summary": "`GET /invoices/export?start=&end=` streams the CSV and answers 422 above 10,000 rows.",
                   "handoff": "The cap is in ledger/api.py. The 422 body names the limit.",
                   "checks": [{"method": "inspection", "evidence": "The handler returns StreamingResponse(invoice_rows(...)); nothing is collected into a list"},
                              {"method": "test", "evidence": "pytest tests/test_api.py::test_export_cap: passed"}],
                   "evidence": ["pytest tests/test_api.py -q: 3 passed in 0.41s"],
                   "risks": ["The 10,000-row cap is a guess; large customers may need paging"]},
            "T3": {"thought": "The docs need the column list; the changelog needs one line.",
                   "plan": ["Read docs/api.md", "Document GET /invoices/export", "Add the changelog entry", "Check the links"],
                   "summary": "Documented `GET /invoices/export` in docs/api.md and added a changelog entry.",
                   "checks": [{"method": "inspection", "evidence": "Read the new section: both parameters, the columns and the 422 answer are there"},
                              {"method": "command", "evidence": "git diff --stat CHANGELOG.md: 1 insertion under Unreleased"}],
                   "evidence": ["markdownlint docs/api.md: no issues"],
                   "command": "markdownlint docs/api.md CHANGELOG.md"},
            "V1": {"thought": "Run everything from a clean checkout, then compare a real export with the database.",
                   "plan": ["Run the full suite", "Export October from a seeded database", "Compare with the rows in the database"],
                   "summary": "Verified independently: the suite passes and an export of October matches the database row for row.", "verdict": "approved",
                   "checks": [{"method": "test", "evidence": "pytest -q: 41 passed in 3.2s"},
                              {"method": "command", "evidence": "curl -s 'localhost:8000/invoices/export?start=2026-10-01&end=2026-10-31' | head -3: header plus INV-2031 1280.00 EUR and INV-2032 96.50 EUR, as in the database"}],
                   "evidence": ["pytest -q: 41 passed in 3.2s"],
                   "command": "pytest -q"},
        },
        "memories": [
            {"kind": "howto", "title": "Running the tests", "hook": "uv run pytest -q; API tests need LEDGER_DB=:memory:", "body": "Run `uv run pytest -q`. The API tests read `LEDGER_DB`; set it to `:memory:`."},
            {"kind": "pitfall", "title": "Money is Decimal", "hook": "float amounts fail the rounding tests", "body": "Build amounts with Decimal('...'). A float anywhere on the way fails tests/test_tax.py."},
        ],
        # (name, prompt, cron, what it says after a wake)
        "loops": [
            ("Issue triage", "Look at the issue tracker for new bug reports.\nAnswer what a sentence can answer; dispatch real bugs as runs.", "0 * * * *",
             "2 new issues: answered #412, dispatched #415 as a run."),
            ("Dependency watch", "Check the lockfile against new security advisories.", "0 9 * * 1-5", "No advisories against the lockfile today."),
        ],
        "dispatch": "Fix duplicate rows when an invoice has two tax lines (#415)",
        "store_goal": "Put the new checkout behind a feature flag and roll it out to 5%",
        "store_task": {"id": "S1", "title": "Feature flag and rollout config", "assignee": "developer",
                       "instructions": "Add the checkout_v2 flag at 5% and push the branch for review.",
                       "accept": ["config/flags.yaml has checkout_v2 at 5%"]},
        "store_ask": {"question": "Roll out to 5% of all shoppers, or only to signed-in ones?",
                      "why": "It decides who sees the new checkout first, and only you know the launch plan.",
                      "options": ["All shoppers", "Signed-in only"], "recommend": "Signed-in only"},
        "hand_goal": "Fix the broken links in the onboarding guide",
        "hand_report": "## Conclusion\nAll 6 broken links in the onboarding guide are fixed, and the link checker finds none left.",
        "hand_tasks": [
            {"id": "H1", "title": "Fix broken links", "assignee": "developer", "owns": ["docs/onboarding.md"],
             "instructions": "Find and fix the broken links in docs/onboarding.md.", "accept": ["The link checker reports no broken links"]},
            {"id": "H2", "title": "Verify the links", "assignee": "qa", "verifies": "H1",
             "instructions": "Check every link in the guide yourself.", "accept": ["Every link in the guide resolves"]},
        ],
        "hand_results": {
            "H1": {"summary": "Fixed 6 broken links in docs/onboarding.md.", "checks": [{"method": "command", "evidence": "lychee docs/onboarding.md: 23 OK, 0 errors"}],
                   "evidence": ["lychee docs/onboarding.md: 23 OK, 0 errors"]},
            "H2": {"summary": "All 23 links resolve.", "verdict": "approved", "checks": [{"method": "command", "evidence": "lychee docs/onboarding.md: 23 OK, 0 errors"}],
                   "evidence": ["lychee docs/onboarding.md: 23 OK, 0 errors"]},
        },
    },
    "zh": {
        "language": "zh-Hans",
        "roles": {},
        "brief": "Python 3.12，测试用 pytest。金额一律用 Decimal，不用 float。对外接口有变化时，在 docs/api.md 里补上说明。",
        "old_goal": "修复税额合计的舍入",
        "old_report": "## 结论\n税额现在逐行四舍五入后再求和，缺陷报告里的三张发票都算对了。",
        "goal": "给发票加上 CSV 导出，补上测试和接口文档",
        "report": ("## 结论\n发票可以导出成 CSV 了：`GET /invoices/export` 以流的方式返回文件，测试和接口文档都已补上，质量工程师在结果上做了独立验证。\n\n"
                   "## 需要你决定\n- 导出每次最多 10,000 行。保留这个上限，还是给更大的范围加分页？\n\n"
                   "## 做了什么\n\n- **T1** 研发工程师在 `ledger/export.py` 里加了 CSV 序列化，带单元测试。\n"
                   "- **T2** 研发工程师加了 `GET /invoices/export`，逐行输出，范围过大时返回 422。\n"
                   "- **T3** 研发工程师写了接口文档和更新记录。\n"
                   "- **V1** 质量工程师跑了全部测试，并把一次真实导出和数据库逐行比对。\n"),
        "tasks": [
            {"id": "T1", "title": "发票的 CSV 序列化", "assignee": "developer", "owns": ["ledger/export.py", "tests/test_export.py"],
             "instructions": "新增 ledger/export.py，把发票转成 CSV 行；写成生成器，接口可以直接流式输出。",
             "accept": ["金额保留两位小数并带币种", "含逗号、引号或换行的字段正确转义"]},
            {"id": "T2", "title": "GET /invoices/export 接口", "assignee": "developer", "deps": ["T1"], "owns": ["ledger/api.py", "tests/test_api.py"],
             "instructions": "按日期范围通过 HTTP 导出发票。流式输出；范围过大时拒绝。",
             "accept": ["逐行输出，不在内存里拼出整个文件", "超过 10,000 行的范围返回 422"]},
            {"id": "T3", "title": "接口文档和更新记录", "assignee": "developer", "deps": ["T2"], "owns": ["docs/api.md", "CHANGELOG.md"],
             "instructions": "在 docs/api.md 里写明新接口，并在更新记录里加一条。",
             "accept": ["docs/api.md 写明参数、各列和 422 错误", "CHANGELOG.md 的 Unreleased 下有一条记录"]},
            {"id": "V1", "title": "独立验证导出", "assignee": "qa", "verifies": "T2",
             "instructions": "自己验证导出：跑全部测试，再拿一次真实导出和数据比对。",
             "accept": ["全部测试通过", "导出的文件列和金额都正确"]},
        ],
        "results": {
            "T1": {"thought": "接口要流式输出，所以这里用 csv 模块写成生成器，不拼一个大字符串。",
                   "plan": ["读 ledger/models.py", "写 ledger/export.py", "补单元测试", "运行测试"],
                   "summary": "新增 `ledger/export.py`：`invoice_rows()` 从任意一组发票逐行生成 CSV，第一行是表头。",
                   "handoff": "调用 `invoice_rows(invoices)`。它是生成器，接口可以直接交给流式响应。",
                   "checks": [{"method": "test", "evidence": "pytest tests/test_export.py::test_amounts_keep_two_decimals：通过"},
                              {"method": "test", "evidence": "pytest tests/test_export.py::test_fields_are_escaped：通过"}],
                   "evidence": ["pytest tests/test_export.py -q：2 passed in 0.08s"]},
            "T2": {"thought": "先数行数，范围过大时在输出第一行之前就拒绝。",
                   "plan": ["读 ledger/api.py", "加导出路由", "补接口测试", "运行测试"],
                   "summary": "`GET /invoices/export?start=&end=` 流式返回 CSV，超过 10,000 行时返回 422。",
                   "handoff": "上限写在 ledger/api.py 里，422 的响应里写明了上限。",
                   "checks": [{"method": "inspection", "evidence": "处理函数返回 StreamingResponse(invoice_rows(...))，没有把行收集成列表"},
                              {"method": "test", "evidence": "pytest tests/test_api.py::test_export_cap：通过"}],
                   "evidence": ["pytest tests/test_api.py -q：3 passed in 0.41s"],
                   "risks": ["10,000 行的上限是估的，大客户可能需要分页"]},
            "T3": {"thought": "文档要列出各列，更新记录加一行。",
                   "plan": ["读 docs/api.md", "写 GET /invoices/export 的说明", "加更新记录", "检查链接"],
                   "summary": "在 docs/api.md 里写明了 `GET /invoices/export`，并加了一条更新记录。",
                   "checks": [{"method": "inspection", "evidence": "读了新增的一节：两个参数、各列和 422 的响应都写了"},
                              {"method": "command", "evidence": "git diff --stat CHANGELOG.md：Unreleased 下新增 1 行"}],
                   "evidence": ["markdownlint docs/api.md：没有问题"],
                   "command": "markdownlint docs/api.md CHANGELOG.md"},
            "V1": {"thought": "在干净的目录里跑全部测试，再拿一次真实导出和数据库比对。",
                   "plan": ["运行全部测试", "从有数据的库里导出 10 月", "和数据库里的行比对"],
                   "summary": "独立验证通过：测试全部通过，10 月的导出和数据库逐行一致。", "verdict": "approved",
                   "checks": [{"method": "test", "evidence": "pytest -q：41 passed in 3.2s"},
                              {"method": "command", "evidence": "curl -s 'localhost:8000/invoices/export?start=2026-10-01&end=2026-10-31' | head -3：表头加 INV-2031 1280.00 EUR 和 INV-2032 96.50 EUR，与数据库一致"}],
                   "evidence": ["pytest -q：41 passed in 3.2s"],
                   "command": "pytest -q"},
        },
        "memories": [
            {"kind": "howto", "title": "怎么跑测试", "hook": "uv run pytest -q；接口测试要设 LEDGER_DB=:memory:", "body": "运行 `uv run pytest -q`。接口测试读 `LEDGER_DB`，设成 `:memory:`。"},
            {"kind": "pitfall", "title": "金额用 Decimal", "hook": "用 float 会让舍入测试失败", "body": "金额用 Decimal('...') 构造，中间任何一步出现 float，tests/test_tax.py 就会失败。"},
        ],
        "loops": [
            ("Issue 分拣", "看看 issue 里有没有新的缺陷报告。\n一句话能答的直接答，真的缺陷派成任务。", "0 * * * *",
             "2 个新 issue：#412 已回答，#415 派成了任务。"),
            ("依赖巡检", "对照最新的安全公告检查锁文件。", "0 9 * * 1-5", "今天没有涉及锁文件的安全公告。"),
        ],
        "dispatch": "修复一张发票有两行税时导出重复行（#415）",
        "store_goal": "把新结账页放到功能开关后面，先放量 5%",
        "store_task": {"id": "S1", "title": "功能开关和放量配置", "assignee": "developer",
                       "instructions": "加上 checkout_v2 开关，放量 5%，推送分支等待评审。",
                       "accept": ["config/flags.yaml 里 checkout_v2 是 5%"]},
        "store_ask": {"question": "放量给全部顾客的 5%，还是只给已登录顾客？",
                      "why": "这决定谁先看到新结账页，只有你知道上线计划。",
                      "options": ["全部顾客", "只给已登录顾客"], "recommend": "只给已登录顾客"},
        "hand_goal": "修好入职指南里的失效链接",
        "hand_report": "## 结论\n入职指南里 6 个失效链接都修好了，链接检查没有再报错。",
        "hand_tasks": [
            {"id": "H1", "title": "修复失效链接", "assignee": "developer", "owns": ["docs/onboarding.md"],
             "instructions": "找出并修好 docs/onboarding.md 里的失效链接。", "accept": ["链接检查不再报失效链接"]},
            {"id": "H2", "title": "验证链接", "assignee": "qa", "verifies": "H1",
             "instructions": "自己把指南里的每个链接检查一遍。", "accept": ["指南里的每个链接都能打开"]},
        ],
        "hand_results": {
            "H1": {"summary": "修好了 docs/onboarding.md 里的 6 个失效链接。", "checks": [{"method": "command", "evidence": "lychee docs/onboarding.md：23 OK，0 errors"}],
                   "evidence": ["lychee docs/onboarding.md：23 OK，0 errors"]},
            "H2": {"summary": "23 个链接都能打开。", "verdict": "approved", "checks": [{"method": "command", "evidence": "lychee docs/onboarding.md：23 OK，0 errors"}],
                   "evidence": ["lychee docs/onboarding.md：23 OK，0 errors"]},
        },
    },
}


def content(t):
    """What the scripted agent says and writes (`content.json`, backend/testdata/fake_acp.py)."""
    tasks = {}
    for tid, r in {**t["results"], **t["hand_results"]}.items():
        d = dict(r, files=FILES.get(tid, {}), tokens=TOKENS[tid], context=CONTEXT[tid], cost=COST[tid], seconds=SECONDS.get(tid, 2))
        command = d.pop("command", None)
        if command:
            d["commands"] = [{"title": command, "output": r["evidence"][0]}]
        tasks[tid] = d
    # T3 and V1 stay at work until their sessions are pictured.
    tasks["T3"]["hold"] = "hold-T3"
    tasks["V1"]["hold"] = "hold-V1"
    # The storefront's subtask asks before it pushes, and waits for the answer.
    tasks["S1"] = {"files": FILES["S1"], "permission": "git push origin checkout-v2-flag", "tokens": TOKENS["S1"], "context": CONTEXT["S1"], "cost": COST["S1"]}
    return {
        "tasks": tasks,
        "goals": {
            t["old_goal"]: {"key": "old", "report": t["old_report"], "tokens": 14200, "cost": 0.08},
            t["goal"]: {"key": "main", "report": t["report"], "tokens": 23800, "cost": 0.14, "context": 37000},
            t["hand_goal"]: {"key": "hand", "report": t["hand_report"], "tokens": 8600, "cost": 0.05},
        },
        "loops": {prompt.splitlines()[0]: reply for _, prompt, _, reply in t["loops"]},
        "loop_title": "Loop",
        "loop_default": t["loops"][0][3],
    }


def stage(lang, server_bin, app_bin, node, raw):
    """Stage the data for `lang` on a scratch Janus home and picture the pages into `raw`."""
    t = TEXTS[lang]
    if os.path.exists(BASE):
        if not os.path.exists(MARK):
            raise Failure(f"{BASE} 已经存在，而且不是这个脚本留下的；先把它移走")
        shutil.rmtree(BASE)
    home, host, logs = (os.path.join(BASE, d) for d in ("janus", "host", "logs"))
    for d in (home, host, logs):
        os.makedirs(d)
    open(MARK, "w").close()
    stand_in_adapters(home, sys.executable)
    addr = free_addr()
    env = {"JANUS_NODE": node, "npm_config_registry": Registry().url, "JANUS_MAX_RUNNING_RUNS": "32", "JANUS_HOST_HOME": host}
    server = Server(server_bin, home, os.path.join(logs, "janus.log"), addr=addr, env=env)
    app = None
    try:
        server.start()

        def api(method, path, body=None):
            return http(addr, method, path, body)

        fake = Fake(os.path.join(BASE, "agent"))
        given = content(t)
        fake.touch("content.json", json.dumps(given, ensure_ascii=False))
        fake.script({tid: ["content"] for tid in given["tasks"]})

        # Agents, checked the way Settings does; the default team, bound to them.
        envtext = f"FAKE_DIR={fake.dir}"
        api("POST", "/api/setup/connections", {"id": "codex", "kind": "codex", "name": "Codex", "auth": "apikey", "apiKey": "sk-demo-not-a-key", "envText": envtext})
        api("POST", "/api/setup/connections", {"id": "claude", "kind": "claude", "name": "Claude Code", "auth": "apikey",
                                               "envText": envtext + "\nexport ANTHROPIC_AUTH_TOKEN=demo-not-a-token"})
        for c in ("codex", "claude"):
            api("POST", f"/api/connections/{c}/test")
        wait(lambda: all(c.get("phase") == "ok" for c in api("GET", "/api/setup")["connections"]), "两个 Agent 的配置检查通过", 90)
        roles = {r["id"]: r for r in api("GET", "/api/setup")["roles"]}
        for rid, conn in (("pmo", "codex"), ("developer", "claude"), ("qa", "claude")):
            r = dict(roles[rid], connection=conn, lead=rid == "pmo")
            if rid in t["roles"]:
                r["name"], r["desc"] = t["roles"][rid]
            api("POST", "/api/setup/roles", r)
        members = [{"role": "pmo"}, {"role": "developer"}, {"role": "qa"}]
        for name, files, brief in (("ledger", LEDGER_FILES, t["brief"]),
                                   ("storefront", {"README.md": "# storefront\n", "config/flags.yaml": "{}\n"}, ""),
                                   ("handbook", {"README.md": "# handbook\n", "docs/onboarding.md": "# Onboarding\n"}, "")):
            api("POST", "/api/setup/workspaces", {"id": name, "name": name, "path": git_repo(os.path.join(BASE, name), files),
                                                  "lead": "pmo", "brief": brief, "members": members})

        def events(run):
            return api("GET", f"/api/runs/{run}")["events"]

        def start(workspace, goal):
            run = api("POST", "/api/runs", {"goal": goal, "workspace": workspace})["id"]
            fake.wait_call("goal " + goal, 90)
            return run

        def lead(run, tool, args):
            text, err = fake.board(run, "lead", 90).call(tool, args)
            if err:
                raise Failure(f"{tool} 被拒绝：{text}")

        def done(run, *tasks):
            return {e.get("task") for e in events(run) if e["type"] == "task.succeeded"} >= set(tasks)

        def deliver(run, key):
            fake.touch("go-" + key)
            wait(lambda: any(e["type"] == "run.delivered" for e in events(run)), f"{key} 交付", 120)

        # An earlier run, accepted: the workspace's history.
        old = start("ledger", t["old_goal"])
        deliver(old, "old")
        api("POST", f"/api/runs/{old}/actions", {"type": "accept"})
        # The handbook: a delivery with nothing to look at.
        hand = start("handbook", t["hand_goal"])
        lead(hand, "orchestrate", {"tasks": t["hand_tasks"]})
        wait(lambda: done(hand, "H1", "H2"), "handbook 的子任务完成", 120)
        deliver(hand, "hand")
        # The storefront: an approval and a question from the lead, both waiting for the person.
        store = start("storefront", t["store_goal"])
        lead(store, "orchestrate", {"tasks": [t["store_task"]]})
        lead(store, "ask_user", t["store_ask"])
        wait(lambda: any(e["type"] == "approval.requested" for e in events(store)), "storefront 的审批请求", 90)

        app = App(app_bin, home, addr, raw, os.path.join(logs, "app.log"), settings={"janus.language": t["language"]})
        app.until(lambda s: s.get("connections") is not None, "应用连上服务", timeout=90)
        # The window keeps the size of its last launch.
        app.do("size 1000x800")

        def shoot(page, *commands):
            app.do("reload", *commands)
            time.sleep(1.5)
            for look in ("dark", "light"):
                app.do(f"appearance {look}", *commands)
                time.sleep(1.0)
                app.do(f"shot {page}-{lang}-{look}")
            say(f"  {lang}：{page}")

        # The main run: T3 and V1 are pictured at work, then it is delivered.
        main_run = start("ledger", t["goal"])
        lead(main_run, "orchestrate", {"tasks": t["tasks"]})
        for memory in t["memories"]:
            lead(main_run, "remember", memory)
        wait(lambda: done(main_run, "T2"), "T2 完成", 180)
        wait(lambda: {e.get("task") for e in events(main_run) if e["type"] == "tool.started"} >= {"T3", "V1"}, "T3 和 V1 在干活", 120)
        time.sleep(2)
        shoot("session", f"open run:{main_run}", "page task:V1")
        fake.touch("hold-T3")
        fake.touch("hold-V1")
        wait(lambda: done(main_run, "T3", "V1"), "T3 和 V1 完成", 120)
        deliver(main_run, "main")

        # Two Loops in the ledger workspace; the first has dispatched a run that is at work.
        tz = os.path.realpath("/etc/localtime").split("zoneinfo/")[-1]
        for i, (name, prompt, cron, _) in enumerate(t["loops"]):
            made = api("POST", "/api/workspaces/ledger/loops", {"name": name, "prompt": prompt, "schedule": {"cron": cron, "tz": tz}, "wakeNow": True})
            if i == 0:
                lead(made["run"], "dispatch", {"goal": t["dispatch"], "key": "github:acme/ledger#415", "source_url": "https://github.com/acme/ledger/issues/415"})
        time.sleep(3)

        shoot("delivery", f"open run:{main_run}", "page board")
        shoot("workspace", "open home:ledger")
        shoot("inbox", "open inbox")
    finally:
        if app:
            app.quit()
        server.stop()


def publish(raw, out, lang):
    """The pictures of `lang` in `raw`, converted to sRGB and made small, into `out`."""
    for page in ("inbox", "delivery", "session", "workspace"):
        for look in ("light", "dark"):
            name = f"{page}-{lang}-{look}.png"
            cmd = ["magick", os.path.join(raw, name), "-profile", SRGB]
            rows = CUT.get((page, lang))
            if rows:
                # Kept in full colour: 256 colours would turn the fade into stripes.
                cmd += ["-crop", f"2000x{rows}+0+0", "+repage", "-write", "mpr:picture", "-alpha", "extract",
                        "(", "-size", f"2000x{rows - 40}", "xc:white", "-size", "2000x40", "gradient:white-black", "-append", ")",
                        "-compose", "Multiply", "-composite", "-write", "mpr:mask", "+delete",
                        "mpr:picture", "mpr:mask", "-alpha", "off", "-compose", "CopyOpacity", "-composite", "-strip"]
            else:
                cmd += ["-strip", "-dither", "None", "-colors", "256"]
            done = subprocess.run(cmd + ["-define", "png:compression-level=9", os.path.join(out, name)], capture_output=True, text=True)
            if done.returncode != 0:
                raise Failure(f"magick 处理 {name} 失败：{done.stderr.strip()[-400:]}")


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--lang", action="append", choices=sorted(TEXTS), help="only these languages (repeatable)")
    p.add_argument("--out", default=os.path.join(ROOT, "docs", "assets", "screenshots"), help="where the pictures go")
    p.add_argument("--keep", action="store_true", help=f"keep {BASE} (the last language's home, workspaces and logs)")
    a = p.parse_args()
    if not shutil.which("magick"):
        sys.exit("需要 ImageMagick（brew install imagemagick）：截图用它转成 sRGB 并压缩")
    out = os.path.abspath(a.out)
    os.makedirs(out, exist_ok=True)
    server_bin, app_bin, node = build(False)
    try:
        for lang in a.lang or sorted(TEXTS):
            say(f"\n== {lang}")
            raw = os.path.join(BASE, "shots")
            stage(lang, server_bin, app_bin, node, raw)
            publish(raw, out, lang)
    except Failure as e:
        sys.exit(f"\n✗ {e}")
    finally:
        if not a.keep and os.path.exists(MARK):
            shutil.rmtree(BASE, ignore_errors=True)
    say(f"\n截图在 {out}；提交前逐张看一遍")


if __name__ == "__main__":
    main()
