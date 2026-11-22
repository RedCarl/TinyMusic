import StoreKit
import UIKit

@MainActor
enum ReviewPrompt {
    private static let key = "pro.successCount"

    static func registerSuccess() {
        let count = UserDefaults.standard.integer(forKey: key) + 1
        UserDefaults.standard.set(count, forKey: key)
        guard count == 3 else { return }
        guard let scene = UIApplication.shared.connectedScenes
            .compactMap({ $0 as? UIWindowScene })
            .first(where: { $0.activationState == .foregroundActive }) else { return }
        SKStoreReviewController.requestReview(in: scene)
    }
}