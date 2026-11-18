//! Roles: saving and deleting them, the library items they use, and the default roles a first
//! start creates.

use super::*;

impl App {
    pub fn roles(&self) -> Result<Vec<Value>> {
        self.db.list_config(ROLE)
    }

    pub fn save_role(&self, body: &Value) -> Result<Value> {
        let name = str_of(body, "name");
        if name.is_empty() {
            bail!("数字员工需要一个名字");
        }
        let id = match str_of(body, "id") {
            "" => new_id("r"),
            id => {
                check_id(id)?;
                id.to_string()
            }
        };
        let connection = str_of(body, "connection");
        // The session controls this role chose, checked against what its agent declares.
        let session = if connection.is_empty() { json!({}) } else { role_session(body, &self.connection(connection)?) };
        let init = match str_of(body, "init") {
            "" => name.chars().filter(|c| !c.is_whitespace()).take(2).collect::<String>().to_uppercase(),
            i => i.to_string(),
        };
        // The library items the role uses, each once; an id the library no longer has is dropped.
        let known: HashSet<String> = self.items()?.into_iter().map(|i| i.id).collect();
        let mut resources: Vec<&str> = vec![];
        for id in body["resources"].as_array().into_iter().flatten().filter_map(Value::as_str) {
            if known.contains(id) && !resources.contains(&id) {
                resources.push(id);
            }
        }
        let role = json!({
            "id": id, "name": name, "desc": str_of(body, "desc"),
            "prompt": body["prompt"].as_str().unwrap_or("").trim(), "resources": resources,
            "connection": connection, "model": str_of(body, "model"), "effort": str_of(body, "effort"),
            "session": session,
            "lead": body["lead"].as_bool().unwrap_or(false),
            "color": match str_of(body, "color") { "" => "#64748b", c => c }, "init": init,
        });
        self.db.put_config(ROLE, &id, &role, now_ms())?;
        Ok(role)
    }

    pub fn delete_role(&self, id: &str) -> Result<()> {
        if let Some(ws) = self
            .db
            .list_config(WORKSPACE)?
            .iter()
            .find(|w| w["members"].as_array().is_some_and(|m| m.iter().any(|m| m["role"] == id)))
        {
            bail!("工作区「{}」正在使用这个角色，请先移除", str_of(ws, "name"));
        }
        check_id(id)?;
        self.db.delete_config(ROLE, id)
    }

    /// Claude Code runs `/goal` only in a workspace it trusts. The lead's agent has its own
    /// config directory, so record the trust there (never in the user's own Claude config).
    pub fn trust_claude_workspace(&self, id: &str, path: &Path) -> Result<()> {
        let file = self.agent_dir(id).join("home").join(".claude.json");
        let mut cfg: Value = std::fs::read_to_string(&file)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf()).to_string_lossy().to_string();
        if cfg["projects"][&key]["hasTrustDialogAccepted"] == true {
            return Ok(());
        }
        if !cfg["projects"].is_object() {
            cfg["projects"] = json!({});
        }
        if !cfg["projects"][&key].is_object() {
            cfg["projects"][&key] = json!({});
        }
        cfg["projects"][&key]["hasTrustDialogAccepted"] = json!(true);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&file, serde_json::to_vec_pretty(&cfg)?)?;
        Ok(())
    }

    /// First start: a usable set of roles that only lacks agent bindings. It happens once
    /// (`meta.seeded`): a default role the person deleted or edited afterwards stays that way.
    // @project-doc docs/domains/20261031-agents_and_roles.md#roles
    pub fn seed_roles(&self) -> Result<()> {
        if self.db.get_config("meta", "seeded")?.is_some() {
            return Ok(());
        }
        for r in &SEEDS {
            self.save_role(&json!({ "id": r.id, "name": r.name, "desc": r.desc, "color": r.color, "init": r.init, "lead": r.lead, "prompt": r.prompt }))?;
        }
        self.db.put_config("meta", "seeded", &json!(true), now_ms())
    }
}

/// A default role, created on a first start with no agent bound yet.
pub(super) struct RoleSeed {
    id: &'static str,
    name: &'static str,
    desc: &'static str,
    color: &'static str,
    init: &'static str,
    lead: bool,
    prompt: &'static str,
}

/// The product launch team: a PMO lead, market operations, PRD, engineering and QA.
pub(super) const SEEDS: [RoleSeed; 5] = [
    RoleSeed {
        id: "coordinator",
        name: "PMO",
        desc: "担任负责人：拆解目标、安排顺序、分派子任务、跟进进度与风险、汇总交付；不替产品、技术和质量做决定",
        color: "#5452e8",
        init: "LD",
        lead: true,
        prompt: "你是团队的 PMO，也是这次目标的负责人。你的职责是让目标按质交付：拆解工作、安排顺序、分派子任务、跟进进度和风险、汇总结果。产品范围和验收标准、市场判断、技术方案和质量结论，分别由负责这些事的协作者决定；你决定顺序、分工和什么时候算完成。\n\n先判断规模，再决定流程：\n- 小目标（修一个缺陷、加一个小功能、回答一个调研问题）不走完整流程。直接派给职责最匹配的协作者；改了代码的，再安排一次独立验证。一个人一轮就能做完的事，可以自己做。\n- 新产品、跨模块的功能或范围不清的目标，按下面四个阶段推进。\n不要为了协作而拆分子任务，也不要让协作者重复已经有结论的工作。\n\n按协作者名单中的职责找人，不按名字。名单里没有某类职责的人时，在计划中写明这部分由谁承担，必要时自己做。\n\n一、澄清。范围或成功标准不清时，先请负责产品的协作者对照目标、工作区共享资料和已有证据，列出只有用户能回答、而且答案会明显改变范围、优先级或成功标准的问题，最多 5 个，每个带选项和建议。你把这些问题合并成一次交给用户；等待期间推进不依赖答案的工作，例如市场研究。没有这样的问题就跳过。\n\n二、定义。需要市场证据时，先安排研究，再安排写 PRD。把目标、约束、预算、周期和里程碑交给写 PRD 的人，不替他决定产品内容。收到 PRD 后检查：能否在约束内完成；功能是否编号并标出优先级；验收标准是否编号（例如 AC-1）且可验证；待用户决定的事项是否列清。超出约束时，请产品经理给出删减方案；需要改变用户给定的目标、范围、预算或周期时，附上建议交给用户决定。\n\n三、执行。按 PRD 的功能和优先级拆子任务。每个子任务写清背景、PRD 路径、对应的功能和验收标准编号、输入、产出、负责的文件和依赖；验收标准直接引用 PRD，不自行增删。多个子任务共用的数据结构或接口，先在上游子任务中定下来。质量验证可以在实现进行时先设计方案；改了代码的子任务完成后，请质量工程师做独立验证。PRD 定稿后，发布内容可以并行准备。\n\n执行中的问题按归属处理。产品问题 PRD 有答案的，按 PRD 回复并注明条目；没有答案的，请产品经理裁定并更新 PRD，再把结论转给提问的人。市场、技术和质量问题，分别交给负责这些事的协作者决定。\n\n持续跟进看板。发现延期、阻塞、依赖未满足或验收不完整，及时调整顺序、补充信息或重新分派。收到结果后，核对产出是否齐全、证据是否对应验收标准、是否留下风险；不满足时，把具体问题、修复要求和重新验收的条件发回原执行者。\n\n四、验收。质量结论出来后，请产品经理按 PRD 做产品验收；是否达到目标，以产品验收和质量结论为准。交付摘要写清：完成了什么、关键产出和位置、每条验收标准的结论和证据、未解决的问题和风险、需要用户决定的事项。用户在澄清中的回答、PRD 的位置等值得长期保留的事实，按 Janus 的说明列进共享资料建议。\n\n始终区分事实、推断和假设，不要把未完成的写成完成，不要把没运行的检查写成通过。改变范围、预算或周期，使用外部资源或付费服务，以及不可逆或影响工作目录之外的操作，先交给用户决定。",
    },
    RoleSeed {
        id: "researcher",
        name: "市场运营",
        desc: "市场和用户研究、竞品分析、市场侧成功指标、发布与增长方案；适合需要外部证据或对外内容的子任务",
        color: "#7c3aed",
        init: "MK",
        lead: false,
        prompt: "你负责市场和用户研究、市场侧成功指标和发布内容。产品经理会用你的结论定义范围和成功标准，所以结论要能核对、能直接用于决策。\n\n开始前，写清本次要回答的问题、目标用户和市场范围。子任务没有说清的，按目标和共享资料做合理假设，并在结果中写明。\n\n研究：\n- 优先使用能核对的一手来源，例如官方网站、产品文档、定价页、应用商店和公开数据，记下链接、访问日期和要点。不能联网或找不到可靠来源时如实说明，相关结论标为待验证。\n- 区分已确认的事实、有来源支持的推断和你的判断。不要用单个案例或搜索摘要代表整个市场，不要编造数据、用户反馈或增长效果。\n- 结论至少覆盖目标用户和使用场景、核心痛点、主要竞品及差异、机会和风险，以及对产品范围的建议。\n\n指标：提出市场侧的成功指标，例如获客、激活、留存和渠道转化，写清定义、衡量方式、数据来源和目标值的依据，供产品经理写进 PRD。\n\n发布与增长：写清目标、受众、渠道、内容、时间和衡量方式，内容要能直接使用。发帖、投放、发送邮件、提交应用商店审核等对外操作，只准备材料和步骤，没有用户明确授权不要执行。\n\n篇幅较长的研究写成工作目录中的 Markdown 文件，在结果中给出路径和最重要的几条结论。",
    },
    RoleSeed {
        id: "product-manager",
        name: "产品经理",
        desc: "澄清需求、定义范围和优先级、编写带编号验收标准的 PRD、裁定产品问题、做产品验收",
        color: "#2563eb",
        init: "PD",
        lead: false,
        prompt: "你负责产品定义：范围、优先级、需求、成功标准和产品验收由你决定。你写的 PRD 是研发实现和质量验证的共同依据。\n\n开始前，阅读目标、工作区共享资料、市场研究结论和现有产品，弄清已有能力和约束。\n\n澄清：收到澄清子任务时，只列出只有用户能回答、而且答案会明显改变范围、优先级或成功标准的问题，最多 5 个；每个问题给出选项、你的建议和不同答案的影响。能从已有信息推断的，写成假设，不要提问。\n\nPRD 写成工作目录中的 Markdown 文件，在结果中给出路径。篇幅与目标的规模相称，小改动一页就够。至少包括：\n- 目标用户、使用场景、要解决的问题和成功标准，引用市场侧指标；\n- 本次的范围，以及明确不做的内容；\n- 功能清单，逐项编号（例如 F-1）并标出优先级；\n- 用户流程、关键状态、边界条件和异常路径；\n- 验收标准，逐条编号（例如 AC-1）并注明对应的功能，每条写成可观察的行为：前置条件、操作和预期结果；\n- 采用的假设、用户的回答和待用户决定的事项。涉及定价、合规、隐私、权限、数据或外部服务时，写明依据和限制。\n\n不要把推断写成事实，不要擅自扩大范围，不要用「友好」「快速」这类无法验证的词代替行为定义。\n\n裁定：其他人提出产品问题时，给出结论和理由，更新 PRD，并在结果中列出改动的条目。目标无法在约束内完成或需求互相冲突时，给出取舍建议，交给负责人。\n\n产品验收：研发和质量验证完成后，参考质量结论，逐条判断功能和成功标准是否达到，列出偏差和能否接受，最后给出能否交付的结论。",
    },
    RoleSeed {
        id: "implementer",
        name: "研发工程师",
        desc: "技术方案、编码实现、缺陷修复和自动化测试；适合需要改代码、配置或构建脚本的子任务",
        color: "#11845a",
        init: "RD",
        lead: false,
        prompt: "你负责技术实现：方案、编码、缺陷修复和自动化测试。\n\n开始前，读子任务说明和对应的 PRD 条目，再读相关代码、项目约定（例如 AGENTS.md、CLAUDE.md、README 和贡献指南）和已有测试，弄清怎样构建、运行和测试。\n\n实现：\n- 先想清楚方案和影响范围。改动较大或有多种做法时，在交接说明中写明你的选择和理由。\n- 沿用项目已有的结构、命名和模式，优先复用已有代码；只改完成子任务需要的部分，不做无关的重构。\n- 处理正常路径、边界条件、错误处理和数据一致性。不要为了让功能看起来能用而绕过校验、权限、凭据或安全边界。\n- 为新增或修改的行为补上有意义的测试；修复缺陷时，先写一个能复现它的测试。\n- PRD 有歧义、需求互相冲突或成本明显超出预期时，提出具体问题，不要自行改变产品行为。\n\n完成前，查看实际的 diff，运行构建、相关测试和项目的静态检查，必要时实际运行程序观察行为。涉及配置、数据迁移或部署的改动，写明操作步骤和回滚方式。下游需要的接口、数据格式和注意事项，写进交接说明。\n\n不要把没运行的命令写成运行过，不要把失败的测试写成通过。",
    },
    RoleSeed {
        id: "reviewer",
        name: "质量工程师",
        desc: "设计和执行测试、独立验证他人的改动、报告缺陷、给出质量结论；独立验证时只检查不改文件",
        color: "#b0670a",
        init: "QA",
        lead: false,
        prompt: "你负责质量：设计和执行验证、报告缺陷、给出质量结论。改动能否交付取决于你的结论，所以结论要基于实际检查，不能只读执行者的说明。\n\n开始前，读 PRD 的验收标准、被验证子任务的结果和 diff、相关代码和已有测试，按真实的用户流程确定验证范围。\n\n验证：\n- 覆盖正常路径、边界条件、错误处理、数据一致性、权限与隐私和回归风险。按风险选择方式：运行已有测试、补充测试、调用接口、实际运行程序或检查代码。\n- 能运行的就运行，看到结果再下结论；无法运行时说明原因，把结论标为未验证。\n- 自动化测试通过不等于功能正确，要对照验收标准检查实际行为。\n\n负责人指定你独立验证某个子任务时，只检查、不修改文件，按 Janus 的要求给出判定。其他子任务中，可以补充测试代码。\n\n缺陷：写清复现步骤、前置条件、实际结果、预期结果、影响范围、严重程度和证据，区分缺陷、需求歧义、环境问题和已知限制。\n\n结论：按验收标准的编号逐条给出通过或未通过，以及实际做了哪些检查。未通过的，写明修复要求和需要重新验证的范围。最后给出能否发布、阻塞问题和可以接受的风险。",
    },
];

#[cfg(test)]
mod seed_tests {
    use super::*;
    use crate::orchestrator::{App, test_app};
    use std::sync::Arc;

    fn app(name: &str) -> (Arc<App>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("janus-seed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        (test_app(&dir), dir)
    }

    fn role(app: &App, id: &str) -> Option<Value> {
        app.db.get_config(ROLE, id).unwrap()
    }

    #[test]
    fn a_first_start_seeds_every_default_role() {
        let (app, dir) = app("fresh");
        app.seed_roles().unwrap();
        for id in ["coordinator", "researcher", "product-manager", "implementer", "reviewer"] {
            assert!(!role(&app, id).unwrap()["prompt"].as_str().unwrap().is_empty(), "{id}");
        }
        // A deleted default role stays deleted on the next start.
        app.db.delete_config(ROLE, "reviewer").unwrap();
        app.seed_roles().unwrap();
        assert!(role(&app, "reviewer").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod id_tests {
    use super::*;
    use crate::orchestrator::test_app;

    /// Ids name files under Janus home: one that could leave its folder is refused when a role or a
    /// workspace is saved, and deleting a role by such an id touches nothing outside.
    #[test]
    fn ids_that_could_leave_their_folder_are_refused() {
        let dir = std::env::temp_dir().join(format!("janus-ids-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let app = test_app(&dir);
        // `<home>/../outside.json`, should an id ever name a path.
        let outside = dir.join("outside.json");
        std::fs::write(&outside, "keep").unwrap();
        for bad in ["../../outside", "a/b", "Upper", "-lead", "x".repeat(41).as_str()] {
            let err = app.save_role(&json!({ "id": bad, "name": "R" })).unwrap_err().to_string();
            assert!(err.contains("不合法"), "role id {bad}: {err}");
            let err = app
                .save_workspace(&json!({ "id": bad, "name": "W", "path": dir.to_string_lossy() }))
                .unwrap_err()
                .to_string();
            assert!(err.contains("不合法"), "workspace id {bad}: {err}");
        }
        assert!(app.delete_role("../../outside").is_err());
        assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep", "nothing outside Janus home is removed");
        // An agent's id names the folder its deletion removes.
        let err = app
            .save_connection(&json!({ "id": "../x", "kind": "custom", "name": "A", "command": "true" }))
            .unwrap_err()
            .to_string();
        assert!(err.contains("不合法"), "{err}");
        assert!(app.delete_connection("..").is_err());
        assert!(outside.exists(), "deleting an agent never leaves the agents folder");
        // A valid id is kept as given.
        assert_eq!(app.save_role(&json!({ "id": "my-role", "name": "R" })).unwrap()["id"], "my-role");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
