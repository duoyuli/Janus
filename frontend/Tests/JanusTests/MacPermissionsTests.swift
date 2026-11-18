import Foundation
import Testing
@testable import Janus

@MainActor private final class PermissionSystem {
    var allowed = false
    var requestResult = false
    var settingsOpen = true
    var saveFails = false
    var checks = 0
    var requests = 0
    var opened: [MacPermission] = []
    var saved: MacPermissions.Record?

    nonisolated static var installation: MacPermissions.Installation {
        .init(bundleID: "local.janus.test", path: "/Applications/Janus Test.app", version: "1", build: "10", codeHash: "first-code")
    }

    func make(record: MacPermissions.Record? = nil, installation: MacPermissions.Installation? = PermissionSystem.installation) -> MacPermissions {
        MacPermissions(installation: installation, record: record, system: .init(screenAllowed: { [self] in
            checks += 1
            return allowed
        }, requestScreen: { [self] in
            requests += 1
            return requestResult
        }, openSettings: { [self] permission in
            opened.append(permission)
            return settingsOpen
        }, revealApp: {})) { [self] record in
            if saveFails { throw CocoaError(.fileWriteNoPermission) }
            saved = record
        }
    }
}

@Test @MainActor func permissionsReviewRunsOncePerInstalledBuild() {
    let system = PermissionSystem()
    let first = system.make()
    first.prepareForLaunch()
    #expect(first.reviewPresented)
    #expect(system.saved?.reminded == true)
    #expect(system.requests == 0 && system.opened.isEmpty, "launch checks do not silently request permissions")
    first.dismissReview()
    first.prepareForLaunch()
    #expect(!first.reviewPresented)
    let restart = system.make(record: system.saved)
    restart.prepareForLaunch()
    #expect(!restart.reviewPresented)
    #expect(system.checks == 3, "even ordinary restarts query the live screen-recording state")
}

@Test @MainActor func anUpdateInvalidatesManualPermissionConfirmations() {
    let system = PermissionSystem()
    let old = MacPermissions.Record(installation: PermissionSystem.installation, reminded: true, confirmed: [.fullDisk, .appManagement])
    let unchanged = system.make(record: old)
    #expect(unchanged.status(.fullDisk) == .manuallyConfirmed)
    for field in [\MacPermissions.Installation.version, \.build, \.path] {
        var next = PermissionSystem.installation
        next[keyPath: field] += "-changed"
        let updated = system.make(record: old, installation: next)
        updated.prepareForLaunch()
        #expect(updated.reviewPresented)
        #expect(updated.status(.fullDisk) == .needsConfirmation)
        #expect(updated.status(.appManagement) == .needsConfirmation)
    }
    var rebuilt = PermissionSystem.installation
    rebuilt.codeHash = "another-binary-at-the-same-version"
    let updated = system.make(record: old, installation: rebuilt)
    updated.prepareForLaunch()
    #expect(updated.reviewPresented && updated.record?.confirmed.isEmpty == true)
}

@Test @MainActor func permissionGuidanceIsSequentialAndDenialIsNotAConfirmation() {
    let system = PermissionSystem()
    let p = system.make()
    p.beginAll()
    #expect(p.current == .fullDisk && system.opened == [.fullDisk])
    #expect(p.status(.fullDisk) == .needsConfirmation, "opening settings grants nothing")
    p.refresh()
    #expect(p.current == .fullDisk, "returning from Settings cannot confirm a permission we cannot query")
    p.confirmCurrent()
    #expect(p.status(.fullDisk) == .manuallyConfirmed)
    #expect(p.status(.fullDisk) != .granted)
    #expect(p.current == .appManagement && system.opened == [.fullDisk, .appManagement])
    p.confirmCurrent()
    #expect(p.current == .screenRecording && system.requests == 1)
    p.confirmCurrent()
    #expect(p.current == .screenRecording && p.status(.screenRecording) == .notGranted)
    #expect(p.record?.confirmed.contains(.screenRecording) == false)
    p.skipCurrent()
    #expect(p.flow == .complete && p.outstanding == [.screenRecording])
}

@Test @MainActor func screenRecordingUsesTheSystemCheckAndDetectsRevocation() {
    let system = PermissionSystem()
    system.requestResult = true
    let p = system.make()
    p.begin(.screenRecording)
    #expect(p.status(.screenRecording) == .notGranted, "a request result is not a cached authorization")
    system.allowed = true
    p.refresh()
    #expect(p.status(.screenRecording) == .granted && p.flow == .complete)
    system.allowed = false
    p.refresh()
    #expect(p.status(.screenRecording) == .notGranted)
}

@Test @MainActor func skippedAndUnavailablePermissionsNeverBecomeGranted() {
    let system = PermissionSystem()
    let p = system.make()
    p.beginAll()
    p.skipCurrent()
    p.skipCurrent()
    p.skipCurrent()
    #expect(p.outstanding == MacPermission.allCases)
    #expect(p.record?.confirmed.isEmpty == true)
    let sourceRun = system.make(installation: nil)
    let checks = system.checks, requests = system.requests, opened = system.opened
    sourceRun.prepareForLaunch()
    sourceRun.beginAll()
    sourceRun.begin(.screenRecording)
    sourceRun.openSettings(.fullDisk)
    sourceRun.confirmCurrent()
    #expect(!sourceRun.reviewPresented && sourceRun.flow == .idle)
    #expect(sourceRun.status(.screenRecording) == .unavailable)
    #expect(system.checks == checks && system.requests == requests && system.opened == opened)
}

@Test @MainActor func guidanceSkipsPermissionsGrantedWhileThePersonWasInSettings() {
    let system = PermissionSystem()
    let p = system.make()
    p.beginAll()
    system.allowed = true
    p.refresh()
    p.confirmCurrent()
    p.confirmCurrent()
    #expect(p.flow == .complete && system.requests == 0)
    #expect(p.status(.screenRecording) == .granted)
}

@Test @MainActor func permissionFailuresRemainVisibleAndNeverImplyAuthorization() {
    let system = PermissionSystem()
    system.settingsOpen = false
    let p = system.make()
    p.begin(.appManagement)
    #expect(p.error?.contains("无法打开系统设置") == true)
    #expect(p.status(.appManagement) == .needsConfirmation)
    system.settingsOpen = true
    p.openSettings(.appManagement)
    #expect(p.error == nil && p.status(.appManagement) == .needsConfirmation)
    system.saveFails = true
    p.confirmCurrent()
    #expect(p.status(.appManagement) == .manuallyConfirmed && p.flow == .complete)
    #expect(p.error?.contains("无法保存") == true)
    #expect(system.saved == nil)
}
