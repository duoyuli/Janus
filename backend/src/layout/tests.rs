//! The layout: what it recognizes.

use super::*;

fn temp_home(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("janus-layout-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    // Recorded paths are resolved ones (`/private/var/…` for `/var/…`), as a worktree's is.
    dir.canonicalize().unwrap()
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// Only artifacts are served and only stored attachments are sent; a path that leads out does
/// not count, and anything at the top of the home outside `TOP` is someone else's.
#[test]
fn it_recognizes_only_what_janus_stored() {
    let home = temp_home("recognize");
    let layout = Layout::new(&home);
    let run = layout.run(RunKind::Goal, "r1");
    let report = run.artifacts("T1").join("T1-a1-report.md");
    let image = layout.run(RunKind::Chat, "c1").attachments().join("1.png");
    write(&report, "done");
    write(&image, "png");
    write(&layout.db(), "");
    assert!(layout.is_artifact(&report));
    let whole = run.changes().join("run-0123456789ab.diff");
    write(&whole, "diff");
    assert!(layout.is_artifact(&whole), "the whole run's diff");
    assert!(!layout.is_artifact(&run.changes().join("../../../../janus.db")));
    assert!(!layout.is_artifact(&layout.db()), "the database is not an artifact");
    assert!(!layout.is_artifact(&run.artifacts("T1").join("../../../../janus.db")));
    assert!(layout.is_attachment(&image) && !layout.is_attachment(&report));
    write(&home.join("signing/cert.pem"), "");
    write(&home.join(".DS_Store"), "");
    assert_eq!(layout.foreign(), ["signing"]);
    let _ = std::fs::remove_dir_all(&home);
}
