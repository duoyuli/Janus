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

@Test func reviewCommentsBecomeOneMessage() {
    let l = Diff.parse(sample)[0].hunks[0].lines
    let text = Diff.message(task: "T1", artifact: "T1-a1.diff", comments: [
        Diff.Comment(file: "pay.js", line: l[2], text: "加上错误处理"),
        Diff.Comment(file: "pay.js", line: l[1], text: "旧接口还有人用吗"),
    ], summary: "整体可以")
    #expect(text.contains("T1 的改动（T1-a1.diff）") && text.contains("总体意见：整体可以"))
    #expect(text.contains("pay.js 第 2 行：`+return sdk.pay()`\n  加上错误处理"))
    #expect(text.contains("原第 2 行（已删除）"))
}

/// Only what is new since the last look is announced, and nothing on the first look.
@Test func notificationsAreForWhatIsNew() {
    func e(_ k: String) -> InboxEntry { InboxEntry(key: k, run: "r", goal: "", workspace: "", title: k, text: "", sub: "", ts: 0, canWait: true, canSettle: false) }
    #expect(Notifier.fresh([e("a"), e("b")], seen: nil).isEmpty)
    #expect(Notifier.fresh([e("a"), e("b")], seen: ["a"]).map(\.key) == ["b"])
}
