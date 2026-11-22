//! Agent processes are a machine's memory: each ACP session's adapter and agent take a seat
//! (`seats`) while open, and an idle one gives its seat back. A session whose turn is over and
//! that nothing in its run needs (a chat, a Loop between wakes, a goal run done and not running
//! tasks) is parked: its agent process closes, and the next message or wake opens it again from
//! its log. That happens after `idle_after`, or at once, idlest first, when someone waits for a seat.
// @project-doc docs/domains/20261031-run_lifecycle.md#agent_seats

use super::*;
use crate::seats::Seat;

/// Why a session was parked.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Parked {
    /// It waited idle for `idle_after`.
    Idle,
    /// Every seat was taken and it was the idlest.
    Full,
}

impl App {
    /// Whether run `run`'s lead session may close now: between turns, and nothing in the run
    /// needs it (a goal's lead works until its goal ends, and answers its team while they work).
    pub(super) fn parkable(&self, run: &Run) -> bool {
        let st = run.st.lock().unwrap();
        if st.lead.in_turn || st.lead.parked || st.cancelled() || st.lead.session.is_none() {
            return false;
        }
        match run.kind {
            RunKind::Chat | RunKind::Loop => true,
            RunKind::Goal => st.lead.goal.ended() && !run.driving.active(),
        }
    }

    /// Record that the session closes for `why`; the caller closes it. A chat's or a Loop's run is
    /// interrupted, as after a restart; a goal run keeps its status. Until it leaves memory the run
    /// is marked parked, so the next message waits for it to go and opens it anew (`resume_run_by`).
    pub(super) fn park(&self, run: &Run, why: Parked) {
        run.st.lock().unwrap().lead.parked = true;
        let text = match why {
            Parked::Idle => {
                format!(
                    "闲置了 {} 分钟，先关掉 Agent 进程；发消息时重新打开",
                    self.cfg.idle_after.as_secs().div_ceil(60)
                )
            }
            Parked::Full => {
                format!("同时开着的 Agent 满了（{} 个），先关掉闲得最久的这个会话；发消息时重新打开", self.seats.cap())
            }
        };
        self.emit(run, ev("session.parked", "janus", text).to("lead").data(json!({ "reason": why })));
        if run.kind.single_session() {
            self.set_status(run, RunStatus::Interrupted);
        }
    }

    /// Ask the idlest parkable session to close, to free a seat.
    fn park_idlest(&self) {
        let runs: Vec<Arc<Run>> = self.runs.lock().unwrap().values().cloned().collect();
        let idlest = runs.iter().filter(|r| self.parkable(r)).min_by_key(|r| r.st.lock().unwrap().lead.idle_since);
        if let Some(run) = idlest {
            let _ = self.send_steer(run, "lead", "", Steer::Park);
        }
    }

    /// A seat for an agent of `run` (`owner`'s session), waiting in line as `asker` when every
    /// seat is taken: the idlest session is asked to close, and the agent's status says it waits.
    /// A run cancelled or deleted meanwhile stops waiting.
    pub(super) async fn take_seat(&self, run: &Run, owner: &Owner, agent: &str, asker: Asker) -> Result<Seat> {
        let mut line = match self.seats.take(asker) {
            Ok(seat) => return Ok(seat),
            Err(line) => line,
        };
        let task = match &owner.session {
            SessionKey::Task(t) => t.as_str(),
            SessionKey::Lead => "",
        };
        let mut e = ev("agent.status", agent, format!("同时开着的 Agent 已满（{} 个），等空位", self.seats.cap()))
            .task(task)
            .data(json!({ "phase": AgentPhase::WaitingForSeat }));
        e.attempt = (owner.attempt > 0).then_some(owner.attempt);
        self.emit(run, e);
        self.park_idlest();
        loop {
            tokio::select! {
                seat = &mut line => return seat.map_err(|_| anyhow!("Janus 正在退出")),
                () = tokio::time::sleep(SEAT_RETRY) => {
                    let gone = run.st.lock().unwrap().cancelled() || *run.emitting.lock().unwrap_or_else(|p| p.into_inner());
                    if gone {
                        return Err(Cancelled.into());
                    }
                    // A session may have gone idle since.
                    self.park_idlest();
                }
            }
        }
    }
}

/// How often one waiting for a seat looks again for an idle session to close.
const SEAT_RETRY: Duration = Duration::from_secs(10);
