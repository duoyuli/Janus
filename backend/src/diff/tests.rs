//! The app's diff tests (`DiffTests`, `InlineDiff.lines`, `ChatTurns.lineCounts`), moved here
//! with the code.

use super::*;

const SAMPLE: &str = "diff --git a/pay.js b/pay.js
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
";

fn kinds(f: &File) -> Vec<LineKind> {
    f.hunks[0].lines.iter().map(|l| l.kind).collect()
}

/// Janus's own diff artifact, file by file: hunks, both sides' line numbers, and the untracked
/// files listed after it.
#[test]
fn diffs_are_read_file_by_file_with_line_numbers() {
    let files = parse(SAMPLE);
    assert_eq!(files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["pay.js", "README.md", "notes.md"]);
    let pay = &files[0];
    assert_eq!((pay.added, pay.removed, pay.hunks.len()), (2, 1, 1));
    use LineKind::*;
    assert_eq!(kinds(pay), [Context, Removed, Added, Added, Context]);
    let l = &pay.hunks[0].lines;
    assert_eq!((l[1].old, l[1].new), (Some(2), None));
    assert_eq!((l[2].new, l[3].new, l[4].old, l[4].new), (Some(2), Some(3), Some(3), Some(4)));
    assert!(files[1].added == 1 && !files[1].untracked);
    assert!(files[2].untracked && files[2].hunks.is_empty());
}

/// An untracked file's content is part of the diff; the trailing list only marks it as new.
#[test]
fn an_untracked_files_content_is_kept_and_marked() {
    let files = parse(
        "diff --git a/notes.md b/notes.md\nnew file mode 100644\n--- /dev/null\n+++ b/notes.md\n@@ -0,0 +1,2 @@\n+# 笔记\n+内容\n\n# 未跟踪的新文件\nnotes.md\n",
    );
    assert_eq!(files.len(), 1);
    let notes = &files[0];
    assert!(notes.untracked && notes.added == 2);
    let l = &notes.hunks[0].lines;
    assert_eq!(l.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["# 笔记", "内容"]);
    assert_eq!(l.iter().map(|l| l.new).collect::<Vec<_>>(), [Some(1), Some(2)]);
}

/// A deleted tracked file keeps its path and its removed lines.
#[test]
fn a_deleted_file_keeps_its_path_and_removed_lines() {
    let files = parse(
        "diff --git a/old.md b/old.md\ndeleted file mode 100644\nindex 1111111..0000000\n--- a/old.md\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-# 旧内容\n-正文",
    );
    assert_eq!(files.len(), 1);
    let old = &files[0];
    assert!(old.path == "old.md" && !old.untracked);
    assert_eq!((old.added, old.removed), (0, 2));
    assert_eq!(kinds(old), [LineKind::Removed, LineKind::Removed]);
    assert_eq!(
        old.hunks[0].lines.iter().map(|l| (l.old, l.new)).collect::<Vec<_>>(),
        [(Some(1), None), (Some(2), None)]
    );
}

/// The comments of a review become one message, in the run's language, by file and line.
#[test]
fn review_comments_become_one_message() {
    let files = parse(SAMPLE);
    let l = &files[0].hunks[0].lines;
    let comments = [
        Comment { line: l[2].id, text: "加上错误处理".into() },
        Comment { line: l[1].id, text: "旧接口还有人用吗".into() },
    ];
    let text = message(Lang::Zh, Some("T1"), "T1-a1.diff", &files, &comments, "整体可以");
    assert!(text.starts_with("对T1 的改动（T1-a1.diff）"), "{text}");
    assert!(text.contains("总体意见：整体可以"));
    assert!(text.contains("pay.js 第 2 行：`+return sdk.pay()`\n  加上错误处理"), "{text}");
    assert!(text.contains("原第 2 行（已删除）"));
    assert!(text.find("旧接口").unwrap() < text.find("错误处理").unwrap(), "in the diff's order");
    assert!(message(Lang::Zh, None, "run-1.diff", &files, &[], "好").starts_with("对整个任务的改动（run-1.diff）"));
    let en = message(Lang::En, Some("T1"), "T1-a1.diff", &files, &comments, "");
    assert!(en.contains("pay.js line 2: `+return sdk.pay()`"), "{en}");
}

fn marks(old: &str, new: &str) -> Vec<String> {
    inline(old, new).into_iter().map(|m| format!("{}{}", m.mark, m.text)).collect()
}

/// An edit shows what it removed and added, with three lines of context, the rest left out.
#[test]
fn an_edit_shows_its_lines_with_context() {
    assert_eq!(marks("a\nb\nc", "a\nB\nc\nd"), [" a", "-b", "+B", " c", "+d"]);
    let old: Vec<String> = (1..=20).map(|i| i.to_string()).collect();
    let mut new = old.clone();
    new[10] = "x".into();
    assert_eq!(marks(&old.join("\n"), &new.join("\n")), [" 8", " 9", " 10", "-11", "+x", " 12", " 13", " 14"]);
}

/// Lines added and removed over an edit, a new file counting every line.
#[test]
fn line_counts_are_the_shortest_edit() {
    assert_eq!(line_counts("a\nb", "a\nc\nd"), (2, 1));
    assert_eq!(line_counts("", "x"), (1, 0));
    assert_eq!(line_counts("a\nb\nc", "a\nb\nc"), (0, 0));
    assert_eq!(line_counts("a\nb\nc\nd", "b\nd\ne"), (1, 2));
}
