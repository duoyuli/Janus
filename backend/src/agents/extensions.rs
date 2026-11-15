//! What a role mounts on top of its agent: Janus's own Skill library, project Skills, and MCP
//! servers from the library.

use super::*;

impl App {
    pub fn delete_skill(&self, id: &str) -> Result<()> {
        check_skill_id(id)?;
        if let Some(role) = self.db.list_config(ROLE)?.iter().find(|r| {
            r["skills"]
                .as_array()
                .is_some_and(|s| s.iter().any(|x| x == id))
        }) {
            bail!("角色「{}」挂载了这个 Skill，请先移除", str_of(role, "name"));
        }
        std::fs::remove_dir_all(skill_library().join(id)).context("删除 Skill 失败")
    }
}

/// Janus's own skill library. Agents never read the user's agent skill folders; a skill enters
/// Janus by being written here (created in the UI, or copied once from a folder the user picks).
pub fn skill_library() -> PathBuf {
    crate::config::settings().home.join("skills")
}

pub(crate) fn check_skill_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if !ok {
        bail!("Skill ID 只能用小写字母、数字、- 和 _");
    }
    Ok(())
}

/// A skill is a folder with SKILL.md; its front matter gives the name and the one-line description.
pub(crate) fn read_skill(dir: &Path, source: &str) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let id = dir.file_name()?.to_string_lossy().to_string();
    let mut name = id.clone();
    let mut desc = String::new();
    if let Some(front) = text
        .strip_prefix("---")
        .and_then(|t| t.split_once("\n---"))
        .map(|(f, _)| f)
    {
        let lines: Vec<&str> = front.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            if let Some(v) = line.strip_prefix("name:") {
                name = v.trim().trim_matches(['"', '\'']).to_string();
            } else if let Some(v) = line.strip_prefix("description:") {
                desc = v.trim().trim_matches(['"', '\'']).to_string();
                // YAML block scalars (`>`, `|`, `>-`) continue on the indented lines that follow.
                if matches!(desc.as_str(), ">" | "|" | ">-" | "|-" | ">+" | "|+") {
                    desc = lines[i + 1..]
                        .iter()
                        .take_while(|l| l.starts_with(' ') || l.is_empty())
                        .map(|l| l.trim())
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string();
                }
            }
        }
    }
    Some(
        json!({ "id": id, "path": dir, "name": name, "description": short_desc(&desc), "source": source }),
    )
}

pub(super) fn short_desc(s: &str) -> String {
    let s: String = s.chars().take(240).collect();
    if s.chars().count() == 240 {
        format!("{s}…")
    } else {
        s
    }
}

pub fn list_skills() -> Value {
    let root = skill_library();
    let mut out: Vec<Value> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter_map(|d| read_skill(&d, "Janus"))
        .collect();
    out.sort_by(|a, b| {
        a["name"]
            .as_str()
            .unwrap_or("")
            .to_lowercase()
            .cmp(&b["name"].as_str().unwrap_or("").to_lowercase())
    });
    json!({ "skills": out, "root": root })
}

pub(super) fn unique_skill_id(want: &str) -> Result<String> {
    let base = slug(want).unwrap_or_else(|| "skill".into());
    let root = skill_library();
    (1..1000)
        .map(|i| {
            if i == 1 {
                base.clone()
            } else {
                format!("{base}-{i}")
            }
        })
        .find(|id| !root.join(id).exists())
        .context("没有可用的 Skill ID")
}

/// Write a new skill into the library from the editor.
pub fn create_skill(body: &Value) -> Result<Value> {
    let name = str_of(body, "name");
    let content = body["content"].as_str().unwrap_or("").trim();
    if name.is_empty() || content.is_empty() {
        bail!("Skill 需要名称和内容");
    }
    let id = unique_skill_id(name)?;
    let dir = skill_library().join(&id);
    std::fs::create_dir_all(&dir)?;
    let text = if content.starts_with("---") {
        content.to_string()
    } else {
        format!(
            "---\nname: {name}\ndescription: {}\n---\n\n{content}\n",
            str_of(body, "description").replace('\n', " ")
        )
    };
    std::fs::write(dir.join("SKILL.md"), text)?;
    read_skill(&dir, "Janus").context("写入 Skill 失败")
}

/// Copy a skill folder the user picked into the library. Only regular files are copied, symlinks
/// are resolved to their content, and nothing outside the chosen folder is followed.
pub fn import_skill(path: &str) -> Result<Value> {
    let src = PathBuf::from(expand_home(path.trim()))
        .canonicalize()
        .context("文件夹不存在")?;
    if !src.join("SKILL.md").is_file() {
        bail!("这个文件夹里没有 SKILL.md");
    }
    let id = unique_skill_id(
        &src.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
    )?;
    let dest = skill_library().join(&id);
    let mut total = 0u64;
    copy_tree(&src, &src, &dest, &mut total)?;
    read_skill(&dest, "Janus").context("导入 Skill 失败")
}

pub(super) fn copy_tree(root: &Path, from: &Path, to: &Path, total: &mut u64) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let real = match entry.path().canonicalize() {
            Ok(p) if p.starts_with(root) || p.is_file() => p,
            _ => continue,
        };
        if real.is_dir() {
            if real.starts_with(root) {
                copy_tree(root, &real, &to.join(&name), total)?;
            }
        } else if real.is_file() {
            *total += real.metadata()?.len();
            if *total > 20 * 1024 * 1024 {
                bail!("Skill 超过 20MB，没有导入");
            }
            std::fs::copy(&real, to.join(&name))?;
        }
    }
    Ok(())
}

/// Skills that belong to the project itself, found inside the task's own workspace.
/// These are the only skills an agent reads from outside Janus's library.
pub fn project_skills(workspace: &Path) -> Vec<Value> {
    let mut out = vec![];
    for rel in [
        ".agents/skills",
        ".claude/skills",
        ".codex/skills",
        ".pi/skills",
        ".janus/skills",
    ] {
        for e in std::fs::read_dir(workspace.join(rel))
            .into_iter()
            .flatten()
            .flatten()
        {
            let p = e.path();
            if p.canonicalize()
                .is_ok_and(|real| real.starts_with(workspace.canonicalize().unwrap_or_default()))
                && let Some(mut s) = read_skill(&p, "项目") {
                    s["path"] = json!(format!("{rel}/{}", e.file_name().to_string_lossy()));
                    out.push(s);
                }
        }
    }
    out
}

/// Normalize an MCP entry from the role editor.
pub(super) fn resolve_mcp(m: &Value) -> Result<Value> {
    let m = m.clone();
    let name = str_of(&m, "name").to_string();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("MCP 名称只能包含字母、数字、- 和 _：{name}");
    }
    if reserved_mcp_name(&name) {
        bail!("MCP 名称 {name} 规范化后是 janus，保留给 Janus 看板，请换一个名字");
    }
    let (command, args) = match (&m["args"], str_of(&m, "command")) {
        (Value::Array(a), cmd) => (
            cmd.to_string(),
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect::<Vec<_>>(),
        ),
        (_, line) => {
            let mut parts = split_command(line)?;
            if parts.is_empty() {
                bail!("MCP {name} 没有启动命令");
            }
            let cmd = parts.remove(0);
            (cmd, parts)
        }
    };
    if command.is_empty() {
        bail!("MCP {name} 没有启动命令");
    }
    let env = match &m["envText"] {
        Value::String(t) if !t.trim().is_empty() => Value::Object(parse_env(t)?),
        _ => match &m["env"] {
            Value::Object(o) => Value::Object(
                o.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            json!(
                                v.as_str()
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| v.to_string())
                            ),
                        )
                    })
                    .collect(),
            ),
            _ => json!({}),
        },
    };
    Ok(json!({ "name": name, "transport": "stdio", "command": expand_home(&command), "args": args, "env": env }))
}
