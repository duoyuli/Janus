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
    var relaunches = 0
    var saved: MacPermissions.Record?

    nonisolated static var installation: MacPermissions.Installation {
        .init(bundleID: "local.janus.test", requirement: #"identifier "local.janus.test" and certificate leaf = H"aa""#)
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
        }, revealApp: {}, relaunch: { [self] in relaunches += 1 })) { [self] record in
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

/// An update signed by the same certificate is the same app to macOS: no new review, and the
/// person's confirmations stand. A new signing identity or bundle id starts over.
@Test @MainActor func onlyANewSigningIdentityInvalidatesManualPermissionConfirmations() {
    let system = PermissionSystem()
    let old = MacPermissions.Record(installation: PermissionSystem.installation, reminded: true, confirmed: [.fullDisk, .appManagement])
    let update = system.make(record: old)
    update.prepareForLaunch()
    #expect(!update.reviewPresented && update.status(.fullDisk) == .manuallyConfirmed)
    let adhoc = MacPermissions.Installation(bundleID: "local.janus.test", requirement: #"cdhash H"bb""#)
    let renamed = MacPermissions.Installation(bundleID: "io.github.janus.test", requirement: PermissionSystem.installation.requirement)
    for next in [adhoc, renamed] {
        let changed = system.make(record: old, installation: next)
        changed.prepareForLaunch()
        #expect(changed.reviewPresented)
        #expect(changed.status(.fullDisk) == .needsConfirmation && changed.status(.appManagement) == .needsConfirmation)
    }
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
    #expect(p.error?.contains(expectedCopy("无法打开系统设置", "Could not open System Settings")) == true)
    #expect(p.status(.appManagement) == .needsConfirmation)
    system.settingsOpen = true
    p.openSettings(.appManagement)
    #expect(p.error == nil && p.status(.appManagement) == .needsConfirmation)
    system.saveFails = true
    p.confirmCurrent()
    #expect(p.status(.appManagement) == .manuallyConfirmed && p.flow == .complete)
    #expect(p.error?.contains(expectedCopy("无法保存", "Could not save")) == true)
    #expect(system.saved == nil)
}

/// Granted in System Settings but not seen: Janus can only offer to reopen; a source run cannot.
@Test @MainActor func relaunchingIsOfferedOnlyForTheInstalledApp() {
    let system = PermissionSystem()
    system.make().relaunch()
    #expect(system.relaunches == 1)
    system.make(installation: nil).relaunch()
    #expect(system.relaunches == 1, "a source run has no app to reopen")
}
