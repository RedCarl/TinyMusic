import SwiftUI

struct SettingsView: View {
    let model: AppModel
    let pro: ProStore

    @Environment(\.openURL) private var openURL

    @State private var showS3 = false
    @State private var testing = false
    @State private var alert: SettingsAlert?

    private enum SettingsAlert: Identifiable {
        case test(String)
        case iCloudUnavailable
        case openFailed(String)

        var id: String {
            switch self {
            case .test: return "test"
            case .iCloudUnavailable: return "icloud"
            case .openFailed: return "openFailed"
            }
        }
    }

    /// 连接正常：地址、桶和密钥齐全，且最近一次保存/测试没有报错。
    private var isConnected: Bool {
        model.settings.isConfigured && model.settings.error == nil
    }

    private var connectionColor: Color {
        if model.settings.error != nil { return .red }
        return model.settings.isConfigured ? .green : .orange
    }

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    ProSettingsRow(store: pro)
                }

                Section {
                    HStack(spacing: 12) {
                        ZStack(alignment: .topTrailing) {
                            Image(systemName: "externaldrive.connected.to.line.below")
                                .font(.title3)
                                .frame(width: 28, height: 28)
                            Circle()
                                .fill(connectionColor)
                                .frame(width: 8, height: 8)
                                .overlay(Circle().stroke(Color(.systemBackground), lineWidth: 1.5))
                                .offset(x: 3, y: -2)
                        }
                        Text(L("S3 网盘"))
                        Spacer()
                        Button {
                            showS3 = true
                        } label: {
                            Image(systemName: "pencil")
                                .font(.system(size: 15, weight: .semibold))
                                .frame(width: 34, height: 34)
                                .background(Circle().fill(Color(.tertiarySystemFill)))
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(L("编辑连接"))

                        Button {
                            Task { await runTest() }
                        } label: {
                            Group {
                                if testing {
                                    ProgressView()
                                        .controlSize(.small)
                                } else {
                                    Image(systemName: "arrow.clockwise")
                                        .font(.system(size: 15, weight: .semibold))
                                }
                            }
                            .frame(width: 34, height: 34)
                            .background(Circle().fill(Color(.tertiarySystemFill)))
                        }
                        .buttonStyle(.plain)
                        .disabled(!model.settings.isConfigured || testing)
                        .accessibilityLabel(L("测试连接"))

                        // 只有连接正常（绿点）时才给断开入口，避免没连接时也摆一个危险按钮。
                        if isConnected {
                            Button(role: .destructive) {
                                Task {
                                    await model.settings.disconnect()
                                    model.library.clear()
                                }
                            } label: {
                                Image(systemName: "externaldrive.badge.xmark")
                                    .font(.system(size: 15, weight: .semibold))
                                    .foregroundStyle(.red)
                                    .frame(width: 34, height: 34)
                                    .background(Circle().fill(Color(.tertiarySystemFill)))
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(L("断开 S3 连接"))
                            .transition(.scale.combined(with: .opacity))
                        }
                    }
                    .animation(.easeInOut(duration: 0.2), value: isConnected)
                }

                Section {
                    Toggle(L("iCloud 同步配置"), isOn: Binding(
                        get: { model.settings.iCloudEnabled },
                        set: { value in
                            guard AppConfiguration.hasICloudAccount else {
                                alert = .iCloudUnavailable
                                return
                            }
                            Task { await model.settings.setICloudEnabled(value) }
                        }
                    ))
                }

                Section {
                    // 语言：进独立选择页，选中后留在那里（不弹回设置页）。
                    NavigationLink {
                        LanguagePickerView()
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: "globe")
                                .foregroundStyle(Color.accentColor)
                            Text(L("语言"))
                                .foregroundStyle(.primary)
                            Spacer(minLength: 8)
                            Text(Localization.shared.setting.displayName)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .accessibilityIdentifier("settings.language")
                }

                Section {
                    NavigationLink {
                        EqualizerView(eq: model.player.eq)
                    } label: {
                        Label(L("均衡器"), systemImage: "slider.horizontal.3")
                    }
                }

                Section {
                    Button {
                        rateApp()
                    } label: {
                        supportLabel(L("去应用商店评分"), systemImage: "star")
                    }
                    .tint(Color.primary)
                    .accessibilityIdentifier("settings.rateApp")

                    ShareLink(
                        item: AppLinks.appStoreURL,
                        subject: Text(L("TinyMusic · S3 音乐播放器")),
                        message: Text(AppLinks.shareMessage)
                    ) {
                        supportLabel(L("分享应用"), systemImage: "square.and.arrow.up")
                    }
                    .tint(Color.primary)
                    .accessibilityIdentifier("settings.shareApp")

                    Button {
                        sendFeedback()
                    } label: {
                        supportLabel(L("意见反馈（邮件）"), systemImage: "envelope")
                    }
                    .tint(Color.primary)
                    .accessibilityIdentifier("settings.feedback")
                }

                Section {
                    HStack {
                        Text(L("版本"))
                        Spacer()
                        Text(versionText)
                            .foregroundStyle(.secondary)
                    }
                    Link(destination: URL(string: "https://redcarl.github.io/TinyMusic/tinymusic/privacy.html")!) {
                        Text(L("隐私政策"))
                    }
                }

                Section {
                    Color.clear
                        .frame(height: 76)
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets())
                }
            }
            .contentMargins(.bottom, 88, for: .scrollContent)
            .navigationTitle(L("设置"))
            .sheet(isPresented: $showS3) {
                S3SetupView(model: model, isOnboarding: false)
            }
            .alert(item: $alert) { item in
                switch item {
                case .test(let message):
                    return Alert(
                        title: Text(L("连接测试")),
                        message: Text(message),
                        dismissButton: .default(Text(L("好")))
                    )
                case .iCloudUnavailable:
                    return Alert(
                        title: Text(L("iCloud 不可用")),
                        message: Text(L("当前构建或 Apple ID 未启用 iCloud 同步。")),
                        dismissButton: .default(Text(L("好")))
                    )
                case .openFailed(let message):
                    return Alert(
                        title: Text(L("无法完成操作")),
                        message: Text(message),
                        dismissButton: .default(Text(L("好")))
                    )
                }
            }
        }
    }

    private var versionText: String {
        let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "1.0.0"
        let build = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String
        guard let build, !build.isEmpty else { return version }
        return "\(version) (\(build))"
    }

    /// Row label that keeps the text readable (a Button would otherwise tint it).
    private func supportLabel(_ title: String, systemImage: String) -> some View {
        Label {
            Text(title)
                .foregroundStyle(.primary)
        } icon: {
            Image(systemName: systemImage)
                .foregroundStyle(Color.accentColor)
        }
    }

    /// Opens the App Store app on the review form; Safari is the fallback when
    /// the store app cannot take the URL.
    private func rateApp() {
        openURL(AppLinks.writeReviewAppURL) { opened in
            guard !opened else { return }
            openURL(AppLinks.writeReviewURL) { fallbackOpened in
                guard !fallbackOpened else { return }
                alert = .openFailed(L("无法打开 App Store，请稍后再试。"))
            }
        }
    }

    private func sendFeedback() {
        guard let url = AppLinks.feedbackURL else {
            alert = .openFailed(L("无法生成反馈邮件。"))
            return
        }
        openURL(url) { opened in
            guard !opened else { return }
            alert = .openFailed(L("没有可用的邮件 App。请手动发送到 %@。", AppLinks.supportEmail))
        }
    }

    private func runTest() async {
        testing = true
        defer { testing = false }
        let result = await model.settings.testConnection()
        alert = .test(result ?? model.settings.error ?? L("连接失败"))
    }
}