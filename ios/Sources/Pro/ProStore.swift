import Foundation
import Observation
import Security
import StoreKit
import SwiftUI

// MARK: - Offer

struct ProFeature: Identifiable {
    let id = UUID()
    let symbol: String
    let title: String
    let detail: String
}

struct ProOffer {
    let appName: String
    let productID: String
    let privacyURL: URL
    let termsURL: URL
    let features: [ProFeature]
    let legacyCutover: Date
    let trialDays: Int

    static let tinyMusic = ProOffer(
        appName: "TinyMusic",
        productID: "cn.carljoy.tinymusic.lifetime",
        privacyURL: URL(string: "https://redcarl.github.io/TinyMusic/tinymusic/privacy.html")!,
        termsURL: URL(string: "https://redcarl.github.io/TinyMusic/tinymusic/terms.html")!,
        features: [
            ProFeature(symbol: "externaldrive.connected.to.line.below", title: "连接任意 S3 存储", detail: "AWS S3、Cloudflare R2、MinIO 等"),
            ProFeature(symbol: "arrow.down.circle", title: "离线缓存与无损播放", detail: "整张歌单缓存，断网也能听"),
            ProFeature(symbol: "slider.horizontal.3", title: "10 段 EQ 与内嵌歌词", detail: "12 套预设、前级增益、封面与队列")
        ],
        legacyCutover: ISO8601DateFormatter().date(from: "2026-11-21T00:00:00Z")!,
        trialDays: 30
    )
}

// MARK: - Trial clock

private enum ProKeychain {
    static let service = "cn.carljoy.pro"

    static func read(bundleID: String, account: String) -> String? {
        var query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service + "." + bundleID,
            kSecAttrAccount as String: account,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]
        query[kSecAttrSynchronizable as String] = kCFBooleanTrue
        var item: CFTypeRef?
        guard SecItemCopyMatching(query as CFDictionary, &item) == errSecSuccess,
              let data = item as? Data,
              let text = String(data: data, encoding: .utf8) else { return nil }
        return text
    }

    static func write(bundleID: String, account: String, value: String) {
        let base: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service + "." + bundleID,
            kSecAttrAccount as String: account,
            kSecAttrSynchronizable as String: kCFBooleanTrue
        ]
        SecItemDelete(base as CFDictionary)
        var add = base
        add[kSecValueData as String] = Data(value.utf8)
        add[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        SecItemAdd(add as CFDictionary, nil)
    }
}

enum ProTrialClock {
    static func startDate(bundleID: String) -> Date? {
        guard let raw = ProKeychain.read(bundleID: bundleID, account: "trial.start"),
              let stamp = Double(raw) else { return nil }
        return Date(timeIntervalSince1970: stamp)
    }

    static func startIfNeeded(bundleID: String) -> Date {
        if let existing = startDate(bundleID: bundleID) { return existing }
        let now = Date()
        ProKeychain.write(bundleID: bundleID, account: "trial.start", value: String(now.timeIntervalSince1970))
        return now
    }

    static func isUnlockedFlag(bundleID: String) -> Bool {
        ProKeychain.read(bundleID: bundleID, account: "unlocked") == "1"
    }

    static func markUnlocked(bundleID: String) {
        ProKeychain.write(bundleID: bundleID, account: "unlocked", value: "1")
    }
}

// MARK: - Store

@MainActor
@Observable
final class ProStore {
    enum Access: Equatable {
        case loading
        case trial(daysLeft: Int)
        case locked
        case unlocked
    }

    let offer: ProOffer
    private(set) var access: Access = .loading
    private(set) var product: Product?
    private(set) var isBusy = false
    private(set) var message: String?

    private let bundleID: String
    private var started = false

    init(offer: ProOffer) {
        self.offer = offer
        self.bundleID = Bundle.main.bundleIdentifier ?? "unknown"
    }

    var isLocked: Bool { access == .locked }
    var isUnlocked: Bool { access == .unlocked }
    var daysLeft: Int? {
        if case .trial(let days) = access { return days }
        return nil
    }

    var statusText: String {
        switch access {
        case .loading: return "正在检查购买状态…"
        case .unlocked: return "完整版已解锁"
        case .trial(let days): return "免费试用中 · 剩余 \(days) 天"
        case .locked: return "试用已结束 · 需要解锁"
        }
    }

    var displayPrice: String { product?.displayPrice ?? "¥8.00" }

    func start() async {
        guard !started else { return }
        started = true
        #if DEBUG
        if ProcessInfo.processInfo.arguments.contains("-proForceLocked") {
            access = .locked
            Task { await loadProduct() }
            return
        }
        #endif
        if ProTrialClock.isUnlockedFlag(bundleID: bundleID) {
            access = .unlocked
        } else if await hasEntitlement() {
            ProTrialClock.markUnlocked(bundleID: bundleID)
            access = .unlocked
        } else if await legacyPurchase() {
            ProTrialClock.markUnlocked(bundleID: bundleID)
            access = .unlocked
        } else {
            let start = ProTrialClock.startIfNeeded(bundleID: bundleID)
            let elapsed = Date().timeIntervalSince(start)
            let remaining = TimeInterval(offer.trialDays) * 86_400 - elapsed
            access = remaining > 0 ? .trial(daysLeft: max(1, Int(ceil(remaining / 86_400)))) : .locked
        }
        await loadProduct()
    }

    func refresh() async {
        guard access != .unlocked else { return }
        if await hasEntitlement() {
            ProTrialClock.markUnlocked(bundleID: bundleID)
            access = .unlocked
        }
    }

    func purchase() async {
        guard let product else {
            message = "暂时无法连接 App Store，请稍后再试。"
            return
        }
        isBusy = true
        message = nil
        defer { isBusy = false }
        do {
            let result = try await product.purchase()
            switch result {
            case .success(let verification):
                if case .verified(let transaction) = verification {
                    await transaction.finish()
                    ProTrialClock.markUnlocked(bundleID: bundleID)
                    access = .unlocked
                }
            case .pending:
                message = "购买正在等待批准。"
            case .userCancelled:
                break
            @unknown default:
                break
            }
        } catch {
            message = error.localizedDescription
        }
    }

    func restore() async {
        isBusy = true
        message = nil
        defer { isBusy = false }
        do {
            try await AppStore.sync()
            if await hasEntitlement() {
                ProTrialClock.markUnlocked(bundleID: bundleID)
                access = .unlocked
            } else {
                message = "没有找到可恢复的购买记录。"
            }
        } catch {
            message = error.localizedDescription
        }
    }

    func dismissMessage() { message = nil }

    private func loadProduct() async {
        guard product == nil else { return }
        product = try? await Product.products(for: [offer.productID]).first
    }

    private func hasEntitlement() async -> Bool {
        for await result in Transaction.currentEntitlements {
            guard case .verified(let transaction) = result,
                  transaction.productID == offer.productID,
                  transaction.revocationDate == nil else { continue }
            return true
        }
        return false
    }

    private func legacyPurchase() async -> Bool {
        guard let result = try? await AppTransaction.shared,
              case .verified(let transaction) = result else { return false }
        return transaction.originalPurchaseDate < offer.legacyCutover
    }
}