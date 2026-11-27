mod acp;
mod acp_view;
mod adapters;
mod agent_records;
mod agents;
mod attachments;
mod bill;
mod coderun;
mod config;
mod data_shape;
mod db;
mod devices;
mod diff;
mod events;
mod evidence;
mod front_matter;
mod git;
mod goal_settle;
mod host_env;
mod http;
mod lang;
mod layout;
mod library;
mod migrations;
mod orchestrator;
mod prices;
mod run_ops;
mod run_view;
mod schedule;
mod seats;
mod status;
mod teamwork;
mod text;
mod traces;
mod usage;
mod util;
mod wire;
mod worktrees;

use anyhow::Result;
use orchestrator::{App, Config};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Janus home (~/.janus): every user-level thing Janus keeps. Owner-only,
/// since it holds credentials.
fn janus_home() -> Result<PathBuf> {
    let dir = config::settings().home.clone();
    if dir.as_os_str().is_empty() {
        anyhow::bail!("找不到用户目录：请设置 HOME 或 JANUS_HOME");
    }
    std::fs::create_dir_all(&dir)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

/// One Janus per home: a second one would mark the first one's running runs as interrupted on
/// startup (`recover`) and drive the same database. The lock is an exclusive OS lock on
/// `<home>/janus.pid`, taken atomically and released when the process ends however it ends; the
/// file also names the owner's pid, which the app reads to find a running Janus. Keep the
/// returned file for as long as the process runs.
// @project-doc docs/operations/20261031-deployment.md#single_instance
fn lock_home(layout: &layout::Layout) -> Result<std::fs::File> {
    use std::io::Write;
    let pid = layout.pid_file();
    let mut file = std::fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(&pid)?;
    match file.try_lock() {
        Ok(()) => {}
        Err(std::fs::TryLockError::WouldBlock) => {
            let owner = std::fs::read_to_string(&pid).unwrap_or_default();
            anyhow::bail!(
                "另一个 Janus（pid {}）正在使用 {}。先停止它（以服务方式运行时执行 just stop），或用另一个 JANUS_HOME 启动",
                owner.trim(),
                layout.home().display()
            );
        }
        Err(std::fs::TryLockError::Error(err)) => return Err(err.into()),
    }
    file.set_len(0)?;
    write!(file, "{}", std::process::id())?;
    Ok(file)
}

/// The release this build is: the date version a package build sets (`JANUS_VERSION`), else
/// the crate version. The app compares it with its own to tell a stale service.
pub const VERSION: &str = match option_env!("JANUS_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

// @project-doc docs/architecture/20261116-modules.md#startup
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        // Hidden: the MCP router's code mode runs each script in this, before anything else starts.
        Some(coderun::COMMAND) => std::process::exit(coderun::main()),
        Some("--version" | "-V") => {
            println!("janus {VERSION}");
            return Ok(());
        }
        Some("migrate") => {
            match args.next().as_deref() {
                Some("--dry-run") => {
                    print!("{}", migrations::dry_run(&migrations::current_db(&layout::Layout::new(janus_home()?)))?)
                }
                Some("--shape") => println!("{}", serde_json::to_string_pretty(&data_shape::snapshot()?)?),
                _ => anyhow::bail!(
                    "janus migrate --dry-run 预览这份数据升级到数据版本 {} 会改什么（不修改数据）；janus migrate --shape 输出当前的数据结构快照。升级本身在服务启动时自动执行。",
                    migrations::latest()
                ),
            }
            return Ok(());
        }
        Some("--help" | "-h") => {
            println!(
                "janus {}: local multi-agent workspace. Run without arguments to start the server.\n`janus migrate --dry-run` previews the data upgrade without changing anything.\n\nSettings (environment, else ~/.janus/config.toml): JANUS_HOME, JANUS_ADDR (addr), JANUS_NODE (node), JANUS_MAX_CONCURRENT_ATTEMPTS (max_concurrent_attempts), JANUS_MAX_RUNNING_RUNS (max_running_runs), JANUS_RUN_TOKEN_BUDGET (run_token_budget).\nSee docs/operations/20261031-deployment.md.",
                VERSION
            );
            return Ok(());
        }
        Some(other) => anyhow::bail!("unknown argument {other}; see janus --help"),
        None => {}
    }
    let layout = layout::Layout::new(janus_home()?);
    let _home_lock = lock_home(&layout)?;
    // Nothing runs yet that could be using the scratch of the last run.
    let _ = std::fs::remove_dir_all(layout.tmp());
    migrations::adopt_db(&layout)?;
    let (db, upgrade) = db::Db::open(&layout)?;
    if !upgrade.applied.is_empty() {
        let backup = upgrade.backup.as_ref().map(|b| format!("，升级前的备份：{}", b.display())).unwrap_or_default();
        config::log(format!("数据从版本 {} 升级到 {}{backup}", upgrade.from, upgrade.to));
        for (version, name, changed) in &upgrade.applied {
            config::log(format!("  {version}「{name}」：改动 {changed} 处"));
        }
    }
    let foreign = layout.foreign();
    if !foreign.is_empty() {
        config::log(format!("Janus home 中这些不是 Janus 的：{}（Janus 不读也不删它们）", foreign.join("、")));
    }
    let settings = config::settings();
    let app = App::new(
        db,
        Config {
            layout,
            addr: settings.addr.clone(),
            node: settings.node.clone(),
            stall_after: orchestrator::STALL_AFTER,
            goal_settle: goal_settle::SETTLE,
            retry_after: orchestrator::RETRY_AFTER,
            max_concurrent_attempts: settings.max_concurrent_attempts,
            max_running_runs: settings.max_running_runs,
            max_agent_sessions: settings.max_agent_sessions,
            idle_after: orchestrator::IDLE_AFTER,
            run_token_budget: settings.run_token_budget,
            host_home: settings.host_home.clone(),
        },
    );
    app.recover()?;
    app.sync_seeds()?;
    app.reset_stale_checks()?;
    // Library sources: ask their remotes for news shortly after start, then every few hours.
    periodic(&app, "library sources not checked", Duration::from_secs(30), library::CHECK_EVERY, |app| async move {
        app.check_sources().await.map(drop)
    });
    // ACP adapters: ask npm for newer versions shortly after start, then every few hours, and
    // install what it finds without asking; a version the person pinned stays.
    // @project-doc docs/architecture/20261031-adapters.md#update_check
    periodic(
        &app,
        "adapter update check stopped",
        adapters::FIRST_CHECK_AFTER,
        adapters::CHECK_EVERY,
        |app| async move { Ok(tokio::task::spawn_blocking(move || app.check_adapter_updates()).await?) },
    );
    // Model prices: check every hour, download models.dev's list again once a day.
    periodic(&app, "model prices not synced", Duration::from_secs(60), prices::CHECK_EVERY, |app| async move {
        app.sync_prices(false).await.map(drop)
    });

    let addr = app.cfg.addr.clone();
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    // Only once nothing can stop the start: goals the restart broke off go on by themselves, and
    // runs queued before the restart start when slots are free, as they did before it.
    app.continue_after_restart();
    app.pump();
    // Each Loop wakes when it last said; one that missed its time while Janus was down wakes once now.
    app.start_loops()?;
    config::log(format!("janus {VERSION}: http://{addr}  ·  home {}", app.cfg.layout.home().display()));
    // Phones on the local network, if the person let them in (`devices`).
    http::lan::apply(&app);
    let service = http::router(app, http::Via::Local).into_make_service_with_connect_info::<std::net::SocketAddr>();
    axum::serve(listener, service).await?;
    Ok(())
}

/// Run `job` once `first` has passed, then every `every`, for as long as Janus runs. A round that
/// fails is logged (`what` says what did not happen) and the next one runs anyway.
fn periodic<F, Fut>(app: &Arc<App>, what: &'static str, first: Duration, every: Duration, job: F)
where
    F: Fn(Arc<App>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<()>> + Send,
{
    let app = app.clone();
    tokio::spawn(async move {
        tokio::time::sleep(first).await;
        loop {
            if let Err(err) = job(app.clone()).await {
                config::log(format!("janus: {what}: {err:#}"));
            }
            tokio::time::sleep(every).await;
        }
    });
}

#[cfg(test)]
mod lock_tests {
    /// A second holder of one home is refused while the first lives; the lock goes with its holder.
    #[test]
    fn one_janus_per_home() {
        let home = std::env::temp_dir().join(format!("janus-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        let home = crate::layout::Layout::new(home);
        let first = super::lock_home(&home).unwrap();
        assert_eq!(std::fs::read_to_string(home.pid_file()).unwrap(), std::process::id().to_string());
        let err = super::lock_home(&home).unwrap_err().to_string();
        assert!(err.contains("另一个 Janus"), "{err}");
        assert_eq!(
            std::fs::read_to_string(home.pid_file()).unwrap(),
            std::process::id().to_string(),
            "the refused one does not overwrite the owner"
        );
        drop(first);
        // Other tests spawn processes meanwhile: a child forked in that moment holds the descriptor
        // until its exec closes it, so the release can take an instant to show.
        let again = (0..100).find_map(|_| {
            super::lock_home(&home).ok().or_else(|| {
                std::thread::sleep(std::time::Duration::from_millis(20));
                None
            })
        });
        assert!(again.is_some(), "the lock is released with its holder");
        let _ = std::fs::remove_dir_all(home.home());
    }
}
