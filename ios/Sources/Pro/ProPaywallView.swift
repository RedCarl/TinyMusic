import SwiftUI

private func proText(_ zh: String, _ en: String) -> String {
    Locale.preferredLanguages.first?.hasPrefix("zh") == true ? zh : en
}

struct ProPaywallView: View {
    let store: ProStore
    var allowClose: Bool = false

    @Environment(\.dismiss) private var dismiss
    @Environment(\.openURL) private var openURL

    private var isLocked: Bool { store.isLocked }

    var body: some View {
        ZStack {
            background
            ScrollView {
                VStack(spacing: 22) {
                    icon
                    headline
                    featureList
                    priceCard
                    actions
                    legal
                }
                .padding(.horizontal, 24)
                .padding(.top, allowClose ? 44 : 30)
                .padding(.bottom, 28)
                .frame(maxWidth: 480)
                .frame(maxWidth: .infinity)
            }
            .scrollIndicators(.hidden)
        }
        .overlay(alignment: .topTrailing) {
            if allowClose {
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 13, weight: .bold))
                        .frame(width: 30, height: 30)
                        .background(.thinMaterial, in: Circle())
                }
                .buttonStyle(.plain)
                .padding(14)
                .accessibilityLabel(proText("关闭", "Close"))
            }
        }
        .interactiveDismissDisabled(!allowClose)
        .alert(proText("提示", "Notice"), isPresented: Binding(
            get: { store.message != nil },
            set: { if !$0 { store.dismissMessage() } }
        )) {
            Button(proText("好", "OK"), role: .cancel) { store.dismissMessage() }
        } message: {
            Text(store.message ?? "")
        }
    }

    private var background: some View {
        LinearGradient(
            colors: [Color.accentColor.opacity(0.22), Color(.systemBackground), Color(.systemBackground)],
            startPoint: .top,
            endPoint: .center
        )
        .ignoresSafeArea()
    }

    private var icon: some View {
        ZStack {
            RoundedRectangle(cornerRadius: 24, style: .continuous)
                .fill(Color.accentColor.gradient)
                .frame(width: 82, height: 82)
                .shadow(color: Color.accentColor.opacity(0.35), radius: 18, y: 8)
            Image(systemName: "lock.open.fill")
                .font(.system(size: 34, weight: .semibold))
                .foregroundStyle(.white)
        }
        .padding(.top, 6)
    }

    private var headline: some View {
        VStack(spacing: 8) {
            Text(isLocked
                 ? proText("免费试用已结束", "Your free trial has ended")
                 : proText("解锁完整版", "Unlock the full version"))
                .font(.system(size: 28, weight: .bold, design: .rounded))
                .multilineTextAlignment(.center)
            Text(isLocked
                 ? proText("一次付费，永久使用全部功能。", "One purchase keeps every feature available.")
                 : proText("30 天免费试用，结束后 ¥8 一次付费。", "30-day free trial, then a one-time purchase."))
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
    }

    private var featureList: some View {
        VStack(spacing: 14) {
            ForEach(store.offer.features) { feature in
                HStack(alignment: .top, spacing: 12) {
                    Image(systemName: feature.symbol)
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(Color.accentColor)
                        .frame(width: 30, height: 30)
                        .background(Color.accentColor.opacity(0.12), in: RoundedRectangle(cornerRadius: 9, style: .continuous))
                    VStack(alignment: .leading, spacing: 2) {
                        Text(feature.title)
                            .font(.subheadline.weight(.semibold))
                        Text(feature.detail)
                            .font(.footnote)
                            .foregroundStyle(.secondary)
                    }
                    Spacer(minLength: 0)
                }
            }
        }
        .padding(16)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
    }

    private var priceCard: some View {
        HStack(alignment: .center, spacing: 10) {
            VStack(alignment: .leading, spacing: 3) {
                Text(store.displayPrice)
                    .font(.system(size: 26, weight: .bold, design: .rounded))
                Text(proText("一次付费 · 永久可用 · 无订阅", "One-time purchase · No subscription"))
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Text(proText("永久", "Lifetime"))
                .font(.caption.weight(.semibold))
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(Color.accentColor.opacity(0.14), in: Capsule())
                .foregroundStyle(Color.accentColor)
        }
        .padding(.horizontal, 18)
        .padding(.vertical, 14)
        .background(Color(.secondarySystemBackground), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
    }

    private var actions: some View {
        VStack(spacing: 10) {
            Button {
                Task { await store.purchase() }
            } label: {
                HStack(spacing: 8) {
                    if store.isBusy {
                        ProgressView().tint(.white)
                    }
                    Text(proText("解锁完整版", "Unlock full version"))
                        .font(.headline)
                }
                .frame(maxWidth: .infinity)
                .padding(.vertical, 4)
            }
            .buttonStyle(.borderedProminent)
            .controlSize(.large)
            .disabled(store.isBusy)
            .accessibilityIdentifier("pro.purchase")

            Button {
                Task { await store.restore() }
            } label: {
                Text(proText("恢复购买", "Restore Purchase"))
                    .font(.subheadline)
                    .frame(maxWidth: .infinity)
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
            .disabled(store.isBusy)
            .accessibilityIdentifier("pro.restore")
        }
    }

    private var legal: some View {
        VStack(spacing: 8) {
            HStack(spacing: 14) {
                Button(proText("隐私政策", "Privacy")) { openURL(store.offer.privacyURL) }
                Button(proText("支持与条款", "Terms & Support")) { openURL(store.offer.termsURL) }
            }
            .font(.caption)
            .foregroundStyle(.secondary)
            Text(proText("购买后与你的 Apple ID 绑定，可在同一账号的设备上恢复。", "Your purchase is tied to your Apple ID and can be restored on your devices."))
                .font(.caption2)
                .foregroundStyle(.tertiary)
                .multilineTextAlignment(.center)
        }
    }
}

struct ProSettingsRow: View {
    let store: ProStore
    @State private var showPaywall = false

    var body: some View {
        Button {
            if !store.isUnlocked { showPaywall = true }
        } label: {
            HStack(spacing: 10) {
                Image(systemName: store.isUnlocked ? "checkmark.seal.fill" : "sparkles")
                    .foregroundStyle(Color.accentColor)
                    .frame(width: 22)
                VStack(alignment: .leading, spacing: 2) {
                    Text(proText("完整版", "Full Version"))
                    Text(store.statusText)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                if !store.isUnlocked {
                    Image(systemName: "chevron.right")
                        .font(.footnote.weight(.semibold))
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .tint(.primary)
        .disabled(store.isUnlocked)
        .accessibilityIdentifier("pro.row")
        .sheet(isPresented: $showPaywall) {
            ProPaywallView(store: store, allowClose: true)
        }
    }
}