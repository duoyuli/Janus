import Testing
@testable import Janus

/// The credential editor reads its status once from the library entry, then follows saves
/// locally. Clearing a value must not fall back to the stale `envSet` / `headerSet` it started
/// with.
@Test func clearedCredentialShowsUnsetImmediately() {
    let staleEnvSet = ["TOKEN", "ROOT"]
    let staleHeaderSet = ["Authorization"]
    var state = McpSecretState(envSet: staleEnvSet, headerSet: staleHeaderSet)
    #expect(state.isSet("env", "TOKEN"))
    #expect(state.isSet("env", "ROOT"))
    #expect(state.isSet("headers", "Authorization"))

    state.markCleared([McpSecretState.slot("env", "TOKEN")])
    #expect(!state.isSet("env", "TOKEN"), "cleared value is unset while the old server value still lists it")
    #expect(state.isSet("env", "ROOT"))
    #expect(state.isSet("headers", "Authorization"), "the same name in the other field is untouched")
}

@Test func savedCredentialShowsSetWithoutAnotherLibraryRead() {
    var state = McpSecretState()
    #expect(!state.isSet("env", "TOKEN"))

    state.markSaved([McpSecretState.slot("env", "TOKEN")])
    #expect(state.isSet("env", "TOKEN"))
}

/// A save replies with the resource as it is now; rebuilding from that reply is authoritative.
@Test func replyRebuildDoesNotResurrectClearedCredential() {
    var state = McpSecretState(envSet: ["TOKEN", "ROOT"], headerSet: ["Authorization"])
    state.markCleared([McpSecretState.slot("env", "TOKEN")])
    #expect(!state.isSet("env", "TOKEN"))

    state = McpSecretState(envSet: ["ROOT"], headerSet: ["Authorization"])
    #expect(!state.isSet("env", "TOKEN"))
    #expect(state.isSet("env", "ROOT"))
}
