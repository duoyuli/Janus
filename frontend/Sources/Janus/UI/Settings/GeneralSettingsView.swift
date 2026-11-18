import SwiftUI

/// Settings › 通用: language and appearance, then the update and macOS permission sections. Other places
/// (the update badge, "Check for Updates…") open this pane at a section through `AppStore.generalSection`.
// @project-doc docs/architecture/20261031-frontend.md#screens
struct GeneralSettingsView: View {
    let store: AppStore
    @Local private var appearance = Appearance.saved

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
                settingLabel(L.text("语言"), detail: L.text("选择 Janus 的界面语言，更改后立即生效。"))
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
                settingLabel(L.text("外观"), detail: L.text("选择 Janus 的界面外观，更改后立即生效。"))
                Spacer(minLength: 0)
                appearanceControl
            }
            Divider()
            sectionTitle(L.text("更新"))
                .id(GeneralSection.updates)
            UpdatesView()
            Divider()
            sectionTitle(L.text("macOS 权限"))
                .id(GeneralSection.permissions)
            MacPermissionsView(permissions: store.permissions)
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

    private var appearanceControl: some View {
        HStack(spacing: 2) {
            ForEach(Appearance.allCases) { item in
                let selected = item == appearance
                Button {
                    appearance = item
                    item.apply()
                } label: {
                    Text(item.title)
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
        .accessibilityLabel(L.text("外观"))
    }
}
