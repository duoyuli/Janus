import Foundation

/// What 招聘顾问 proposes in a chat: the sheet the person creates the digital employees from.
extension AppStore {
    /// The sheet for a chat's proposal: the proposal as the server gave it, which employees are
    /// ticked (by number) and the agent they will run on.
    struct ProposalSheet {
        var chat: String
        var proposal: Proposal
        var connection: String
        var picked: Set<String> = []
        var error: String?
        var roles: [ProposedRole] { proposal.roles }
    }

    /// Open the sheet on the digital employees chat `run` proposed, as the server has them now;
    /// those whose name is taken start unticked.
    // @project-doc docs/domains/20261120-chats.md#proposed_employees
    func openProposal(_ run: RunModel) async {
        await run.loadProposal()
        guard let p = run.proposal else { return }
        proposalSheet = ProposalSheet(chat: run.id, proposal: p, connection: okConnections.first?["id"].str ?? "",
                                      picked: Set(p.roles.filter { !$0.existing }.map(\.n)))
    }

    /// Create the ticked employees of the open proposal; closes the sheet and says so when it
    /// worked. They stay out of the way: the person is in the chat.
    func createProposed() async {
        guard let sheet = proposalSheet else { return }
        let picks = JSON.array(sheet.roles.map(\.n).filter { sheet.picked.contains($0) }.map { JSON($0) })
        do {
            let made = try await api.post("/api/runs/\(API.q(sheet.chat))/proposal", ["proposal": sheet.proposal.id, "picks": picks, "connection": JSON(sheet.connection)])
            await loadSetup()
            proposalSheet = nil
            show(L.text("已创建 \(made.array.count) 位数字员工，在「设置 › 数字员工」里可以改"))
        } catch { proposalSheet?.error = error.localizedDescription }
    }

    /// Create none of what chat `run` proposed.
    func declineProposal(_ run: RunModel) async {
        guard let p = run.proposal else { return }
        do {
            _ = try await api.post("/api/runs/\(API.q(run.id))/proposal/decline", ["proposal": p.id])
            await run.loadProposal()
        } catch { show(error.localizedDescription) }
    }

    /// The default digital employee who designs teams in a chat (`backend/roles/recruiter.md`).
    static let recruiter = "recruiter"

    /// Talk with 招聘顾问 about the team to build; it proposes the digital employees it designs,
    /// which the chat shows for the person to create. Without an agent it opens the employee to
    /// pick one.
    func chatWithRecruiter() async {
        guard let r = role(Self.recruiter) else { return }
        if let why = r.unavailable {
            show(why)
            editRole(r.json)
            return
        }
        closeRoleEditor()
        showingSettings = false
        chatRole = r.id
        await createChat(role: r.id)
    }

    /// Whether adding a digital employee by hand (`draft`, the editor's) offers 招聘顾问 instead:
    /// only for a new one, while 招聘顾问 is still around.
    func offersRecruiter(_ draft: JSON) -> Bool {
        !draft["id"].truthy && role(Self.recruiter) != nil
    }
}
