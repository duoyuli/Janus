//! What tests build an `App` from: a fresh home with the stand-in adapters installed.

use super::*;

/// Codex and Claude Code adapters that run the scripted agent (`testdata/fake_acp.py`, driven by
/// the `FAKE_DIR` its connection sets) instead of the real one. Janus starts a runtime as
/// `<node> <bin of the adapter package>` from `cache/adapters/<id>/current`; tests run Python in
/// place of Node, so the bin is the script itself and the launch path is the real one.
// @project-doc docs/operations/20261031-testing.md#fake_agent
fn stand_in_adapters(layout: &crate::layout::Layout) {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/fake_acp.py");
    for a in crate::adapters::ADAPTERS {
        let dir = layout.adapters().join(a.id);
        let pkg = dir.join("9.9.9").join("node_modules").join(a.package);
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join("package.json"), json!({ "name": a.package, "bin": "fake_acp.py" }).to_string())
            .unwrap();
        let _ = std::os::unix::fs::symlink(&script, pkg.join("fake_acp.py"));
        let _ = std::os::unix::fs::symlink("9.9.9", dir.join("current"));
    }
}

/// An `App` over a fresh database in `dir`, for tests, with the stand-in adapters installed.
pub(crate) fn test_app(dir: &Path) -> Arc<App> {
    test_app_with(dir, |_| {})
}

/// `test_app` with some of its settings changed by `set`.
pub(crate) fn test_app_with(dir: &Path, set: impl FnOnce(&mut Config)) -> Arc<App> {
    std::fs::create_dir_all(dir).unwrap();
    let layout = crate::layout::Layout::new(dir);
    stand_in_adapters(&layout);
    let mut cfg = Config {
        layout,
        addr: "127.0.0.1:7878".into(),
        // The stand-in adapters are Python scripts: see `stand_in_adapters`.
        node: "python3".into(),
        stall_after: STALL_AFTER,
        goal_settle: crate::goal_settle::SETTLE,
        // Tests retry at once; the one about the pause sets it.
        retry_after: Duration::ZERO,
        max_concurrent_attempts: crate::config::DEFAULT_MAX_CONCURRENT_ATTEMPTS,
        max_running_runs: crate::config::DEFAULT_MAX_RUNNING_RUNS,
        max_agent_sessions: crate::config::DEFAULT_MAX_AGENT_SESSIONS,
        idle_after: IDLE_AFTER,
        run_token_budget: None,
        host_home: dir.join("host-home"),
    };
    set(&mut cfg);
    App::new(Db::open(&cfg.layout).unwrap().0, cfg)
}
