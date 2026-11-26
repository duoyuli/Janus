//! What an agent runs on, and everything Janus does differently for each runtime. The rest of
//! Janus asks a `Runtime` what to do instead of matching on it, so a difference between Codex and
//! Claude Code is written down once, here.

use crate::wire::wire_enum;
use anyhow::{Result, anyhow};
use serde_json::{Value, json};

wire_enum! {
    /// What an agent runs on: Codex or Claude Code, each started through its official ACP adapter.
    pub enum Runtime {
        Codex = "codex",
        Claude = "claude",
    }
}

wire_enum! {
    /// Where a member's system layer reaches the model: Janus's instructions for the session and
    /// the person's standing rules. They hold for the whole session and outrank the task text, so
    /// they go in the system layer wherever the agent offers one (`session.prompt`, `data.via`).
    // @project-doc docs/interfaces/20261031-acp_integration.md#standing_instructions
    pub enum InstructionChannel {
        /// Claude Code: `session/new` / `session/load` `_meta.systemPrompt.append`.
        System = "system",
        /// Codex: `developer_instructions` in the adapter's `CODEX_CONFIG`.
        Developer = "developer",
    }
}

impl Runtime {
    /// The runtime of a connection's `base`.
    pub fn of(base: &str) -> Result<Runtime> {
        Runtime::parse(base).ok_or_else(|| anyhow!("不支持的 Agent 类型：{base}"))
    }

    /// The permission mode a session starts in when its role leaves the mode open: Codex's Full
    /// access and Claude Code's Bypass permissions, so goals run without stopping for approvals.
    // @project-doc docs/domains/20261031-agents_and_roles.md#permission_modes
    pub fn default_mode(self) -> &'static str {
        match self {
            Runtime::Codex => "agent-full-access",
            Runtime::Claude => "bypassPermissions",
        }
    }

    /// The settings a session's agent starts with for permission mode `mode`, on top of the mode
    /// the adapter is told (`session/set_config_option`). Codex's adapter applies a mode only to
    /// the prompts it sends; `/goal`, and the turns Codex continues a goal with, run under the
    /// thread's own approval and sandbox settings, which come from its config (`CODEX_CONFIG`)
    /// and otherwise default to asking and to a workspace sandbox. So the thread starts with the
    /// settings of the member's mode, one of the presets Codex's adapter declares. Claude Code
    /// applies its mode to the whole session and needs none.
    // @project-doc docs/domains/20261031-agents_and_roles.md#permission_modes
    pub fn mode_settings(self, mode: &str) -> &'static [(&'static str, &'static str)] {
        match (self, mode) {
            (Runtime::Codex, "read-only") => &[("approval_policy", "on-request"), ("sandbox_mode", "read-only")],
            (Runtime::Codex, "workspace-write") => {
                &[("approval_policy", "on-request"), ("sandbox_mode", "workspace-write")]
            }
            (Runtime::Codex, "agent") => &[
                ("approval_policy", "on-request"),
                ("approvals_reviewer", "auto_review"),
                ("sandbox_mode", "workspace-write"),
            ],
            (Runtime::Codex, "agent-full-access") => {
                &[("approval_policy", "never"), ("sandbox_mode", "danger-full-access")]
            }
            _ => &[],
        }
    }

    /// The variable that points the agent at its isolated config directory instead of the
    /// person's own `~/.codex` or `~/.claude`.
    // @project-doc docs/operations/20261031-security.md#agent_isolation
    pub fn config_dir_var(self) -> &'static str {
        match self {
            Runtime::Codex => "CODEX_HOME",
            Runtime::Claude => "CLAUDE_CONFIG_DIR",
        }
    }

    /// Where the person's own agent keeps its setup on this machine: what 「复制本机配置」 copies
    /// from unless another folder is chosen.
    pub fn user_config_dir(self) -> &'static str {
        match self {
            Runtime::Codex => "~/.codex",
            Runtime::Claude => "~/.claude",
        }
    }

    /// The variable that runs a CLI chosen on the agent instead of the one its adapter bundles.
    pub fn bin_var(self) -> &'static str {
        match self {
            Runtime::Codex => "CODEX_PATH",
            Runtime::Claude => "CLAUDE_CODE_EXECUTABLE",
        }
    }

    /// Defaults the agent's own variables can replace or unset (`KEY=`).
    // @project-doc docs/interfaces/20261031-acp_integration.md#request_timeout
    pub fn default_env(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Runtime::Codex => vec![],
            // Claude Code waits 10 minutes for a model response by default. A gateway that took
            // the request and never answers would hold the session that long before a retry.
            Runtime::Claude => vec![("API_TIMEOUT_MS", "120000")],
        }
    }

    /// The variables this runtime's sessions get on top of the isolation: the credentials of the
    /// connection's sign-in method (`secret` reads one of its saved secrets) and the settings Janus
    /// relies on.
    pub fn launch_env(self, conn: &Value, secret: impl Fn(&str) -> String) -> Vec<(&'static str, String)> {
        let mut env = vec![];
        match self {
            Runtime::Codex => {
                if conn["auth"] == "apikey" {
                    env.push(("OPENAI_API_KEY", secret("apiKey")));
                    let base_url = conn["baseUrl"].as_str().unwrap_or_default();
                    if !base_url.is_empty() {
                        let provider = json!({
                            "model_provider": "janus",
                            "model_providers": { "janus": { "name": "Janus gateway", "base_url": base_url, "env_key": "OPENAI_API_KEY", "wire_api": "responses", "requires_openai_auth": false } }
                        });
                        env.push(("CODEX_CONFIG", provider.to_string()));
                        env.push(("MODEL_PROVIDER", "janus".into()));
                    }
                }
            }
            Runtime::Claude => {
                // Janus reads a returned turn as the agent's answer. A command or subagent Claude
                // Code sends to the background would end the turn before the work is done.
                // @project-doc docs/interfaces/20261031-acp_integration.md#background_tasks
                env.push(("CLAUDE_CODE_DISABLE_BACKGROUND_TASKS", "1".into()));
                // Its own memory would be kept per working directory, and every run and chat has
                // its own: what a run learns goes to the workspace's team memory instead.
                // @project-doc docs/domains/20261123-team_memory.md#native_memory
                env.push(("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1".into()));
                if conn["auth"] == "oauth" {
                    env.push(("CLAUDE_CODE_OAUTH_TOKEN", secret("token")));
                }
            }
        }
        env
    }

    /// Whether the agent's own environment variables may carry credentials (Claude Code's custom
    /// API is set as environment lines), so the ones that are not plainly settings are kept with
    /// the secrets instead of in the stored connection.
    pub fn env_holds_secrets(self) -> bool {
        self == Runtime::Claude
    }

    /// How Janus's instructions reach this runtime's system layer.
    pub fn instruction_channel(self) -> InstructionChannel {
        match self {
            Runtime::Claude => InstructionChannel::System,
            Runtime::Codex => InstructionChannel::Developer,
        }
    }

    /// Where the runtime reads a bundle's skills, relative to the bundle: Claude Code loads the
    /// bundle as a local plugin, Codex reads `.agents/skills` under each additional root.
    pub fn skills_dir(self) -> &'static str {
        match self {
            Runtime::Claude => "skills",
            Runtime::Codex => ".agents/skills",
        }
    }

    /// The bundle is a Claude Code plugin, which needs its manifest (`.claude-plugin/plugin.json`).
    pub fn bundle_is_plugin(self) -> bool {
        self == Runtime::Claude
    }

    /// The `_meta` of `session/new` that has the runtime load a bundle of skills at `path`.
    pub fn bundle_meta(self, path: &str) -> Value {
        match self {
            Runtime::Claude => {
                json!({ "claudeCode": { "options": { "plugins": [{ "type": "local", "path": path }] } } })
            }
            Runtime::Codex => json!({ "additionalRoots": [path] }),
        }
    }

    /// What a session of this runtime is opened with on top of its resources: Claude Code is told
    /// that Janus's own board tools need no approval, and takes `system` (Janus's instructions for
    /// the session, when its channel is the system prompt) appended to its own system prompt.
    pub fn open_meta(self, meta: &mut Value, system: Option<&str>) {
        if self != Runtime::Claude {
            return;
        }
        // Janus's own board tools only read and write this run's board; in its default mode
        // Claude Code would ask the person every time a collaborator posts progress. Codex reads
        // the tools' read-only annotations for the same effect. Nothing else changes what the
        // role's mode allows.
        meta["claudeCode"]["options"]["settings"] = json!({ "permissions": { "allow": ["mcp__janus"] } });
        if let Some(text) = system {
            // Appended to Claude Code's own system prompt, never replacing it.
            meta["systemPrompt"] = json!({ "append": text });
        }
    }

    /// The runtime runs `/goal` only in a workspace it trusts, recorded in its own config
    /// (Claude Code), so Janus records the trust in the agent's isolated config first.
    pub fn trusts_workspaces(self) -> bool {
        self == Runtime::Claude
    }
}
