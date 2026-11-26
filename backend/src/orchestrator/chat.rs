//! Chats: a conversation with one digital employee, as a run of its own kind. The employee is the
//! run's one member and holds its main session; there is no board, no goal and no delivery. The
//! conversation works in a scratch directory under Janus home made for it, and everything else
//! (messages, steering, takeover, approvals and forms, images, `@` files, controls, usage, reopening
//! after a restart) is the same as in the lead's conversation.
// @project-doc docs/domains/20261120-chats.md#chat_model

use super::*;
use crate::agents::RoleFile;
use crate::lang::{Lang, tr};

impl App {
    /// Start a chat with `role`: snapshot it as the run's one member, make the chat's directory and
    /// open its session, which waits for the person's first message.
    // @project-doc docs/domains/20261120-chats.md#chat_model
    pub async fn create_chat(self: &Arc<Self>, req: NewChat) -> Result<String> {
        let role = self.role(&req.role)?;
        let member = self.single_member(&role)?;
        let id = crate::util::new_id("r");
        let cwd = self.own_workdir(RunKind::Chat, &id)?;
        // Titled by its first message, once there is one (`title_chat`).
        let run = self.single_member_run(RunKind::Chat, &id, member, cwd, "", json!({}))?;
        let name = run.team[0].name.clone();
        self.emit(&run, ev(EventKind::RunCreated, "user", format!("和 {name} 开始对话")).data(created(&run, &role.id)));
        self.open_single_member_run(run);
        Ok(id)
    }

    /// The working directory of a run that has its own (a chat, a consolidation), made now: the
    /// path the agent's own tools report, so the files it names resolve against it.
    pub(super) fn own_workdir(&self, kind: RunKind, id: &str) -> Result<PathBuf> {
        let dir = self.cfg.layout.run(kind, id).work;
        std::fs::create_dir_all(&dir).with_context(|| format!("创建目录 {} 失败", dir.display()))?;
        Ok(dir.canonicalize().unwrap_or(dir))
    }

    /// A new single-member run goes live: its session opens and waits for its first message.
    pub(super) fn open_single_member_run(self: &Arc<Self>, run: Arc<Run>) {
        self.runs.lock().unwrap().insert(run.id.clone(), run.clone());
        self.spawn_conversation(run);
    }

    /// A digital employee as the one member of a run's main session (a chat, a Loop's session, a
    /// consolidation),
    /// with the library as it is now, pinned like for a run's team; its agent must be able to run.
    // @project-doc docs/domains/20261116-resource_library.md#delivery
    pub(super) fn single_member(&self, role: &Role) -> Result<Member> {
        let member = self.role_member(role, &self.items()?, true)?;
        self.spec_for(&member.connection)?;
        Ok(member)
    }

    /// Record a run whose only member is `member` (a chat, a Loop's session), working in `cwd`:
    /// its skills bundled, its snapshot (`extra` adds to it) and the run, not yet live.
    pub(super) fn single_member_run(
        &self,
        kind: RunKind,
        id: &str,
        member: Member,
        cwd: PathBuf,
        goal: &str,
        extra: Value,
    ) -> Result<Arc<Run>> {
        let team = vec![member];
        let lang = self.language();
        self.bundle_team(&self.cfg.layout.run(kind, id), &team, lang)?;
        let mut config = json!({ "team": team, "git": false, "baseRef": "", "lang": lang });
        if let (Some(c), Some(more)) = (config.as_object_mut(), extra.as_object()) {
            c.extend(more.clone());
        }
        self.db.insert_run(id, kind, goal, &cwd.to_string_lossy(), &config, now_ms())?;
        Ok(Arc::new(Run::new(RunRecord {
            id: id.to_string(),
            kind,
            goal: goal.to_string(),
            cwd,
            worktree: None,
            team,
            git: false,
            base_ref: String::new(),
            brief: String::new(),
            goal_images: vec![],
            lang,
            last_seq: 0,
            status: RunStatus::Planning,
            asker: Asker::Person,
        })))
    }

    /// A chat is listed by its first message: the first one the person sends titles it.
    pub(super) fn title_chat(&self, run: &Run, text: &str) -> Result<()> {
        if !run.kind.titled_by_first_message() || self.db.run(&run.id)?.is_some_and(|r| !r.goal.is_empty()) {
            return Ok(());
        }
        let title = match text.trim() {
            "" => "看看这些图片",
            t => t,
        };
        self.db.set_run_goal(&run.id, title)
    }
}

/// The most digital employees one proposal holds.
const MAX_PROPOSED: usize = 20;

/// The digital employees a chat proposed that the person has not created or declined yet: the
/// chat's latest proposal (each new one replaces the one before), by its event's seq.
#[derive(Serialize)]
pub struct Proposal {
    pub proposal: i64,
    pub roles: Vec<ProposedRole>,
}

/// One employee of a proposal, numbered from 1 (`n`, what the person ticks), and the saved
/// digital employee of the same name, if any, which the app leaves unticked.
#[derive(Serialize)]
pub struct ProposedRole {
    pub n: String,
    #[serde(flatten)]
    pub role: RoleFile,
    pub existing: Option<String>,
}

impl App {
    /// `propose_employees`: check the employees the chat's digital employee designed and put them
    /// on record for the person, who sees them in the chat and creates the ones they pick.
    // @project-doc docs/domains/20261120-chats.md#proposed_employees
    pub(super) fn propose_employees(&self, run: &Run, args: &Value) -> std::result::Result<String, String> {
        let lang = run.lang;
        let list = args["employees"].as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
            tr!(lang, "employees 要列出至少一位数字员工", "employees must list at least one digital employee")
        })?;
        if list.len() > MAX_PROPOSED {
            return Err(tr!(lang, "一次最多 {MAX_PROPOSED} 位", "At most {MAX_PROPOSED} at a time"));
        }
        let text = |v: &Value, k: &str| v[k].as_str().unwrap_or("").to_string();
        let mut roles = vec![];
        for (i, e) in list.iter().enumerate() {
            let role = RoleFile {
                id: String::new(),
                name: text(e, "name"),
                desc: text(e, "desc"),
                group: text(e, "group"),
                color: text(e, "color"),
                init: text(e, "init"),
                lead: e["lead"] == true,
                prompt: text(e, "instructions"),
            }
            .checked()
            .map_err(|why| {
                let n = i + 1;
                tr!(lang, "第 {n} 位：{why}", "Employee {n}: {why}")
            })?;
            roles.push(role);
        }
        let names = roles.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join(list_sep(lang));
        let n = roles.len();
        let e = ev(EventKind::RolesProposed, "lead", format!("提议了 {n} 位数字员工：{names}"))
            .data(json!({ "employees": roles }));
        self.emit_kept(run, e).ok_or_else(|| tr!(lang, "没能记下这一批", "Could not record them"))?;
        Ok(tr!(
            lang,
            "已交给用户：{n} 位（{names}）。用户在对话里逐个看过、选好 Agent 后才会创建；再交一批会替换这一批。",
            "Handed to the person: {n} ({names}). They are created only after the person reads them in the chat and picks an agent; another proposal replaces this one."
        ))
    }

    /// The chat's open proposal, if any (`Proposal`).
    // @project-doc docs/domains/20261120-chats.md#proposed_employees
    pub fn open_proposal(&self, run_id: &str) -> Result<Option<Proposal>> {
        let Some(asked) = self.db.latest_event(run_id, EventKind::RolesProposed)? else { return Ok(None) };
        let settled =
            [EventKind::RolesCreated, EventKind::RolesDeclined].into_iter().try_fold(false, |done, kind| {
                Ok::<_, anyhow::Error>(
                    done || self
                        .db
                        .events_of(run_id, kind, asked.seq)?
                        .iter()
                        .any(|e| e.data.as_ref().is_some_and(|d| d["proposal"] == asked.seq)),
                )
            })?;
        if settled {
            return Ok(None);
        }
        let employees: Vec<RoleFile> =
            asked.data.as_ref().and_then(|d| serde_json::from_value(d["employees"].clone()).ok()).unwrap_or_default();
        let saved = self.db.list::<Role>()?;
        let roles = employees
            .into_iter()
            .enumerate()
            .map(|(i, role)| {
                let existing = saved.iter().find(|r| r.name == role.name).map(|r| r.id.clone());
                ProposedRole { n: (i + 1).to_string(), role, existing }
            })
            .collect();
        Ok(Some(Proposal { proposal: asked.seq, roles }))
    }

    /// The person creates the employees `picks` (their numbers) of the chat's open proposal
    /// `proposal`, all on the agent `connection`, and the chat records it.
    // @project-doc docs/domains/20261120-chats.md#proposed_employees
    pub fn create_proposed(
        &self,
        run_id: &str,
        proposal: i64,
        picks: &[String],
        connection: &str,
    ) -> Result<Vec<Role>> {
        let mut open = self.still_open(run_id, proposal)?.roles;
        let picked = picks
            .iter()
            .map(|n| {
                let at = open.iter().position(|r| &r.n == n).ok_or_else(|| anyhow!("这一批里没有第 {n} 位"))?;
                Ok(open.swap_remove(at).role)
            })
            .collect::<Result<Vec<_>>>()?;
        let made = self.make_roles(picked, connection)?;
        let names = made.iter().map(|r| r.name.as_str()).collect::<Vec<_>>().join("、");
        let created: Vec<Value> = made.iter().map(|r| json!({ "id": r.id, "name": r.name })).collect();
        let e = ev(EventKind::RolesCreated, "user", format!("创建了 {} 位数字员工：{names}", made.len()))
            .data(json!({ "proposal": proposal, "roles": created }));
        self.note(run_id, e)?;
        Ok(made)
    }

    /// The person creates none of the chat's open proposal `proposal`.
    pub fn decline_proposed(&self, run_id: &str, proposal: i64) -> Result<()> {
        self.still_open(run_id, proposal)?;
        self.note(
            run_id,
            ev(EventKind::RolesDeclined, "user", "不创建这一批数字员工").data(json!({ "proposal": proposal })),
        )?;
        Ok(())
    }

    /// The chat's open proposal, when it is `proposal`: one already acted on, or replaced by a
    /// newer one, is not acted on again.
    fn still_open(&self, run_id: &str, proposal: i64) -> Result<Proposal> {
        self.open_proposal(run_id)?
            .filter(|p| p.proposal == proposal)
            .ok_or_else(|| anyhow!("这一批数字员工已经处理过，或者被新的一批替换了"))
    }
}

wire_enum! {
    /// A tool a chat has, for what its digital employee may do (`Caller::Hiring`).
    pub(super) enum ChatTool {
        ProposeEmployees = "propose_employees",
    }
}

impl ChatTool {
    /// What `tools/list` says about the tool, in `lang`.
    // @project-doc docs/interfaces/20261031-board_mcp.md#chat_tools
    pub(super) fn definition(self, lang: Lang) -> Value {
        let t = |zh, en| lang.pick(zh, en);
        let s = |zh, en| json!({ "type": "string", "description": lang.pick(zh, en) });
        let mut d = match self {
            ChatTool::ProposeEmployees => json!({
                "description": t(
                    "把你为用户设计的一批数字员工交给用户。用户在对话里逐个看过指令、选好由哪个 Agent 来做后，才创建他勾选的人；这一批没处理之前再调用，新的一批替换它。创建后用户可以在设置里改。",
                    "Hand the person a set of digital employees you designed for them. The person reads each one's instructions in the chat and picks the agent that runs them; only the ones they tick are created. Calling again before they act replaces this set. They can edit them in Settings afterwards."
                ),
                "inputSchema": { "type": "object", "properties": {
                    "employees": { "type": "array", "items": { "type": "object", "properties": {
                        "name": s("名字。", "Name."),
                        "desc": s("一句话写什么样的子任务交给它：负责人只凭名字和这句话派活。", "One sentence on which subtasks go to it: a lead assigns work by name and this alone."),
                        "group": s("分组名，同一支团队用同一个。", "Group name, the same for one team."),
                        "color": s("头像颜色，# 加 6 位十六进制，每人不同。", "Picture color, # and 6 hex digits, different for each."),
                        "init": s("头像上的一两个字。", "One or two characters on the picture."),
                        "lead": { "type": "boolean", "description": t("能不能当负责人。", "Whether it may lead a team.") },
                        "instructions": s("指令全文：用第二人称写给它，怎样做事、做到什么程度算完、怎样核对、什么时候停下来问人。", "Its full instructions, addressed to it: how it works, when it is done, how it checks its work, when it stops to ask.")
                    }, "required": ["name", "desc", "instructions"] } }
                }, "required": ["employees"] },
                "annotations": board_act()
            }),
        };
        d["name"] = json!(self.as_str());
        d
    }
}

/// What Janus adds to a chat's system layer: nothing of its own, since a chat's agent works only by
/// the digital employee's instructions (the standing ones that follow), and not by Janus's teamwork
/// rules, which are for an Agent Team. Only a chat that goes on in a new session (its old one could
/// not be loaded) gets what was said so far, so it knows where it left off.
// @project-doc docs/domains/20261120-chats.md#chat_session
pub(super) fn chat_brief(events: &[Event], lang: Lang) -> String {
    let recent = recent_exchanges(events, Speakers::You, lang);
    if recent.is_empty() {
        return String::new();
    }
    tr!(
        lang,
        "和用户的这段对话在新会话中继续。照下面的记录，从上次停下的地方接着说。\n\n最近的对话：\n{recent}",
        "This conversation with the user continues in a new session. Use the history below to pick up where you left off.\n\nRecent conversation:\n{recent}"
    )
}

/// What `run.created` says of a single-member run: its kind, directory and the role it is.
pub(super) fn created(run: &Run, role: &str) -> Value {
    json!({
        "kind": run.kind, "cwd": run.cwd, "git": false, "role": { "id": role, "name": run.team[0].name },
    })
}
