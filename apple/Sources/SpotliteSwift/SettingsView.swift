import SwiftUI

/// Spotlite's settings that a front end can change: playback, the
/// equalizer, and Home's shelves.
struct SettingsView: View {
    @Environment(AppModel.self) private var model
    @State private var state: SettingsState?
    @State private var needsApply = false

    /// Settings Spotlite reads when its player starts.
    private let engine: Set<String> = ["bitrate", "normalisation", "gapless", "autoplay", "audio_cache", "device_name"]

    var body: some View {
        TabView {
            Tab("Playback", systemImage: "play.circle") { form(groups: ["Playback", "Home"]) }
            Tab("Equalizer", systemImage: "slider.vertical.3") { equalizer }
        }
        .frame(width: 520, height: 520)
        .task { await load() }
    }

    private func form(groups: [String]) -> some View {
        Form {
            ForEach(groups, id: \.self) { group in
                Section(group) {
                    ForEach(state?.settings.filter { $0.group == group } ?? []) { setting in
                        row(setting)
                    }
                }
            }
            if needsApply {
                Section {
                    HStack {
                        Text("Playback restarts to use these settings.")
                            .foregroundStyle(.secondary)
                        Spacer()
                        Button("Apply") {
                            model.send("settings.apply")
                            needsApply = false
                        }
                        .buttonStyle(.glassProminent)
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    @ViewBuilder
    private func row(_ setting: Setting) -> some View {
        switch (setting.kind, setting.value) {
        case ("bool", .bool(let on)):
            Toggle(isOn: Binding(get: { on }, set: { set(setting, .bool($0)) })) {
                Text(setting.label)
                if let detail = setting.detail { Text(detail) }
            }
        case ("choice", .text(let value)):
            Picker(selection: Binding(get: { value }, set: { set(setting, .text($0)) })) {
                ForEach(setting.choices ?? [], id: \.value) { Text($0.label).tag($0.value) }
            } label: {
                Text(setting.label)
                if let detail = setting.detail { Text(detail) }
            }
        case ("text", .text(let value)):
            TextSetting(setting: setting, value: value) { set(setting, .text($0)) }
        case ("number", .number(let value)):
            LabeledContent(setting.label) {
                Slider(
                    value: Binding(get: { value }, set: { set(setting, .number($0)) }),
                    in: (setting.min ?? 0)...(setting.max ?? 1),
                    step: setting.step ?? 0.1)
                Text(String(format: "%+.1f dB", value))
                    .monospacedDigit()
                    .frame(width: 64, alignment: .trailing)
            }
        default:
            EmptyView()
        }
    }

    private var equalizer: some View {
        let settings = state?.settings ?? []
        let on = settings.first { $0.key == "eq_on" }
        let preamp = settings.first { $0.key == "eq_preamp_db" }
        let bands = settings.filter { $0.key.hasPrefix("eq_band_") }
        return Form {
            if let on { row(on) }
            Section("Bands") {
                HStack(alignment: .bottom, spacing: 10) {
                    ForEach([preamp].compactMap { $0 } + bands) { band in
                        if case .number(let value) = band.value {
                            VStack(spacing: 6) {
                                Text(String(format: "%+.0f", value))
                                    .font(.caption2.monospacedDigit())
                                    .foregroundStyle(.secondary)
                                Slider(
                                    value: Binding(get: { value }, set: { set(band, .number($0)) }),
                                    in: (band.min ?? -12)...(band.max ?? 12),
                                    step: band.step ?? 0.5)
                                    .rotationEffect(.degrees(-90))
                                    .frame(width: 150, height: 24)
                                    .frame(width: 24, height: 150)
                                Text(band.key == "eq_preamp_db" ? "Pre" : band.label)
                                    .font(.caption2)
                                    .lineLimit(1)
                                    .fixedSize()
                            }
                        }
                    }
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 8)
            }
            if let presets = state?.eqPresets {
                Section("Presets") {
                    Picker("Preset", selection: Binding(get: { -1 }, set: { index in
                        model.send("settings.eq_preset", ["index": index])
                        Task {
                            try? await Task.sleep(for: .milliseconds(300))
                            await load()
                        }
                    })) {
                        Text("Choose…").tag(-1)
                        ForEach(Array(presets.enumerated()), id: \.offset) { index, name in
                            Text(name).tag(index)
                        }
                    }
                }
            }
        }
        .formStyle(.grouped)
    }

    private func set(_ setting: Setting, _ value: SettingValue) {
        if let index = state?.settings.firstIndex(where: { $0.key == setting.key }) {
            state?.settings[index].value = value
        }
        if engine.contains(setting.key) { needsApply = true }
        Task {
            if let next = try? await model.call(
                "settings.set", ["key": setting.key, "value": value.json], as: SettingsState.self)
            {
                // A slider's later moves may already be on their way.
                if case .number = value {} else { state = next }
            }
        }
    }

    private func load() async {
        state = try? await model.call("settings.get", as: SettingsState.self)
    }
}

/// A text setting, sent when the field is committed.
private struct TextSetting: View {
    var setting: Setting
    var value: String
    var commit: (String) -> Void
    @State private var text = ""

    var body: some View {
        TextField(text: $text) {
            Text(setting.label)
            if let detail = setting.detail { Text(detail) }
        }
        .onAppear { text = value }
        .onSubmit { commit(text) }
    }
}
