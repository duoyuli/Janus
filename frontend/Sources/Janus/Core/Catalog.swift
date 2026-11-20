import Foundation

/// The library's picks: skills and MCP servers from the community worth having, added in one
/// click. Hand-picked and checked the way magpie picks its featured servers (each skill's folder
/// is in its repository, each server's package is published or its endpoint answers, none needs
/// an OAuth sign-in, which Janus can't do for a server). The list ships with the app, so showing
/// it reaches no network; adding a skill imports it from its GitHub repository as a pasted link
/// would, and adding a server writes a new MCP server like the editor does.
// @project-doc docs/domains/20261116-resource_library.md#catalog
enum Catalog {
    /// A skill in a GitHub repository: the folder holding its SKILL.md.
    struct Skill: Identifiable, Equatable {
        let repo: String
        let path: String
        let title: String
        let publisher: String
        /// What it is for, in Chinese; `L.catalogText` gives the app's language.
        let blurb: String
        var id: String { "\(repo)/\(path)" }
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
        let name = (s.path as NSString).lastPathComponent
        return items.first { i in
            guard i["kind"].str == "skill" else { return false }
            if i["name"].str == name { return true }
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

    static let skills: [Skill] = [
        Skill(repo: "anthropics/skills", path: "skills/pdf", title: "PDF", publisher: "Anthropic", blurb: "读取、合并、拆分和填写 PDF，提取其中的文字和表格。"),
        Skill(repo: "anthropics/skills", path: "skills/docx", title: "Word", publisher: "Anthropic", blurb: "创建和编辑 Word 文档，包括修订和批注。"),
        Skill(repo: "anthropics/skills", path: "skills/xlsx", title: "Excel", publisher: "Anthropic", blurb: "读写 Excel 表格，处理公式、格式和图表。"),
        Skill(repo: "anthropics/skills", path: "skills/pptx", title: "PowerPoint", publisher: "Anthropic", blurb: "创建和编辑 PowerPoint 演示文稿。"),
        Skill(repo: "anthropics/skills", path: "skills/frontend-design", title: "Frontend Design", publisher: "Anthropic", blurb: "做出有设计感、不像模板的前端界面。"),
        Skill(repo: "anthropics/skills", path: "skills/webapp-testing", title: "Web App Testing", publisher: "Anthropic", blurb: "用 Playwright 测试本地 Web 应用，截图、查看浏览器日志。"),
        Skill(repo: "anthropics/skills", path: "skills/mcp-builder", title: "MCP Builder", publisher: "Anthropic", blurb: "按最佳实践编写高质量的 MCP 服务器。"),
        Skill(repo: "anthropics/skills", path: "skills/skill-creator", title: "Skill Creator", publisher: "Anthropic", blurb: "创建、改进和评测 Skill。"),
        Skill(repo: "anthropics/skills", path: "skills/doc-coauthoring", title: "Doc Co-authoring", publisher: "Anthropic", blurb: "按结构化的流程和用户一起写文档、方案和规格。"),
        Skill(repo: "obra/superpowers", path: "skills/brainstorming", title: "Brainstorming", publisher: "Superpowers", blurb: "动手之前，先和用户把需求和设计想清楚。"),
        Skill(repo: "obra/superpowers", path: "skills/systematic-debugging", title: "Systematic Debugging", publisher: "Superpowers", blurb: "按找根因的步骤排查 bug，不靠猜。"),
        Skill(repo: "obra/superpowers", path: "skills/test-driven-development", title: "Test-Driven Development", publisher: "Superpowers", blurb: "先写一个失败的测试，再写让它通过的实现。"),
        Skill(repo: "obra/superpowers", path: "skills/writing-plans", title: "Writing Plans", publisher: "Superpowers", blurb: "把需求拆成一步步可以执行的实现计划。"),
        Skill(repo: "obra/superpowers", path: "skills/verification-before-completion", title: "Verification Before Completion", publisher: "Superpowers", blurb: "说完成之前先运行验证，拿出证据。"),
        Skill(repo: "mattpocock/skills", path: "skills/productivity/grill-me", title: "Grill Me", publisher: "Matt Pocock", blurb: "就一个计划或设计反复追问，直到每个分支都想清楚。"),
        Skill(repo: "mattpocock/skills", path: "skills/engineering/improve-codebase-architecture", title: "Improve Codebase Architecture", publisher: "Matt Pocock", blurb: "找出代码库里值得加深的模块，改善架构。"),
        Skill(repo: "vercel-labs/agent-skills", path: "skills/react-best-practices", title: "React Best Practices", publisher: "Vercel", blurb: "Vercel 总结的 React 和 Next.js 性能最佳实践。"),
        Skill(repo: "vercel-labs/agent-skills", path: "skills/web-design-guidelines", title: "Web Design Guidelines", publisher: "Vercel", blurb: "按 Web 界面规范审查 UI 代码。"),
    ]

    // MARK: Servers

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
        Server(name: "playwright", title: "Playwright", publisher: "Microsoft", blurb: "驱动真实的浏览器：打开页面、点击、输入，通过无障碍树读取页面。",
               homepage: "https://github.com/microsoft/playwright-mcp", transport: .stdio, command: ["npx", "-y", "@playwright/mcp@latest"], process: .perSession),
        Server(name: "chrome-devtools", title: "Chrome DevTools", publisher: "Google", blurb: "控制和检查一个真实的 Chrome：控制台、网络、性能追踪和截图。",
               homepage: "https://github.com/ChromeDevTools/chrome-devtools-mcp", transport: .stdio, command: ["npx", "-y", "chrome-devtools-mcp@latest"], process: .perSession),
        Server(name: "fetch", title: "Fetch", publisher: "Model Context Protocol", blurb: "抓取网页，转成 Markdown 交给 Agent。",
               homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/fetch", transport: .stdio, command: ["uvx", "mcp-server-fetch"]),
        Server(name: "memory", title: "Memory", publisher: "Model Context Protocol", blurb: "跨对话保留的知识图谱记忆。",
               homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/memory", transport: .stdio, command: ["npx", "-y", "@modelcontextprotocol/server-memory"]),
        Server(name: "sequential-thinking", title: "Sequential Thinking", publisher: "Model Context Protocol", blurb: "一步一步把问题想清楚，可以修正和分支。",
               homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/sequentialthinking", transport: .stdio, command: ["npx", "-y", "@modelcontextprotocol/server-sequential-thinking"]),
        Server(name: "time", title: "Time", publisher: "Model Context Protocol", blurb: "任意时区的当前时间和时区换算。",
               homepage: "https://github.com/modelcontextprotocol/servers/tree/main/src/time", transport: .stdio, command: ["uvx", "mcp-server-time"]),
        Server(name: "microsoft-learn", title: "Microsoft Learn", publisher: "Microsoft", blurb: "微软和 Azure 的官方文档与代码示例。",
               homepage: "https://learn.microsoft.com/api/mcp", transport: .http, url: "https://learn.microsoft.com/api/mcp"),
        Server(name: "cloudflare-docs", title: "Cloudflare Docs", publisher: "Cloudflare", blurb: "搜索 Cloudflare 的文档。",
               homepage: "https://github.com/cloudflare/mcp-server-cloudflare", transport: .http, url: "https://docs.mcp.cloudflare.com/mcp"),
        Server(name: "huggingface", title: "Hugging Face", publisher: "Hugging Face", blurb: "搜索 Hugging Face 上的模型、数据集、Spaces 和论文。",
               homepage: "https://huggingface.co/settings/mcp", transport: .http, url: "https://huggingface.co/mcp"),
        Server(name: "brave-search", title: "Brave Search", publisher: "Brave", blurb: "Brave 独立索引的网页、新闻、图片和本地搜索。",
               homepage: "https://github.com/brave/brave-search-mcp-server", transport: .stdio, command: ["npx", "-y", "@brave/brave-search-mcp-server"],
               inputs: [Input(key: "BRAVE_API_KEY", label: "API key", hint: "在 brave.com/search/api 获取")]),
        Server(name: "firecrawl", title: "Firecrawl", publisher: "Firecrawl", blurb: "抓取、爬取和搜索网页，转成干净的 Markdown。",
               homepage: "https://github.com/firecrawl/firecrawl-mcp-server", transport: .stdio, command: ["npx", "-y", "firecrawl-mcp"],
               inputs: [Input(key: "FIRECRAWL_API_KEY", label: "API key", hint: "在 firecrawl.dev/app/api-keys 获取")]),
        Server(name: "tavily", title: "Tavily", publisher: "Tavily", blurb: "用 Tavily 搜索网页并提取内容。",
               homepage: "https://github.com/tavily-ai/tavily-mcp", transport: .stdio, command: ["npx", "-y", "tavily-mcp"],
               inputs: [Input(key: "TAVILY_API_KEY", label: "API key", hint: "在 app.tavily.com 获取")]),
    ]
}
