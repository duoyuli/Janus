import SwiftUI

struct GeneralSettingsView: View {
    @Local private var appearance = Appearance.saved

    var body: some View {
        ScrollView {
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
                }
                Divider()
                HStack(spacing: 22) {
                    settingLabel(L.text("外观"), detail: L.text("选择 Janus 的界面外观，更改后立即生效。"))
                    Spacer(minLength: 0)
                    appearanceControl
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .padding(28)
            .frame(maxWidth: .infinity, alignment: .topLeading)
        }
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
