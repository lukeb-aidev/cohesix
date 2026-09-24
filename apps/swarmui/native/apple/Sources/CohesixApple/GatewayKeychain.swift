// Author: Lukas Bower
// Purpose: Share one delegated Hive Gateway enrollment between signed SwarmUI components through Keychain.
// Copyright 2026 Lukas Bower

import Foundation
import Security

public enum GatewayKeychain {
    private static let service = "com.cohesix.swarmui.gateway"
    private static let account = "selected-delegation"
    private static let maximumBytes = 16_384

    private struct Stored: Decodable {
        let endpoint: String
        let requestToken: String
        let delegatedTicket: String
    }

    private static var query: [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
            kSecUseDataProtectionKeychain as String: kCFBooleanTrue as Any,
        ]
    }

    public static func load() throws -> GatewayCredentials {
        var selected = query
        selected[kSecReturnData as String] = kCFBooleanTrue
        selected[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(selected as CFDictionary, &result)
        guard status == errSecSuccess, let bytes = result as? Data,
              bytes.count <= maximumBytes,
              let stored = try? JSONDecoder().decode(Stored.self, from: bytes)
        else { throw GatewayJobError.invalidCredential }
        return try GatewayCredentials(
            endpoint: stored.endpoint,
            requestToken: stored.requestToken,
            delegatedTicket: stored.delegatedTicket
        )
    }

}
