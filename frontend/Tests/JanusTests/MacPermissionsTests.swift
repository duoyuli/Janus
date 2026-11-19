import Foundation
import Testing
@testable import Janus

@MainActor private final class PermissionSystem {
    var allowed: [MacPermission: Bool?] = [.fullDisk: false, .appManagement: false, .screenRecording: false]
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

    func grant(_ permissions: MacPermission...) { for p in permissions { allowed[p] = true } }

    func make(record: MacPermissions.Record? = nil, installation: MacPermissions.Installation? = PermissionSystem.installation) -> MacPermissions {
        MacPermissions(installation: installation, record: record, system: .init(allowed: { [self] permission in
            checks += 1
            return allowed[permission] ?? nil
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
    #expect(system.checks == 3 * MacPermission.allCases.count, "every launch queries the live state of every permission")
}

/// The status is what macOS reports, at every check: a grant shows up, so does a revocation, and a
/// macOS that gives no answer leaves it unknown rather than granted.
@Test @MainActor func everyPermissionShowsTheDetectedSystemState() {
    let system = PermissionSystem()
    let p = system.make()
    p.refresh()
    #expect(MacPermission.allCases.allSatisfy { p.status($0) == .notGranted })
    system.grant(.fullDisk, .appManagement)
    system.allowed[.screenRecording] = .some(nil)
    p.refresh()
    #expect(p.status(.fullDisk) == .granted && p.status(.appManagement) == .granted)
    #expect(p.status(.screenRecording) == .unknown && p.outstanding == [.screenRecording])
    system.allowed[.fullDisk] = false
    p.refresh()
    #expect(p.status(.fullDisk) == .notGranted)
}

/// Nothing is shown when everything is already granted, and the same signing identity is the same
/// app to macOS: no new review. A new signing identity or bundle id starts over.
@Test @MainActor func onlyANewSigningIdentityWithMissingPermissionsShowsTheReview() {
    let system = PermissionSystem()
    system.grant(.fullDisk, .appManagement, .screenRecording)
    let granted = system.make()
    granted.prepareForLaunch()
    #expect(!granted.reviewPresented && system.saved?.reminded == true)
    system.allowed[.screenRecording] = false
    let old = MacPermissions.Record(installation: PermissionSystem.installation, reminded: true)
    let update = system.make(record: old)
    update.prepareForLaunch()
    #expect(!update.reviewPresented)
    let adhoc = MacPermissions.Installation(bundleID: "local.janus.test", requirement: #"cdhash H"bb""#)
    let renamed = MacPermissions.Installation(bundleID: "io.github.janus.test", requirement: PermissionSystem.installation.requirement)
    for next in [adhoc, renamed] {
        let changed = system.make(record: old, installation: next)
        changed.prepareForLaunch()
        #expect(changed.reviewPresented && changed.record?.installation == next)
    }
}

/// Opening System Settings grants nothing; the guide moves on only once the check sees the grant.
@Test @MainActor func permissionGuidanceIsSequentialAndAdvancesOnlyOnADetectedGrant() {
    let system = PermissionSystem()
    let p = system.make()
    p.beginAll()
    #expect(p.current == .fullDisk && system.opened == [.fullDisk])
    p.refresh()
    #expect(p.current == .fullDisk && p.status(.fullDisk) == .notGranted, "returning from Settings without granting")
    system.grant(.fullDisk)
    p.refresh()
    #expect(p.current == .appManagement && system.opened == [.fullDisk, .appManagement])
    p.skipCurrent()
    #expect(p.current == .screenRecording && system.requests == 1)
    #expect(p.status(.screenRecording) == .notGranted, "a request result is not a cached authorization")
    system.grant(.screenRecording)
    p.refresh()
    #expect(p.flow == .complete && p.outstanding == [.appManagement])
}

@Test @MainActor func skippedAndUnavailablePermissionsNeverBecomeGranted() {
    let system = PermissionSystem()
    let p = system.make()
    p.beginAll()
    p.skipCurrent()
    p.skipCurrent()
    p.skipCurrent()
    #expect(p.flow == .complete && p.outstanding == MacPermission.allCases)
    let sourceRun = system.make(installation: nil)
    let checks = system.checks, requests = system.requests, opened = system.opened
    sourceRun.prepareForLaunch()
    sourceRun.beginAll()
    sourceRun.begin(.screenRecording)
    sourceRun.openSettings(.fullDisk)
    #expect(!sourceRun.reviewPresented && sourceRun.flow == .idle)
    #expect(MacPermission.allCases.allSatisfy { sourceRun.status($0) == .unavailable })
    #expect(system.checks == checks && system.requests == requests && system.opened == opened)
}

@Test @MainActor func guidanceSkipsPermissionsGrantedWhileThePersonWasInSettings() {
    let system = PermissionSystem()
    let p = system.make()
    p.beginAll()
    system.grant(.fullDisk, .appManagement, .screenRecording)
    p.refresh()
    #expect(p.flow == .complete && system.requests == 0 && system.opened == [.fullDisk])
}

@Test @MainActor func permissionFailuresRemainVisibleAndNeverImplyAuthorization() {
    let system = PermissionSystem()
    system.settingsOpen = false
    let p = system.make()
    p.begin(.appManagement)
    #expect(p.error?.contains(expectedCopy("无法打开系统设置", "Could not open System Settings")) == true)
    #expect(p.status(.appManagement) == .notGranted)
    system.settingsOpen = true
    p.openSettings(.appManagement)
    #expect(p.error == nil && p.status(.appManagement) == .notGranted)
    system.saveFails = true
    let launch = system.make()
    launch.prepareForLaunch()
    #expect(launch.reviewPresented)
    #expect(launch.error?.contains(expectedCopy("无法保存", "Could not save")) == true)
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
