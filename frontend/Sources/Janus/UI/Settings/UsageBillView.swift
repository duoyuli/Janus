import AppKit
import SwiftUI

/// The usage bill over a period, across every run: Agents and their models, the token total and its
/// parts, active days and the cache hit rate, from `GET /api/usage`. The last period picked comes
/// back next time.
// @project-doc docs/architecture/frontend.md#usage_bill
struct UsageBillView: View {
    let store: AppStore
    @AppStorage("janus.usagePeriod") private var period: UsagePeriod = .days30
    @Local private var bill: UsageBill?
    @Local private var failure: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("按时间段汇总所有 Run 里 Agent 上报的 token：每一轮都计入，按 Agent 和模型分开。费用只列 Agent 自己上报的金额。")
                    .font(.callout).foregroundStyle(.secondary)
                HStack {
                    Picker("区间", selection: $period) {
                        ForEach(UsagePeriod.allCases) { Text($0.title).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .labelsHidden()
                    .frame(maxWidth: 360)
                    Spacer()
                    if let bill {
                        Button("复制账单") {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(bill.text, forType: .string)
                            store.show("已复制用量账单")
                        }
                    }
                }
                if let failure {
                    Note(text: "读取用量失败：\(failure)", tone: .red)
                } else if let bill {
                    Card(padding: 22) { UsageReceipt(bill: bill) }
                        .frame(maxWidth: 520)
                        .frame(maxWidth: .infinity)
                } else {
                    ProgressView().frame(maxWidth: .infinity)
                }
            }
            .padding(20)
        }
        .task(id: period) { await load() }
    }

    private func load() async {
        let now = Date()
        let (from, to) = period.range(now: now)
        let offset = TimeZone.current.secondsFromGMT(for: now) / 60
        do {
            let j = try await store.api.get("/api/usage?from=\(Int(from.timeIntervalSince1970 * 1000))&to=\(Int(to.timeIntervalSince1970 * 1000))&offset=\(offset)")
            bill = UsageBill.period(j, issued: now)
            failure = nil
        } catch {
            failure = error.localizedDescription
        }
    }
}
