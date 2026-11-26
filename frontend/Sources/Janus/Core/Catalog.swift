import Foundation

/// The library's picks: skills and MCP servers from the community worth having, added in one
/// click. Hand-picked and checked the way magpie picks its featured servers (each skill's folder
/// is in its repository, each server's package is published or its endpoint answers, none needs
/// an OAuth sign-in, which Janus can't do for a server). The list ships with the app, so showing
/// it reaches no network; adding a skill imports it from its GitHub repository as a pasted link
/// would, and adding a server writes a new MCP server like the editor does.
// @project-doc docs/domains/20261116-resource_library.md#catalog
enum Catalog {
    /// A skill in a GitHub repository: the folder holding its SKILL.md, empty when the SKILL.md
    /// is at the repository's top.
    struct Skill: Identifiable, Equatable {
        let repo: String
        var path = ""
        let title: String
        let publisher: String
        /// What it is for, in Chinese; `L.catalogText` gives the app's language.
        let blurb: String
        var id: String { path.isEmpty ? repo : "\(repo)/\(path)" }
        /// The folder the skill is in, which names it: its own, or the repository's.
        var folder: String { ((path.isEmpty ? repo : path) as NSString).lastPathComponent }
        /// Where the SKILL.md is, as the person reads it.
        var location: String { path.isEmpty ? "SKILL.md" : path }
    }

    /// Something a server needs from the user before it works: an environment variable (command
    /// servers) or a header (HTTP ones), and where to get it.
    struct Input: Equatable {
        let key: String
        let label: String
        let hint: String
        /// The value around what the user gives, as `Bearer {}`.
        var format = "{}"
        var required = true
    }

    struct Server: Identifiable, Equatable {
        let name: String
        let title: String
        let publisher: String
        let blurb: String
        let homepage: String
        let transport: McpDraft.Transport
        var command: [String] = []
        var url = ""
        var process: McpDraft.Process = .perRun
        var inputs: [Input] = []
        var id: String { name }

        /// Added without asking anything: it needs nothing, or only what is optional.
        var oneClick: Bool { !inputs.contains(where: \.required) }

        /// The server as the editor starts from it: the required inputs listed, each value
        /// written up to where the user's part goes ("Bearer ").
        var draft: McpDraft {
            var d = McpDraft()
            d.name = name
            d.description = L.catalogText(blurb)
            d.transport = transport
            d.command = command.first ?? ""
            d.args = Array(command.dropFirst())
            d.url = url
            d.process = process
            d.vars = inputs.filter(\.required).map { McpDraft.Var(name: $0.key, value: $0.format.replacingOccurrences(of: "{}", with: "")) }
            return d
        }

        /// What the editor says about the values to fill in.
        var needs: String {
            inputs.map { L.text("\($0.key)：\(L.catalogText($0.label))，\(L.catalogText($0.hint))") }.joined(separator: "\n")
        }
    }

    // MARK: Lookups

    /// The library's item a pick already is: a skill imported from the same folder of the same
    /// repository, or one with its name.
    static func item(for s: Skill, items: [JSON], sources: [JSON]) -> JSON? {
        return items.first { i in
            guard i["kind"].str == "skill" else { return false }
            if i["name"].str == s.folder { return true }
            let o = i["origin"]
            guard o["type"].str == "git", let src = sources.first(where: { $0["id"].str == o["source"].str }) else { return false }
            let full = [src["path"].str, o["path"].str].filter { !$0.isEmpty }.joined(separator: "/")
            return src["repo"].str.lowercased() == s.repo.lowercased() && full == s.path
        }
    }

    /// The library's server a pick already is: one by its name, or one that starts the same
    /// command or reaches the same address.
    static func item(for s: Server, items: [JSON]) -> JSON? {
        items.first { i in
            guard i["kind"].str == "mcp" else { return false }
            let srv = i["server"]
            return i["name"].str == s.name
                || (!s.url.isEmpty && srv["url"].str == s.url)
                || (!s.command.isEmpty && [srv["command"].str] + srv["args"].strings == s.command)
        }
    }

    // MARK: Skills

    /// In the order the shelf shows them (reviewed 2026-11-26): the browser first, then
    /// documents, interfaces, making skills, the ways of writing code, and web practices.
    static let skills: [Skill] = [
        Skill(repo: "citrolabs/ego-lite", path: "skills/ego-browser", title: "ego-browser", publisher: "Citro Labs", blurb: "用 ego lite 浏览器打开网站、登录、填表、点击、截图、提取数据，可以带着你已登录的账号；需要先安装 ego lite。"),
        Skill(repo: "anthropics/skills", path: "skills/pdf", title: "PDF", publisher: "Anthropic", blurb: "读取、合并、拆分和填写 PDF，提取其中的文字和表格。"),
        Skill(repo: "anthropics/skills", path: "skills/docx", title: "Word", publisher: "Anthropic", blurb: "创建和编辑 Word 文档，包括修订和批注。"),
        Skill(repo: "anthropics/skills", path: "skills/xlsx", title: "Excel", publisher: "Anthropic", blurb: "读写 Excel 表格，处理公式、格式和图表。"),
        Skill(repo: "anthropics/skills", path: "skills/pptx", title: "PowerPoint", publisher: "Anthropic", blurb: "创建和编辑 PowerPoint 演示文稿。"),
        Skill(repo: "Fenng/tech-doc-style-chinese", title: "中文技术文档规范", publisher: "Fenng", blurb: "写、改、校对中文技术文档和产品文案时，用克制、准确、可扫读的中文技术写作风格。"),
        Skill(repo: "anthropics/skills", path: "skills/frontend-design", title: "Frontend Design", publisher: "Anthropic", blurb: "做出有设计感、不像模板的前端界面。"),
        Skill(repo: "anthropics/skills", path: "skills/webapp-testing", title: "Web App Testing", publisher: "Anthropic", blurb: "用 Playwright 测试本地 Web 应用，截图、查看浏览器日志。"),
        Skill(repo: "anthropics/skills", path: "skills/skill-creator", title: "Skill Creator", publisher: "Anthropic", blurb: "创建、改进和评测 Skill。"),
        Skill(repo: "obra/superpowers", path: "skills/systematic-debugging", title: "Systematic Debugging", publisher: "Superpowers", blurb: "按找根因的步骤排查 bug，不靠猜。"),
        Skill(repo: "obra/superpowers", path: "skills/test-driven-development", title: "Test-Driven Development", publisher: "Superpowers", blurb: "先写一个失败的测试，再写让它通过的实现。"),
        Skill(repo: "obra/superpowers", path: "skills/writing-plans", title: "Writing Plans", publisher: "Superpowers", blurb: "把需求拆成一步步可以执行的实现计划。"),
        Skill(repo: "unclebob/ub-mode", title: "ub-mode", publisher: "Uncle Bob", blurb: "写或改代码时按精简的原则走：先能用，再写对，再守规范。"),
        Skill(repo: "obra/superpowers", path: "skills/verification-before-completion", title: "Verification Before Completion", publisher: "Superpowers", blurb: "说完成之前先运行验证，拿出证据。"),
        Skill(repo: "mattpocock/skills", path: "skills/productivity/grilling", title: "Grilling", publisher: "Matt Pocock", blurb: "就一个计划、决定或想法反复追问：每轮问一批能问的问题并给出建议答案，直到想清楚。"),
        Skill(repo: "mattpocock/skills", path: "skills/engineering/improve-codebase-architecture", title: "Improve Codebase Architecture", publisher: "Matt Pocock", blurb: "找出代码库里值得加深的模块，改善架构。"),
        Skill(repo: "vercel-labs/agent-skills", path: "skills/react-best-practices", title: "React Best Practices", publisher: "Vercel", blurb: "Vercel 总结的 React 和 Next.js 性能最佳实践。"),
        Skill(repo: "vercel-labs/agent-skills", path: "skills/web-design-guidelines", title: "Web Design Guidelines", publisher: "Vercel", blurb: "按 Web 界面规范审查 UI 代码。"),
    ]

    // MARK: Servers

    /// In the order the shelf shows them (reviewed 2026-11-26): documentation and search that
    /// need nothing first, then GitHub, the browser's insides, models, and services that need a key.
    static let servers: [Server] = [
        Server(name: "context7", title: "Context7", publisher: "Upstash", blurb: "任意库最新的文档和代码示例，直接交给 Agent。",
               homepage: "https://context7.com", transport: .http, url: "https://mcp.context7.com/mcp",
               inputs: [Input(key: "CONTEXT7_API_KEY", label: "API key", hint: "可选，提高调用上限；在 context7.com/dashboard 获取", required: false)]),
        Server(name: "deepwiki", title: "DeepWiki", publisher: "Cognition", blurb: "就任意公开的 GitHub 仓库提问，阅读它生成的 wiki。",
               homepage: "https://deepwiki.com", transport: .http, url: "https://mcp.deepwiki.com/mcp"),
        Server(name: "exa", title: "Exa", publisher: "Exa", blurb: "为 Agent 做的网页搜索和代码搜索，返回干净的页面内容。",
               homepage: "https://docs.exa.ai/reference/exa-mcp", transport: .http, url: "https://mcp.exa.ai/mcp"),
        Server(name: "github", title: "GitHub", publisher: "GitHub", blurb: "GitHub 上的仓库、Issue、Pull Request、Actions 和代码搜索。",
               homepage: "https://github.com/github/github-mcp-server", transport: .http, url: "https://api.githubcopilot.com/mcp/",
               inputs: [Input(key: "Authorization", label: "Personal access token", hint: "在 github.com/settings/personal-access-tokens 创建", format: "Bearer {}")]),
        Server(name: "chrome-devtools", title: "Chrome DevTools", publisher: "Google", blurb: "控制和检查一个真实的 Chrome：控制台、网络、性能追踪和截图。",
               homepage: "https://github.com/ChromeDevTools/chrome-devtools-mcp", transport: .stdio, command: ["npx", "-y", "chrome-devtools-mcp@latest"], process: .perSession),
        Server(name: "huggingface", title: "Hugging Face", publisher: "Hugging Face", blurb: "搜索 Hugging Face 上的模型、数据集、Spaces 和论文。",
               homepage: "https://huggingface.co/settings/mcp", transport: .http, url: "https://huggingface.co/mcp"),
        Server(name: "firecrawl", title: "Firecrawl", publisher: "Firecrawl", blurb: "抓取、爬取和搜索网页，转成干净的 Markdown。",
               homepage: "https://github.com/firecrawl/firecrawl-mcp-server", transport: .stdio, command: ["npx", "-y", "firecrawl-mcp"],
               inputs: [Input(key: "FIRECRAWL_API_KEY", label: "API key", hint: "在 firecrawl.dev/app/api-keys 获取")]),
        Server(name: "tavily", title: "Tavily", publisher: "Tavily", blurb: "用 Tavily 搜索网页并提取内容。",
               homepage: "https://github.com/tavily-ai/tavily-mcp", transport: .stdio, command: ["npx", "-y", "tavily-mcp"],
               inputs: [Input(key: "TAVILY_API_KEY", label: "API key", hint: "在 app.tavily.com 获取")]),
    ]
}
