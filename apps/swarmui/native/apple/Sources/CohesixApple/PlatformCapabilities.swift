// Author: Lukas Bower
// Purpose: Report actual on-device Apple AI and Metal availability without implying Cohesix authority.
// Copyright 2026 Lukas Bower

import Foundation
import FoundationModels
import Metal

public struct PlatformCapabilities: Equatable, Sendable {
    public let foundationModelAvailable: Bool
    public let foundationModelSupportsLocale: Bool
    public let metalDeviceAvailable: Bool
    public let unifiedMemory: Bool

    public init(
        foundationModelAvailable: Bool,
        foundationModelSupportsLocale: Bool,
        metalDeviceAvailable: Bool,
        unifiedMemory: Bool
    ) {
        self.foundationModelAvailable = foundationModelAvailable
        self.foundationModelSupportsLocale = foundationModelSupportsLocale
        self.metalDeviceAvailable = metalDeviceAvailable
        self.unifiedMemory = unifiedMemory
    }

    public static func current() -> Self {
        let model = SystemLanguageModel.default
        let device = MTLCreateSystemDefaultDevice()
        return Self(
            foundationModelAvailable: model.isAvailable,
            foundationModelSupportsLocale: model.supportsLocale(.current),
            metalDeviceAvailable: device != nil,
            unifiedMemory: device?.hasUnifiedMemory ?? false
        )
    }

    public var summary: String {
        let modelState = foundationModelAvailable && foundationModelSupportsLocale
            ? "available" : "unavailable"
        let metalState = metalDeviceAvailable && unifiedMemory
            ? "available" : "unavailable"
        return "Apple assistance: \(modelState); local Metal: \(metalState). No hive action was requested."
    }
}
