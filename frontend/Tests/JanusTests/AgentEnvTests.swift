import Testing
@testable import Janus

/// "Insert Template" on Claude's custom API brings in only the variables the text does not set
/// yet, after what is there, so a pasted token or a kept private name is never replaced
/// (docs/domains/20261031-agents_and_roles.md#providers).
@Test func insertingTheTemplateKeepsWhatIsThere() {
    let template = "export ANTHROPIC_BASE_URL=https://xxx.xxx.com\nexport ANTHROPIC_AUTH_TOKEN=sk-xxx\nexport ANTHROPIC_MODEL=m"
    #expect(Setup.withTemplate("", template) == template)
    #expect(Setup.withTemplate("  \n", template) == template)
    let typed = "# my gateway\nANTHROPIC_AUTH_TOKEN\nexport ANTHROPIC_MODEL=mine\n"
    #expect(Setup.withTemplate(typed, template)
        == "# my gateway\nANTHROPIC_AUTH_TOKEN\nexport ANTHROPIC_MODEL=mine\nexport ANTHROPIC_BASE_URL=https://xxx.xxx.com")
    #expect(Setup.withTemplate(Setup.withTemplate(typed, template), template) == Setup.withTemplate(typed, template), "twice adds nothing more")
}

@Test func anEnvironmentLineNamesItsVariable() {
    #expect(Setup.envName("export  KEY=value") == "KEY")
    #expect(Setup.envName("KEY = value") == "KEY")
    #expect(Setup.envName("KEY") == "KEY")
    #expect(Setup.envName("# KEY=value") == nil)
    #expect(Setup.envName("   ") == nil)
}
