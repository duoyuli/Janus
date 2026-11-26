//! A digital employee as a file: what the default roles ship as, what people import and what
//! export writes. One Markdown file per employee, `<id>.md`: front matter with its name, duty,
//! group and look, then its instructions; an optional `<id>.png` beside it is its picture. A
//! Claude Code subagent file (`name`, `description`, instructions) reads the same way.
// @project-doc docs/domains/20261031-agents_and_roles.md#role_files

use super::*;
use crate::front_matter;
use roles::{MAX_AVATAR_BYTES, Role, RoleInput};
use serde::Serialize;

/// What a role file says about one digital employee.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct RoleFile {
    /// Empty when the file names none; importing then takes one from the file's name.
    pub id: String,
    pub name: String,
    pub desc: String,
    /// Empty when the file names none; importing then takes the folder's name.
    pub group: String,
    pub color: String,
    pub init: String,
    pub lead: bool,
    pub prompt: String,
}

/// The longest group name: it heads a list and names an export's folder.
pub const MAX_GROUP: usize = 40;

impl RoleFile {
    /// Read a role file: `---`, `key: value` lines (`name` and the instructions after the closing
    /// `---` are required; `desc` may be written `description`), `---`, then the instructions.
    pub fn parse(text: &str) -> Result<RoleFile, String> {
        let text = text.replace("\r\n", "\n");
        if front_matter::yaml(&text).is_none() {
            return Err("开头没有 --- 包起来的元数据".into());
        }
        let get = |key: &str| front_matter::field(&text, key).unwrap_or_default();
        let name = get("name");
        if name.is_empty() {
            return Err("元数据里没有 name".into());
        }
        let desc = match get("desc") {
            d if d.is_empty() => duty(&get("description")),
            d => d,
        };
        let lead = match get("lead").as_str() {
            "" | "false" => false,
            "true" => true,
            other => return Err(format!("lead 应当是 true 或 false，而不是 {other}")),
        };
        let prompt = front_matter::body(&text).trim().to_string();
        if prompt.is_empty() {
            return Err("没有指令正文".into());
        }
        let color = Some(get("color")).filter(|c| is_hex_color(c)).unwrap_or_default();
        let group = group_name(&get("group")).map_err(|e| e.to_string())?;
        Ok(RoleFile { id: get("id"), name, desc, group, color, init: get("init"), lead, prompt })
    }

    /// The file for a saved digital employee; `parse` reads it back the same.
    pub fn of(role: &Role) -> RoleFile {
        RoleFile {
            id: role.id.clone(),
            name: role.name.clone(),
            desc: role.desc.clone(),
            group: role.group.clone(),
            color: role.color.clone(),
            init: role.init.clone(),
            lead: role.lead,
            prompt: role.prompt.clone(),
        }
    }

    pub fn render(&self) -> String {
        let q = front_matter::quote;
        let mut head =
            vec![format!("id: {}", self.id), format!("name: {}", q(&self.name)), format!("desc: {}", q(&self.desc))];
        if !self.group.is_empty() {
            head.push(format!("group: {}", q(&self.group)));
        }
        head.extend([
            format!("color: {}", q(&self.color)),
            format!("init: {}", q(&self.init)),
            format!("lead: {}", self.lead),
        ]);
        format!("---\n{}\n---\n\n{}\n", head.join("\n"), self.prompt.trim())
    }
}

/// A subagent's `description` as a duty: the lead reads every collaborator's duty, so only its
/// first paragraph, without the `<example>` blocks some carry (written with literal `\n`).
fn duty(description: &str) -> String {
    let text = description.replace("\\n", "\n");
    let text = text.split("<example>").next().unwrap_or_default();
    text.split("\n\n").next().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_hex_color(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|h| h.is_ascii_hexdigit())
}

/// A role file found under the folder (or the file) the person chose.
#[derive(Serialize)]
pub struct Found {
    /// Relative to the chosen folder; the file's own name when a file was chosen.
    pub file: String,
    #[serde(flatten)]
    pub role: RoleFile,
    /// Whether a picture (`<name>.png`) sits beside it.
    pub picture: bool,
    /// A saved digital employee by the same name, which the app leaves unticked.
    pub existing: Option<String>,
}

/// Every role file under `root`, and the Markdown files that looked like one but do not read.
#[derive(Serialize, Default)]
pub struct Scan {
    pub roles: Vec<Found>,
    /// `(file, why)`.
    pub skipped: Vec<(String, String)>,
    /// More files than are listed, or folders deeper than are read.
    pub limited: bool,
}

const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 500;
/// A role file is instructions, far smaller than this.
const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Look for role files at `path`: the file itself, or the `.md` files under the folder (at most
/// `MAX_DEPTH` deep), skipping hidden entries, `node_modules` and symbolic links. A Markdown file
/// without front matter (a README) is not a role file and is not reported.
pub fn scan(path: &Path) -> Result<Scan> {
    let meta = std::fs::symlink_metadata(path).with_context(|| format!("找不到 {}", path.display()))?;
    let mut files = vec![];
    let mut scan = Scan::default();
    let root = if meta.is_dir() {
        walk(path, 0, &mut files, &mut scan.limited);
        path
    } else if meta.is_file() {
        files.push(path.to_path_buf());
        path.parent().unwrap_or(path)
    } else {
        bail!("{} 不是文件或文件夹", path.display());
    };
    for file in files {
        let rel = file.strip_prefix(root).unwrap_or(&file).to_string_lossy().into_owned();
        let read = std::fs::metadata(&file)
            .map_err(|e| e.to_string())
            .and_then(|m| if m.len() > MAX_FILE_BYTES { Err("文件超过 256 KB".into()) } else { Ok(()) })
            .and_then(|_| std::fs::read_to_string(&file).map_err(|e| e.to_string()));
        let text = match read {
            Ok(t) => t,
            Err(why) => {
                scan.skipped.push((rel, why));
                continue;
            }
        };
        if front_matter::yaml(&text.replace("\r\n", "\n")).is_none() {
            continue;
        }
        match RoleFile::parse(&text) {
            Ok(mut role) => {
                if role.group.is_empty() && meta.is_dir() {
                    let folder = file.parent().and_then(Path::file_name).unwrap_or_default();
                    role.group = folder.to_string_lossy().chars().take(MAX_GROUP).collect();
                }
                let picture = picture_of(&file).is_some();
                scan.roles.push(Found { file: rel, role, picture, existing: None });
            }
            Err(why) => scan.skipped.push((rel, why)),
        }
    }
    Ok(scan)
}

fn walk(dir: &Path, depth: usize, files: &mut Vec<PathBuf>, limited: &mut bool) {
    if depth > MAX_DEPTH {
        *limited = true;
        return;
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir).into_iter().flatten().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            walk(&e.path(), depth + 1, files, limited);
        } else if kind.is_file() && name.ends_with(".md") {
            if files.len() == MAX_FILES {
                *limited = true;
                return;
            }
            files.push(e.path());
        }
    }
}

/// The picture beside a role file (`<name>.png`), when it is a regular PNG file of a sane size.
fn picture_of(file: &Path) -> Option<Vec<u8>> {
    let png = file.with_extension("png");
    let meta = std::fs::symlink_metadata(&png).ok()?;
    if !meta.is_file() || meta.len() > MAX_AVATAR_BYTES as u64 {
        return None;
    }
    std::fs::read(&png).ok().filter(|b| b.starts_with(b"\x89PNG\r\n\x1a\n"))
}

impl App {
    /// The role files at `path`, each marked with the saved digital employee of the same name.
    pub fn scan_role_files(&self, path: &Path) -> Result<Scan> {
        let mut scan = scan(path)?;
        let saved = self.db.list::<Role>()?;
        for f in &mut scan.roles {
            f.existing = saved.iter().find(|r| r.name == f.role.name).map(|r| r.id.clone());
        }
        Ok(scan)
    }

    /// Make a digital employee of each of `files` (as `scan_role_files` listed them) at `path`,
    /// all on the agent `connection` (empty: none yet). Files are read again here, not taken from
    /// the app. An id already taken gets `-2`, `-3`, …: importing never replaces anyone.
    pub fn import_roles(&self, path: &Path, files: &[String], connection: &str) -> Result<Vec<Role>> {
        if !connection.is_empty() {
            self.connection(connection)?;
        }
        let scan = scan(path)?;
        let root = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        let mut made = vec![];
        for file in files {
            let found = scan
                .roles
                .iter()
                .find(|f| &f.file == file)
                .ok_or_else(|| anyhow!("{file} 不是能导入的数字员工文件"))?;
            let r = &found.role;
            let stem = Path::new(&found.file).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let base = [r.id.as_str(), stem.as_str(), r.name.as_str()]
                .into_iter()
                .find_map(|raw| crate::text::slug(raw, 36))
                .unwrap_or_else(|| new_id("r"));
            let id = self.free_role_id(&base)?;
            let input = RoleInput {
                id,
                name: r.name.clone(),
                desc: r.desc.clone(),
                group: r.group.clone(),
                prompt: r.prompt.clone(),
                connection: connection.into(),
                lead: r.lead,
                color: r.color.clone(),
                init: r.init.clone(),
                ..Default::default()
            };
            let mut role = self.save_role(&input)?;
            if let Some(png) = picture_of(&root.join(&found.file)) {
                role = self.put_role_avatar(role, &png)?;
            }
            made.push(role);
        }
        Ok(made)
    }

    /// `base`, or `base-2`, `base-3`, … when a digital employee already has it.
    fn free_role_id(&self, base: &str) -> Result<String> {
        for n in 1.. {
            let id = if n == 1 { base.to_string() } else { format!("{base}-{n}") };
            if self.db.get::<Role>(&id)?.is_none() {
                return Ok(id);
            }
        }
        unreachable!()
    }

    /// The file for digital employee `id` (`<id>.md`), as export writes it.
    pub fn role_file(&self, id: &str) -> Result<String> {
        Ok(RoleFile::of(&self.role(id)?).render())
    }

    /// Give every digital employee of group `from` the name `to`. Returns how many moved.
    pub fn rename_role_group(&self, from: &str, to: &str) -> Result<usize> {
        let to = group_name(to)?;
        if from.is_empty() || to.is_empty() {
            bail!("分组需要一个名字");
        }
        let mut moved = 0;
        for mut role in self.db.list::<Role>()?.into_iter().filter(|r| r.group == from) {
            role.group = to.clone();
            self.db.put(&role, now_ms())?;
            moved += 1;
        }
        Ok(moved)
    }

    /// Put every digital employee of `group` (empty: those in no group) on the agent
    /// `connection`, as if each were saved with it: one already on it keeps its model and
    /// options, the others take the agent's defaults.
    pub fn bind_role_group(&self, group: &str, connection: &str) -> Result<Vec<Role>> {
        self.connection(connection)?;
        let mut out = vec![];
        for role in self.db.list::<Role>()?.into_iter().filter(|r| r.group == group) {
            let same = role.connection == connection;
            let input = RoleInput {
                connection: connection.into(),
                model: if same { role.model.clone() } else { String::new() },
                effort: if same { role.effort.clone() } else { String::new() },
                session: if same { role.session.clone() } else { json!({}) },
                ..RoleInput::from(&role)
            };
            out.push(self.save_role(&input)?);
        }
        Ok(out)
    }
}

/// A group name as saved: one line, trimmed, at most `MAX_GROUP` characters.
pub fn group_name(raw: &str) -> Result<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().count() > MAX_GROUP {
        bail!("分组名最多 {MAX_GROUP} 个字");
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::test_app;

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("janus-role-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_role_file_reads_back_as_written() {
        let file = RoleFile {
            id: "counsel".into(),
            name: "法务 \"顾问\"".into(),
            desc: "审合同: 找风险".into(),
            group: "律所".into(),
            color: "#11845a".into(),
            init: "法".into(),
            lead: true,
            prompt: "逐条读合同。\n\n- 标出风险".into(),
        };
        assert_eq!(RoleFile::parse(&file.render()).unwrap(), file);
        assert!(RoleFile::parse("no front matter").is_err());
        assert!(RoleFile::parse("---\ndesc: d\n---\nbody").unwrap_err().contains("name"));
        assert!(RoleFile::parse("---\nname: n\n---\n\n").unwrap_err().contains("指令"));
        assert!(RoleFile::parse("---\nname: n\nlead: yes\n---\nbody").unwrap_err().contains("lead"));
        let long = format!("---\nname: n\ngroup: {}\n---\nbody", "组".repeat(41));
        assert!(
            RoleFile::parse(&long).unwrap_err().contains("分组"),
            "a group too long to save is caught before importing"
        );
    }

    /// A Claude Code subagent imports as is: its description's first paragraph is the duty, the
    /// examples and the agent's own settings (tools, model, a named color) are left out.
    #[test]
    fn a_claude_code_subagent_reads_as_a_role() {
        let text = "---\nname: code-reviewer\ndescription: Reviews diffs for bugs.\\n\\n<example>Context: a PR</example>\ntools: Read, Grep\nmodel: sonnet\ncolor: blue\n---\n\nYou review code.\n";
        let r = RoleFile::parse(text).unwrap();
        assert_eq!(
            (r.name.as_str(), r.desc.as_str(), r.prompt.as_str()),
            ("code-reviewer", "Reviews diffs for bugs.", "You review code.")
        );
        assert_eq!((r.color.as_str(), r.id.as_str(), r.lead), ("", "", false));
    }

    #[test]
    fn importing_makes_new_employees_and_never_replaces_anyone() {
        let home = dir("import");
        let app = test_app(&home.join("home"));
        let src = home.join("team");
        std::fs::create_dir_all(src.join("legal/.hidden")).unwrap();
        std::fs::create_dir_all(src.join("node_modules")).unwrap();
        let write = |p: &str, t: &str| std::fs::write(src.join(p), t).unwrap();
        write("legal/counsel.md", "---\nname: 法务\ndesc: 审合同\nlead: true\n---\n审。");
        write("legal/paralegal.md", "---\nname: 助理\ndesc: 整理材料\ngroup: 律所\n---\n整理。");
        write("legal/README.md", "# 这个团队\n");
        write("legal/broken.md", "---\nname: 坏的\n---\n");
        write("legal/.hidden/x.md", "---\nname: 藏着的\n---\nx");
        write("node_modules/y.md", "---\nname: 依赖里的\n---\ny");
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), b"picture"].concat();
        std::fs::write(src.join("legal/counsel.png"), &png).unwrap();
        app.save_role(&crate::agents::input(json!({ "id": "counsel", "name": "法务" }))).unwrap();

        let scan = app.scan_role_files(&src).unwrap();
        let files: Vec<_> = scan.roles.iter().map(|f| f.file.as_str()).collect();
        assert_eq!(
            files,
            ["legal/counsel.md", "legal/paralegal.md"],
            "hidden, dependencies and READMEs are not role files"
        );
        assert_eq!(scan.skipped.len(), 1, "a broken role file says why");
        assert_eq!(scan.roles[0].role.group, "legal", "no group: the folder's name");
        assert_eq!(scan.roles[1].role.group, "律所", "the file's own group");
        assert_eq!(scan.roles[0].existing.as_deref(), Some("counsel"));
        assert!(scan.roles[0].picture);

        assert!(app.import_roles(&src, &["legal/counsel.md".into()], "no-such-agent").is_err());
        assert!(app.import_roles(&src, &["../outside.md".into()], "").is_err(), "only what the scan found");
        let made = app.import_roles(&src, &["legal/counsel.md".into(), "legal/paralegal.md".into()], "").unwrap();
        assert_eq!(made[0].id, "counsel-2", "an id in use is never replaced");
        assert_eq!(app.role("counsel").unwrap().desc, "", "the saved one is untouched");
        assert!(made[0].lead && made[0].avatar.len() == 16, "lead and picture come along");
        assert_eq!((made[1].id.as_str(), made[1].group.as_str()), ("paralegal", "律所"));
        // A single file chosen: no folder to name a group after.
        let one = app.scan_role_files(&src.join("legal/counsel.md")).unwrap();
        assert_eq!((one.roles[0].file.as_str(), one.roles[0].role.group.as_str()), ("counsel.md", ""));
        // What export writes imports back the same.
        assert_eq!(RoleFile::parse(&app.role_file("paralegal").unwrap()).unwrap(), RoleFile::of(&made[1]));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_group_is_renamed_and_put_on_one_agent_together() {
        let home = dir("groups");
        let app = test_app(&home);
        for (id, group) in [("a", "旧组"), ("b", "旧组"), ("c", "")] {
            app.save_role(&crate::agents::input(json!({ "id": id, "name": id, "group": group }))).unwrap();
        }
        assert!(app.save_role(&crate::agents::input(json!({ "name": "x", "group": "长".repeat(41) }))).is_err());
        assert_eq!(app.rename_role_group("旧组", "  新\n组 ").unwrap(), 2);
        assert_eq!(app.role("a").unwrap().group, "新 组");
        assert!(app.rename_role_group("新 组", " ").is_err(), "a group keeps a name");
        assert!(app.bind_role_group("新 组", "missing").is_err());
        app.db
            .put_config(
                ConfigKind::Connection,
                "cc",
                &crate::agents::test_connection("cc", "claude", "claude", json!({})),
                1,
            )
            .unwrap();
        let bound = app.bind_role_group("新 组", "cc").unwrap();
        assert_eq!(bound.len(), 2);
        assert!(bound.iter().all(|r| r.connection == "cc"));
        assert_eq!(app.role("c").unwrap().connection, "", "other groups are left alone");
        let _ = std::fs::remove_dir_all(&home);
    }
}
