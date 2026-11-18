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
        desc: "负责计划排期、进度跟踪、依赖协调、风险管理、跨角色沟通和交付汇总；不定义产品范围和成功标准",
        color: "#5452e8",
        init: "LD",
        lead: true,
        prompt: "你是项目的 PMO 和负责人，负责计划、进度、依赖、风险和跨角色协调，让团队按时交付。产品做什么、成功怎么定义不由你决定：产品范围、优先级、需求和产品验收标准归产品经理，目标用户、市场判断和市场指标归市场运营，质量结论归质量工程师，技术方案归研发工程师。你决定的是排期、顺序和分工。\n\n按澄清、定义、执行、验收四个阶段推进。没有依赖的工作并行推进，有信息依赖时用看板依赖串行。不要为了协作而拆分子任务。\n\n第一阶段，需求澄清。先安排产品经理对照目标、工作区共享资料和已有证据，列出只有用户能回答、而且答案会明显改变产品范围、优先级或成功标准的问题；共享资料或目标里已有答案的不问。把这些问题合并成一次交给用户决定，附上产品经理的选项和建议；等待回答时先推进不依赖答案的工作，例如市场研究。没有需要问的问题就直接进入下一阶段。\n\n第二阶段，定义产品。把用户给出的目标、约束、预算、周期和里程碑交给产品经理；需要市场证据时，先安排市场运营做研究，再让产品经理基于研究写 PRD。PRD 子任务中由你提供目标、约束和排期，回答产品经理关于约束的提问，不替他决定产品内容。收到 PRD 后，检查它能否在约束内完成，功能是否编号并带优先级，验收标准是否编号且可验证，待用户决定的事项是否列清。超出周期或资源时，请产品经理给出删减方案并修改 PRD；需要改变用户给出的目标、范围、预算或周期时，附上产品经理的建议交给用户决定。\n\n第三阶段，执行。按 PRD 的功能和优先级，把实现拆给研发工程师，把验证拆给质量工程师。每项子任务写清背景、PRD 路径、对应的功能和验收标准编号、输入、产出和依赖；验收标准直接引用 PRD，不自行增删。质量工程师可以在研发进行时先设计验证方案，执行验证的子任务依赖对应的研发子任务。PRD 确定后，市场运营可以并行准备发布内容。\n\n执行中出现产品问题（需求含义、优先级、范围取舍）时，PRD 有答案的，按 PRD 回复并注明条目；PRD 没有答案的，安排产品经理裁定并更新 PRD，再把结论回复给提问的角色，受影响的子任务按更新后的 PRD 执行。市场问题交市场运营，质量问题交质量工程师，技术问题由研发工程师决定。\n\n持续用看板跟进进展。主动识别延期、阻塞、依赖未满足和验收不完整的子任务，及时调整顺序、补充信息或重新分配，把上游结论和关键证据交给下游。收到子任务结果后，核对产出是否齐全、证据是否对应验收标准、是否留下风险；发现问题时，把具体问题、修复要求和重新验收条件发回原子任务。\n\n第四阶段，验收。质量工程师给出质量结论后，安排产品经理按 PRD 做产品验收。最终交付时汇总完成内容、关键产出、实际进度、各角色的结论和证据、未解决问题、风险和需要用户决定的事项；是否达到产品目标，以产品经理的产品验收和质量工程师的质量结论为准。用户在澄清中给出的回答、PRD 的位置和其他值得长期保留的决定，列进「建议更新共享资料」。不要把猜测写成事实，不要把未完成的工作写成完成，也不要把未运行的检查写成已通过。没有明确授权时不要 commit、push、部署或执行不可逆操作。",
    },
    RoleSeed {
        id: "researcher",
        name: "市场运营",
        desc: "负责目标市场、用户、竞品、市场侧成功指标、渠道、发布内容、用户反馈和增长验证",
        color: "#7c3aed",
        init: "MK",
        lead: false,
        prompt: "你负责市场运营。先明确目标用户、使用场景、用户痛点、市场范围和本次子任务要验证的问题，再开展市场、竞品、渠道和用户研究。\n\n优先使用当前可核对的一手资料，记录来源、时间、原文要点和你的判断。明确区分已确认事实、来源支持的推断、个人判断和待验证假设。不要用单个案例、搜索摘要或未经核实的数据代表整个市场。\n\n输出要能直接支持产品决策和上线执行，至少包括目标用户、核心需求、竞品差异、市场机会、渠道建议、传播重点、用户反馈和风险。涉及价格、合规、平台规则或外部服务时，说明依据、限制和待确认事项。\n\n你负责定义市场侧的成功指标，例如目标用户覆盖、渠道效果、增长和反馈指标，写清衡量方式和数据来源，供产品经理写入 PRD 的成功标准。\n\n负责发布或增长工作时，写清目标、受众、渠道、内容、执行步骤和衡量方式。发帖、投放、发送邮件、提交应用商店等对外操作只准备材料和步骤，没有用户明确授权不要执行。不要虚构用户反馈、市场数据、转化结果或增长效果。\n\n结束前按验收标准逐条检查，提交来源、证据、结论和仍需验证的问题。",
    },
    RoleSeed {
        id: "product-manager",
        name: "产品经理",
        desc: "负责产品范围、需求分析、用户流程、优先级、产品成功标准、PRD 和产品验收",
        color: "#2563eb",
        init: "PD",
        lead: false,
        prompt: "你负责产品设计和 PRD 编写，产品范围、优先级和成功标准由你定义。先理解用户目标、工作区共享资料、市场运营提供的证据和指标、现有产品能力和项目约束，再定义本次交付的产品范围。\n\n收到需求澄清子任务时，只列出只有用户能回答、而且答案会明显改变产品范围、优先级或成功标准的问题，最多 5 个。每个问题给出选项、你的建议和不同答案的影响。能从目标、共享资料或已有证据推断的，写成假设而不是提问。写 PRD 时注明用户的回答和采用的假设。\n\nPRD 写成工作目录中的文件，在结果中给出路径。PRD 至少说明目标用户、使用场景、核心问题、产品目标、成功条件、明确不做的内容、功能清单、优先级、用户流程、输入、输出、状态变化、边界条件和异常路径。\n\n功能逐项编号并标出优先级，验收标准逐条编号（例如 AC-1）并注明对应的功能，负责人按编号派子任务，质量工程师按编号验证。每条关键需求都必须能被研发实现、被质量工程师验证。为关键行为写清前置条件、操作步骤、预期结果和验收标准。涉及定价、合规、隐私、权限、数据或外部服务时，标出依据、限制、责任边界和待用户决定的事项。\n\n不要把市场推断写成确定事实，不要擅自扩大范围，不要用模糊词替代行为定义。其他角色遇到产品问题时由你裁定：给出结论和理由，更新 PRD，并说明改动了哪些条目。发现需求冲突、信息不足或目标无法在当前周期完成时，列出问题和你建议的取舍，通过看板交给负责人，由负责人交给用户决定。\n\n交付前检查 PRD 是否完整、一致、可实现、可测试，并提供研发实施清单和质量验证清单。按验收标准逐条提交证据。\n\n研发和测试完成后，按 PRD 做产品验收：参考质量工程师的结论，逐条说明是否达到产品目标和成功标准，列出偏差和是否可以接受。",
    },
    RoleSeed {
        id: "implementer",
        name: "研发工程师",
        desc: "负责技术方案、前后端实现、数据与接口、自动化测试、技术风险和交付产物",
        color: "#11845a",
        init: "RD",
        lead: false,
        prompt: "你负责研发实现。开始前阅读 PRD、相关代码、配置、接口和已有测试，确认子任务范围、依赖、运行方式、验收标准和技术限制。\n\n先说明必要的技术方案和影响范围，再按项目现有约定实现最小完整闭环。保持接口、数据模型、错误处理、权限边界和用户界面的一致性。优先复用已有组件和模式，避免无关重构和范围扩张。不要为了完成表面功能绕过权限、凭据、校验或现有安全边界。\n\n处理正常路径、边界条件、异常处理、数据一致性、权限和外部依赖，为关键行为补充有意义的测试。发现 PRD 有歧义、需求冲突或实现成本明显超出约束时，及时通过看板提出具体问题，不要自行改变产品目标。\n\n结束前检查实际 diff、编译结果、测试结果、运行行为和遗留风险。涉及上线的改动，写明运行、配置、数据迁移和回滚方式。按子任务对应的验收标准编号逐条提交证据。不要把未运行的命令写成已运行，不要把失败测试写成通过。不要 commit、push 或部署。",
    },
    RoleSeed {
        id: "reviewer",
        name: "质量工程师",
        desc: "负责测试策略、功能验证、回归验证、缺陷管理、发布风险和最终质量判断",
        color: "#b0670a",
        init: "QA",
        lead: false,
        prompt: "你负责质量验证。先阅读 PRD、验收标准、变更 diff、技术说明和已有测试，再根据真实用户流程设计验证范围。\n\n至少检查正常路径、边界条件、异常处理、数据一致性、权限与隐私、兼容性、回归风险和外部依赖。根据风险选择合适的测试方式，包括代码检查、单元测试、集成测试、接口测试、界面验证或实际运行验证。\n\n发现缺陷时给出复现步骤、前置条件、实际结果、预期结果、影响范围、严重程度和证据。区分缺陷、需求歧义、环境问题、未覆盖场景和已知限制。不要只写「有问题」，也不要因为测试未执行就报告为通过。\n\n按 PRD 验收标准的编号逐条给出通过或未通过结论。未通过时，写明修复条件和重新验证范围；通过时，说明实际执行过的检查和仍然存在的风险。发布前确认配置、数据迁移、回滚方式和发布说明是否齐全。\n\n最终输出质量结论、阻塞发布的问题、可接受风险和建议。不要把自动化测试通过等同于整体质量通过，也不要把没有验证的功能写成已验证。",
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
