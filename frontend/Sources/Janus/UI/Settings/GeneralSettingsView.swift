import SwiftUI

/// Settings › 通用: language, appearance and the send key, then the update section and 权限: the macOS
/// permissions Janus needs, and what of the person's home folder agents may use (本机环境). Other places
/// (the update badge, "Check for Updates…") open this pane at a section through `AppStore.generalSection`.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct GeneralSettingsView: View {
    let store: AppStore
    @Local private var appearance = Appearance.saved
    private var sendKey: SendKey { SendKeySetting.shared.value }

    var body: some View {
        ScrollViewReader { proxy in
            ScrollView {
                content
            }
            .onAppear { scroll(proxy) }
            .onChange(of: store.generalSection) { scroll(proxy) }
        }
    }

    private func scroll(_ proxy: ScrollViewProxy) {
        guard let section = store.generalSection else { return }
        store.generalSection = nil
        // On first appearance the sections are laid out after this call; scroll on the next turn.
        DispatchQueue.main.async { withAnimation { proxy.scrollTo(section, anchor: .top) } }
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 22) {
            Text(L.text("通用"))
                .font(.display(.title))
                .accessibilityAddTraits(.isHeader)
                .padding(.bottom, 6)
            HStack(spacing: 22) {
                settingLabel(L.text("语言"), detail: LanguageSettings.shared.selection == LanguageSettings.shared.atLaunch
                             ? L.text("Janus 的界面语言") : L.text("Janus 的界面语言。打开文件夹等系统对话框在重新打开 Janus 后换成这个语言"))
                Spacer(minLength: 0)
                Picker(L.text("语言"), selection: Bindable(LanguageSettings.shared).selection) {
                    ForEach(AppLanguage.allCases, id: \.self) { item in
                        Text(verbatim: item.title).tag(item)
                    }
                }
                .labelsHidden()
                .pickerStyle(.menu)
                .controlSize(.large)
                .frame(width: 166)
                // The menu picker is an AppKit pop-up button that keeps the bezel it was drawn with when
                // `NSApp.appearance` changes; rebuilding it makes it follow the new appearance.
                .id(appearance)
            }
            Divider()
            HStack(spacing: 22) {
                settingLabel(L.text("外观"), detail: L.text("Janus 的界面外观"))
                Spacer(minLength: 0)
                choice(Appearance.allCases, selected: appearance, label: L.text("外观"), title: \.title) {
                    appearance = $0
                    $0.apply()
                }
            }
            Divider()
            HStack(spacing: 22) {
                settingLabel(L.text("发送消息"), detail: L.text("输入框里发送消息的按键。\(sendKey.note)"))
                Spacer(minLength: 0)
                choice(SendKey.allCases, selected: sendKey, label: L.text("发送消息"), title: \.title) { SendKeySetting.shared.value = $0 }
            }
            Divider()
            sectionTitle(L.text("更新"))
                .id(GeneralSection.updates)
            UpdatesView()
            Divider()
            sectionTitle(L.text("权限"))
                .id(GeneralSection.permissions)
            Text(L.text("macOS 权限")).font(.headline)
            MacPermissionsView(permissions: store.permissions)
            Text(L.text("本机环境")).font(.headline).padding(.top, 10)
                .id(GeneralSection.hostEnv)
            HostEnvView(store: store)
        }
        .frame(maxWidth: 760, alignment: .leading)
        .padding(28)
        .frame(maxWidth: .infinity, alignment: .topLeading)
    }

    private func sectionTitle(_ title: String) -> some View {
        Text(title)
            .font(.display(.title2))
            .accessibilityAddTraits(.isHeader)
            .padding(.top, 6)
    }

    private func settingLabel(_ title: String, detail: String) -> some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(title).font(.headline)
            Text(detail).font(.callout).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    /// A row of choices, one selected: the appearance, the send key.
    private func choice<T: Identifiable & Equatable>(
        _ items: [T], selected current: T, label: String, title: @escaping (T) -> String, pick: @escaping (T) -> Void
    ) -> some View {
        HStack(spacing: 2) {
            ForEach(items) { item in
                let selected = item == current
                Button {
                    pick(item)
                } label: {
                    Text(title(item))
                        .font(.body.weight(selected ? .semibold : .regular))
                        .foregroundStyle(selected ? Color.primary : .secondary)
                        .frame(minWidth: 56, minHeight: 28)
                        .padding(.horizontal, 4)
                        .background(selected ? Color.card : .clear, in: RoundedRectangle(cornerRadius: 6))
                        .overlay {
                            if selected {
                                RoundedRectangle(cornerRadius: 6).strokeBorder(Color.primary.opacity(0.15))
                            }
                        }
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .hoverable(6)
                .accessibilityAddTraits(selected ? .isSelected : [])
            }
        }
        .padding(3)
        .background(Color.soft, in: RoundedRectangle(cornerRadius: 9))
        .overlay { RoundedRectangle(cornerRadius: 9).strokeBorder(Color.primary.opacity(0.12)) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(label)
    }
}
