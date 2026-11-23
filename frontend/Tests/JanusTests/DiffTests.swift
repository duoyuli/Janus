import Foundation
import Testing
@testable import Janus

private let sample = """
diff --git a/pay.js b/pay.js
index 1111111..2222222 100644
--- a/pay.js
+++ b/pay.js
@@ -1,3 +1,4 @@
 export function pay() {
-  return old()
+  return sdk.pay()
+  // new SDK
 }
\\ No newline at end of file
diff --git a/README.md b/README.md
new file mode 100644
--- /dev/null
+++ b/README.md
@@ -0,0 +1 @@
+# 商城

# 未跟踪的新文件
notes.md
"""

/// The newer form: an untracked file's diff comes before the list that names it.
private let withUntrackedContent = """
diff --git a/notes.md b/notes.md
new file mode 100644
--- /dev/null
+++ b/notes.md
@@ -0,0 +1,2 @@
+# 笔记
+内容

# 未跟踪的新文件
notes.md
"""

/// The review view reads Janus's own diff artifact: files, hunks, both sides' line numbers, and
/// the untracked files listed after it.
@Test func diffsAreReadFileByFileWithLineNumbers() {
    let files = Diff.parse(sample)
    #expect(files.map(\.path) == ["pay.js", "README.md", "notes.md"])
    let pay = files[0]
    #expect(pay.added == 2 && pay.removed == 1 && pay.hunks.count == 1)
    let lines = pay.hunks[0].lines
    #expect(lines.map(\.kind) == [.context, .removed, .added, .added, .context])
    #expect(lines[1].old == 2 && lines[1].new == nil)
    #expect(lines[2].new == 2 && lines[3].new == 3 && lines[4].old == 3 && lines[4].new == 4)
    #expect(files[1].added == 1 && !files[1].untracked)
    #expect(files[2].untracked && files[2].hunks.isEmpty)
}

/// An untracked file's content is part of the diff; the trailing list only marks it as new.
@Test func anUntrackedFilesContentIsKeptAndMarked() {
    let files = Diff.parse(withUntrackedContent)
    #expect(files.map(\.path) == ["notes.md"])
    let notes = files[0]
    #expect(notes.untracked && notes.added == 2)
    #expect(notes.hunks[0].lines.map(\.text) == ["# 笔记", "内容"])
    #expect(notes.hunks[0].lines.map(\.new) == [1, 2])
}

/// A deleted tracked file keeps its path and its removed lines; the new-file handling does not
/// touch it.
@Test func aDeletedFileKeepsItsPathAndRemovedLines() {
    let text = """
    diff --git a/old.md b/old.md
    deleted file mode 100644
    index 1111111..0000000
    --- a/old.md
    +++ /dev/null
    @@ -1,2 +0,0 @@
    -# 旧内容
    -正文
    """
    let files = Diff.parse(text)
    #expect(files.count == 1)
    let old = files[0]
    #expect(old.path == "old.md" && !old.untracked)
    #expect(old.added == 0 && old.removed == 2)
    #expect(old.hunks[0].lines.map(\.kind) == [.removed, .removed])
    #expect(old.hunks[0].lines.map(\.old) == [1, 2])
    #expect(old.hunks[0].lines.map(\.new) == [nil, nil])
}

@Test func reviewCommentsBecomeOneMessage() {
    let l = Diff.parse(sample)[0].hunks[0].lines
    let text = Diff.message(subject: "T1 的改动", artifact: "T1-a1.diff", comments: [
        Diff.Comment(file: "pay.js", line: l[2], text: "加上错误处理"),
        Diff.Comment(file: "pay.js", line: l[1], text: "旧接口还有人用吗"),
    ], summary: "整体可以")
    #expect(text.contains("T1 的改动（T1-a1.diff）") && text.contains("总体意见：整体可以"))
    #expect(text.contains("pay.js 第 2 行：`+return sdk.pay()`\n  加上错误处理"))
    #expect(text.contains("原第 2 行（已删除）"))
    let run = Artifact(name: "run-1.diff", kind: "run_diff", path: "", desc: "", hash: "", bytes: nil, task: "", attempt: 0, seq: 1, by: "janus")
    #expect(run.isDiff && run.ofRun && run.subject == "整个任务的改动")
    #expect(Diff.message(subject: run.subject, artifact: run.name, comments: [], summary: "好").hasPrefix("对整个任务的改动（run-1.diff）"))
}

/// Only what is new since the last look is announced, and nothing on the first look.
@Test func notificationsAreForWhatIsNew() {
    func e(_ k: String) -> InboxEntry { InboxEntry(key: k, run: "r", goal: "", workspace: "", title: k, text: "", sub: "", ts: 0, canWait: true, canSettle: false) }
    #expect(Notifier.fresh([e("a"), e("b")], seen: nil).isEmpty)
    #expect(Notifier.fresh([e("a"), e("b")], seen: ["a"]).map(\.key) == ["b"])
}
