//! The library's picks: skills and MCP servers from the community worth having, added in one
//! click. Hand-picked and checked the way magpie picks its featured servers (each skill's folder
//! is in its repository, each server's package is published or its endpoint answers, none needs
//! an OAuth sign-in, which Janus can't do for a server). The list ships with Janus, so showing it
//! reaches no network; adding a skill imports it from its GitHub repository as a pasted link
//! would, and adding a server writes a new MCP server like the editor does.
// @project-doc docs/domains/20261116-resource_library.md#catalog

use super::mcp_draft::{Draft, Var};
use crate::lang::{Lang, tr};
use serde_json::{Value, json};

/// A skill in a GitHub repository: the folder holding its SKILL.md, empty when the SKILL.md is at
/// the repository's top.
pub struct Skill {
    pub repo: &'static str,
    pub path: &'static str,
    pub title: &'static str,
    pub publisher: &'static str,
    /// What it is for, in Chinese and in English.
    pub blurb: (&'static str, &'static str),
}

impl Skill {
    pub fn id(&self) -> String {
        if self.path.is_empty() { self.repo.to_string() } else { format!("{}/{}", self.repo, self.path) }
    }

    /// The folder the skill is in, which names it: its own, or the repository's.
    fn folder(&self) -> &str {
        let p = if self.path.is_empty() { self.repo } else { self.path };
        p.rsplit('/').next().unwrap_or(p)
    }

    /// Where the SKILL.md is, as the person reads it.
    pub fn location(&self) -> &str {
        if self.path.is_empty() { "SKILL.md" } else { self.path }
    }
}

/// Something a server needs before it works: an environment variable (command servers) or a
/// header (HTTP ones), and where to get it.
pub struct Input {
    pub key: &'static str,
    pub label: &'static str,
    pub hint: (&'static str, &'static str),
    /// The value around what the person gives, as `Bearer {}`.
    pub format: &'static str,
    pub required: bool,
}

pub struct Server {
    pub name: &'static str,
    pub title: &'static str,
    pub publisher: &'static str,
    pub blurb: (&'static str, &'static str),
    pub homepage: &'static str,
    /// `stdio` or `http`.
    pub transport: &'static str,
    pub command: &'static [&'static str],
    pub url: &'static str,
    pub inputs: &'static [Input],
}

impl Server {
    /// Added without asking anything: it needs nothing, or only what is optional.
    pub fn one_click(&self) -> bool {
        !self.inputs.iter().any(|i| i.required)
    }

    /// The server as the editor starts from it: the required inputs listed, each value written up
    /// to where the person's part goes ("Bearer ").
    pub fn draft(&self, lang: Lang) -> Draft {
        Draft {
            name: self.name.into(),
            description: lang.pick(self.blurb.0, self.blurb.1).into(),
            transport: self.transport.into(),
            command: self.command.first().map(|c| c.to_string()).unwrap_or_default(),
            args: self.command.iter().skip(1).map(|a| a.to_string()).collect(),
            url: self.url.into(),
            vars: self
                .inputs
                .iter()
                .filter(|i| i.required)
                .map(|i| Var { name: i.key.into(), value: i.format.replace("{}", "") })
                .collect(),
            ..Draft::default()
        }
    }

    /// What the editor says about the values to fill in.
    pub fn needs(&self, lang: Lang) -> String {
        self.inputs
            .iter()
            .map(|i| {
                let (key, label, hint) = (i.key, i.label, lang.pick(i.hint.0, i.hint.1));
                tr!(lang, "{key}：{label}，{hint}", "{key}: {label}, {hint}")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

const fn skill(
    repo: &'static str,
    path: &'static str,
    title: &'static str,
    publisher: &'static str,
    blurb: (&'static str, &'static str),
) -> Skill {
    Skill { repo, path, title, publisher, blurb }
}

/// In the order the shelf shows them (reviewed 2026-11-26): the browser first, then documents,
/// interfaces, making skills, the ways of writing code, and web practices.
pub const SKILLS: &[Skill] = &[
    skill(
        "citrolabs/ego-lite",
        "skills/ego-browser",
        "ego-browser",
        "Citro Labs",
        (
            "用 ego lite 浏览器打开网站、登录、填表、点击、截图、提取数据，可以带着你已登录的账号；需要先安装 ego lite。",
            "Open sites, sign in, fill in forms, click, take screenshots and pull data with the ego lite browser, using the accounts you're signed in to; needs ego lite installed.",
        ),
    ),
    skill(
        "anthropics/skills",
        "skills/pdf",
        "PDF",
        "Anthropic",
        (
            "读取、合并、拆分和填写 PDF，提取其中的文字和表格。",
            "Read, merge, split and fill in PDFs, and extract their text and tables.",
        ),
    ),
    skill(
        "anthropics/skills",
        "skills/docx",
        "Word",
        "Anthropic",
        (
            "创建和编辑 Word 文档，包括修订和批注。",
            "Create and edit Word documents, tracked changes and comments included.",
        ),
    ),
    skill(
        "anthropics/skills",
        "skills/xlsx",
        "Excel",
        "Anthropic",
        (
            "读写 Excel 表格，处理公式、格式和图表。",
            "Read and write Excel spreadsheets, with formulas, formatting and charts.",
        ),
    ),
    skill(
        "anthropics/skills",
        "skills/pptx",
        "PowerPoint",
        "Anthropic",
        ("创建和编辑 PowerPoint 演示文稿。", "Create and edit PowerPoint presentations."),
    ),
    skill(
        "Fenng/tech-doc-style-chinese",
        "",
        "中文技术文档规范",
        "Fenng",
        (
            "写、改、校对中文技术文档和产品文案时，用克制、准确、可扫读的中文技术写作风格。",
            "A restrained, precise, scannable style for writing, revising and proofreading Chinese technical docs and product copy.",
        ),
    ),
    skill(
        "anthropics/skills",
        "skills/frontend-design",
        "Frontend Design",
        "Anthropic",
        (
            "做出有设计感、不像模板的前端界面。",
            "Build distinctive front-end interfaces that don't look like a template.",
        ),
    ),
    skill(
        "anthropics/skills",
        "skills/webapp-testing",
        "Web App Testing",
        "Anthropic",
        (
            "用 Playwright 测试本地 Web 应用，截图、查看浏览器日志。",
            "Test local web apps with Playwright, with screenshots and browser logs.",
        ),
    ),
    skill(
        "anthropics/skills",
        "skills/skill-creator",
        "Skill Creator",
        "Anthropic",
        ("创建、改进和评测 Skill。", "Create, improve and evaluate skills."),
    ),
    skill(
        "obra/superpowers",
        "skills/systematic-debugging",
        "Systematic Debugging",
        "Superpowers",
        ("按找根因的步骤排查 bug，不靠猜。", "Track bugs down to their root cause step by step instead of guessing."),
    ),
    skill(
        "obra/superpowers",
        "skills/test-driven-development",
        "Test-Driven Development",
        "Superpowers",
        ("先写一个失败的测试，再写让它通过的实现。", "Write a failing test first, then the code that makes it pass."),
    ),
    skill(
        "obra/superpowers",
        "skills/writing-plans",
        "Writing Plans",
        "Superpowers",
        ("把需求拆成一步步可以执行的实现计划。", "Turn requirements into an implementation plan of concrete steps."),
    ),
    skill(
        "unclebob/ub-mode",
        "",
        "ub-mode",
        "Uncle Bob",
        (
            "写或改代码时按精简的原则走：先能用，再写对，再守规范。",
            "Reduced principles for writing or changing code: make it work, then make it right, then follow practice.",
        ),
    ),
    skill(
        "obra/superpowers",
        "skills/verification-before-completion",
        "Verification Before Completion",
        "Superpowers",
        ("说完成之前先运行验证，拿出证据。", "Run the checks and show evidence before calling the work done."),
    ),
    skill(
        "mattpocock/skills",
        "skills/productivity/grilling",
        "Grilling",
        "Matt Pocock",
        (
            "就一个计划、决定或想法反复追问：每轮问一批能问的问题并给出建议答案，直到想清楚。",
            "Question a plan, decision or idea relentlessly: each round asks what can be asked now, with recommended answers, until it is settled.",
        ),
    ),
    skill(
        "mattpocock/skills",
        "skills/engineering/improve-codebase-architecture",
        "Improve Codebase Architecture",
        "Matt Pocock",
        (
            "找出代码库里值得加深的模块，改善架构。",
            "Find modules worth deepening and improve the codebase's architecture.",
        ),
    ),
    skill(
        "vercel-labs/agent-skills",
        "skills/react-best-practices",
        "React Best Practices",
        "Vercel",
        ("Vercel 总结的 React 和 Next.js 性能最佳实践。", "Vercel's React and Next.js performance best practices."),
    ),
    skill(
        "vercel-labs/agent-skills",
        "skills/web-design-guidelines",
        "Web Design Guidelines",
        "Vercel",
        ("按 Web 界面规范审查 UI 代码。", "Review UI code against web interface guidelines."),
    ),
];

/// In the order the shelf shows them (reviewed 2026-11-26): documentation and search that need
/// nothing first, then GitHub, the browser's insides, models, and services that need a key.
pub const SERVERS: &[Server] = &[
    Server {
        name: "context7",
        title: "Context7",
        publisher: "Upstash",
        blurb: (
            "任意库最新的文档和代码示例，直接交给 Agent。",
            "Up-to-date docs and code examples for any library, straight to the Agent.",
        ),
        homepage: "https://context7.com",
        transport: "http",
        command: &[],
        url: "https://mcp.context7.com/mcp",
        inputs: &[Input {
            key: "CONTEXT7_API_KEY",
            label: "API key",
            hint: (
                "可选，提高调用上限；在 context7.com/dashboard 获取",
                "optional, raises the rate limit; get one at context7.com/dashboard",
            ),
            format: "{}",
            required: false,
        }],
    },
    Server {
        name: "deepwiki",
        title: "DeepWiki",
        publisher: "Cognition",
        blurb: (
            "就任意公开的 GitHub 仓库提问，阅读它生成的 wiki。",
            "Ask about any public GitHub repository and read its generated wiki.",
        ),
        homepage: "https://deepwiki.com",
        transport: "http",
        command: &[],
        url: "https://mcp.deepwiki.com/mcp",
        inputs: &[],
    },
    Server {
        name: "exa",
        title: "Exa",
        publisher: "Exa",
        blurb: (
            "为 Agent 做的网页搜索和代码搜索，返回干净的页面内容。",
            "Web and code search built for agents, with clean page contents.",
        ),
        homepage: "https://docs.exa.ai/reference/exa-mcp",
        transport: "http",
        command: &[],
        url: "https://mcp.exa.ai/mcp",
        inputs: &[],
    },
    Server {
        name: "github",
        title: "GitHub",
        publisher: "GitHub",
        blurb: (
            "GitHub 上的仓库、Issue、Pull Request、Actions 和代码搜索。",
            "Repositories, issues, pull requests, Actions and code search on GitHub.",
        ),
        homepage: "https://github.com/github/github-mcp-server",
        transport: "http",
        command: &[],
        url: "https://api.githubcopilot.com/mcp/",
        inputs: &[Input {
            key: "Authorization",
            label: "Personal access token",
            hint: (
                "在 github.com/settings/personal-access-tokens 创建",
                "create one at github.com/settings/personal-access-tokens",
            ),
            format: "Bearer {}",
            required: true,
        }],
    },
    Server {
        name: "chrome-devtools",
        title: "Chrome DevTools",
        publisher: "Google",
        blurb: (
            "控制和检查一个真实的 Chrome：控制台、网络、性能追踪和截图。",
            "Control and inspect a live Chrome: console, network, performance traces and screenshots.",
        ),
        homepage: "https://github.com/ChromeDevTools/chrome-devtools-mcp",
        transport: "stdio",
        command: &["npx", "-y", "chrome-devtools-mcp@latest"],
        url: "",
        inputs: &[],
    },
    Server {
        name: "huggingface",
        title: "Hugging Face",
        publisher: "Hugging Face",
        blurb: (
            "搜索 Hugging Face 上的模型、数据集、Spaces 和论文。",
            "Search models, datasets, Spaces and papers on the Hugging Face Hub.",
        ),
        homepage: "https://huggingface.co/settings/mcp",
        transport: "http",
        command: &[],
        url: "https://huggingface.co/mcp",
        inputs: &[],
    },
    Server {
        name: "firecrawl",
        title: "Firecrawl",
        publisher: "Firecrawl",
        blurb: ("抓取、爬取和搜索网页，转成干净的 Markdown。", "Scrape, crawl and search the web into clean Markdown."),
        homepage: "https://github.com/firecrawl/firecrawl-mcp-server",
        transport: "stdio",
        command: &["npx", "-y", "firecrawl-mcp"],
        url: "",
        inputs: &[Input {
            key: "FIRECRAWL_API_KEY",
            label: "API key",
            hint: ("在 firecrawl.dev/app/api-keys 获取", "get one at firecrawl.dev/app/api-keys"),
            format: "{}",
            required: true,
        }],
    },
    Server {
        name: "tavily",
        title: "Tavily",
        publisher: "Tavily",
        blurb: ("用 Tavily 搜索网页并提取内容。", "Search and extract from the web with Tavily."),
        homepage: "https://github.com/tavily-ai/tavily-mcp",
        transport: "stdio",
        command: &["npx", "-y", "tavily-mcp"],
        url: "",
        inputs: &[Input {
            key: "TAVILY_API_KEY",
            label: "API key",
            hint: ("在 app.tavily.com 获取", "get one at app.tavily.com"),
            format: "{}",
            required: true,
        }],
    },
];

/// The library's item a skill pick already is: a skill imported from the same folder of the same
/// repository, or one with its name.
fn skill_item<'a>(s: &Skill, items: &'a [Value], sources: &[Value]) -> Option<&'a Value> {
    items.iter().find(|i| {
        if i["kind"] != "skill" {
            return false;
        }
        if i["name"] == s.folder() {
            return true;
        }
        let o = &i["origin"];
        let Some(src) = sources.iter().find(|x| o["type"] == "git" && x["id"] == o["source"]) else { return false };
        let full: Vec<&str> = [src["path"].as_str().unwrap_or(""), o["path"].as_str().unwrap_or("")]
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect();
        src["repo"].as_str().is_some_and(|r| r.eq_ignore_ascii_case(s.repo)) && full.join("/") == s.path
    })
}

/// The library's server a pick already is: one by its name, or one that starts the same command
/// or reaches the same address.
fn server_item<'a>(s: &Server, items: &'a [Value]) -> Option<&'a Value> {
    items.iter().find(|i| {
        if i["kind"] != "mcp" {
            return false;
        }
        let srv = &i["server"];
        let command: Vec<&str> = std::iter::once(srv["command"].as_str().unwrap_or(""))
            .chain(srv["args"].as_array().into_iter().flatten().map(|a| a.as_str().unwrap_or("")))
            .collect();
        i["name"] == s.name
            || (!s.url.is_empty() && srv["url"] == s.url)
            || (!s.command.is_empty() && command == s.command)
    })
}

/// The picks as the shelf shows them (`GET /api/library/picks`): each tab's tiles, what the
/// library doesn't have yet first, each with the words a search matches.
pub fn shelf(library: &Value, lang: Lang) -> Value {
    let items = library["items"].as_array().map(Vec::as_slice).unwrap_or_default();
    let sources = library["sources"].as_array().map(Vec::as_slice).unwrap_or_default();
    let blurb = |b: (&str, &str)| lang.pick(b.0, b.1).to_string();
    let mut skills: Vec<Value> = SKILLS
        .iter()
        .map(|s| {
            json!({
                "id": s.id(), "icon": "sparkles", "title": s.title, "byline": format!("{} · {}", s.publisher, s.repo),
                "blurb": blurb(s.blurb), "tags": [], "have": skill_item(s, items, sources).is_some(),
                "search": format!("{} {} {} {}", s.title, s.path, s.publisher, blurb(s.blurb)).to_lowercase(),
            })
        })
        .collect();
    let mut servers: Vec<Value> = SERVERS
        .iter()
        .map(|s| {
            let mut tags = vec![if s.transport == "stdio" { lang.pick("命令", "Command") } else { "HTTP" }];
            if !s.one_click() {
                tags.push(lang.pick("需要 API key", "Needs an API key"));
            } else if !s.inputs.is_empty() {
                tags.push(lang.pick("API key 可选", "API key optional"));
            }
            json!({
                "id": s.name, "icon": if s.transport == "stdio" { "terminal" } else { "globe" }, "title": s.title,
                "byline": s.publisher, "homepage": s.homepage, "blurb": blurb(s.blurb), "tags": tags, "have": server_item(s, items).is_some(),
                "search": format!("{} {} {} {}", s.title, s.name, s.publisher, blurb(s.blurb)).to_lowercase(),
            })
        })
        .collect();
    // What the library doesn't have yet comes first: that is what the shelf is for.
    skills.sort_by_key(|t| t["have"] == true);
    servers.sort_by_key(|t| t["have"] == true);
    json!({ "skills": skills, "servers": servers })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_the_library_has_says_so_and_goes_last() {
        let library = json!({
            "items": [
                { "kind": "skill", "name": "x", "origin": { "type": "git", "source": "s1", "path": "pdf" } },
                { "kind": "mcp", "name": "mine", "server": { "url": "https://mcp.deepwiki.com/mcp" } },
                { "kind": "mcp", "name": "fc", "server": { "command": "npx", "args": ["-y", "firecrawl-mcp"] } },
            ],
            "sources": [{ "id": "s1", "repo": "Anthropics/skills", "path": "skills" }],
        });
        let shelf = shelf(&library, Lang::En);
        let skills = shelf["skills"].as_array().unwrap();
        assert_eq!(skills.len(), SKILLS.len());
        let pdf = skills.iter().position(|t| t["id"] == "anthropics/skills/skills/pdf").unwrap();
        assert_eq!(pdf, skills.len() - 1, "the one the library has is last");
        assert_eq!(skills[pdf]["have"], true);
        assert_eq!(skills[0]["byline"], "Citro Labs · citrolabs/ego-lite");
        let servers = shelf["servers"].as_array().unwrap();
        let have: Vec<&str> = servers.iter().filter(|t| t["have"] == true).map(|t| t["id"].as_str().unwrap()).collect();
        assert_eq!(have, ["deepwiki", "firecrawl"]);
        let github = servers.iter().find(|t| t["id"] == "github").unwrap();
        assert_eq!(github["tags"], json!(["HTTP", "Needs an API key"]));
        let context7 = servers.iter().find(|t| t["id"] == "context7").unwrap();
        assert_eq!(context7["tags"], json!(["HTTP", "API key optional"]));
        assert!(github["search"].as_str().unwrap().contains("pull requests"));
    }

    #[test]
    fn a_server_that_needs_a_key_starts_the_editor_with_what_to_fill_in() {
        let github = SERVERS.iter().find(|s| s.name == "github").unwrap();
        assert!(!github.one_click());
        let d = github.draft(Lang::Zh);
        assert_eq!((d.transport.as_str(), d.url.as_str()), ("http", "https://api.githubcopilot.com/mcp/"));
        assert_eq!(d.vars, [Var { name: "Authorization".into(), value: "Bearer ".into() }]);
        assert_eq!(
            github.needs(Lang::Zh),
            "Authorization：Personal access token，在 github.com/settings/personal-access-tokens 创建"
        );
        let tavily = SERVERS.iter().find(|s| s.name == "tavily").unwrap().draft(Lang::En);
        assert_eq!(
            (tavily.command.as_str(), tavily.args.clone()),
            ("npx", vec!["-y".to_string(), "tavily-mcp".into()])
        );
        assert!(SERVERS.iter().find(|s| s.name == "context7").unwrap().one_click());
        let ub = SKILLS.iter().find(|s| s.repo == "unclebob/ub-mode").unwrap();
        assert_eq!((ub.id(), ub.folder(), ub.location()), ("unclebob/ub-mode".to_string(), "ub-mode", "SKILL.md"));
        let context7 = SERVERS.iter().find(|s| s.name == "context7").unwrap();
        assert!(context7.draft(Lang::Zh).vars.is_empty(), "an optional key is added later, not left empty");
    }

    /// The picks are each one thing: ids and names unique, names the service accepts, every server
    /// either started by a command or reached at an https address.
    #[test]
    fn the_picks_are_well_formed() {
        let ids: std::collections::HashSet<String> = SKILLS.iter().map(Skill::id).collect();
        assert_eq!(ids.len(), SKILLS.len());
        let names: std::collections::HashSet<&str> = SERVERS.iter().map(|s| s.name).collect();
        assert_eq!(names.len(), SERVERS.len());
        for s in SERVERS {
            crate::library::mcp::check_name(s.name).unwrap();
            if s.transport == "stdio" {
                assert!(!s.command.is_empty() && s.url.is_empty(), "{}", s.name);
            } else {
                assert!(s.url.starts_with("https://") && s.command.is_empty(), "{}", s.name);
            }
        }
        for s in SKILLS {
            assert!(
                s.repo.split('/').count() == 2 && (s.path.is_empty() || s.path.starts_with("skills/")),
                "{}",
                s.id()
            );
        }
        assert_eq!(SKILLS[0].title, "ego-browser", "the browser comes first");
    }

    /// A skill pick is in the library when the same folder of the same repository was imported,
    /// the top of the repository for one whose SKILL.md is there, or a skill has its name.
    #[test]
    fn a_skill_pick_knows_its_folder() {
        let sources = [
            json!({ "id": "s1", "repo": "anthropics/skills", "path": "" }),
            json!({ "id": "s2", "repo": "unclebob/ub-mode", "path": "" }),
        ];
        let items = [
            json!({ "kind": "skill", "name": "pdf-tools", "origin": { "type": "git", "source": "s1", "path": "skills/pdf" } }),
            json!({ "kind": "skill", "name": "uncle-bob", "origin": { "type": "git", "source": "s2", "path": "" } }),
        ];
        let find = |repo: &str, path: &str| SKILLS.iter().find(|s| s.repo == repo && s.path == path).unwrap();
        assert_eq!(skill_item(find("anthropics/skills", "skills/pdf"), &items, &sources).unwrap()["name"], "pdf-tools");
        assert!(skill_item(find("anthropics/skills", "skills/docx"), &items, &sources).is_none());
        assert_eq!(skill_item(find("unclebob/ub-mode", ""), &items, &sources).unwrap()["name"], "uncle-bob");
        let named = [json!({ "kind": "skill", "name": "tech-doc-style-chinese" })];
        assert!(skill_item(find("Fenng/tech-doc-style-chinese", ""), &named, &[]).is_some(), "by its name");
    }
}
