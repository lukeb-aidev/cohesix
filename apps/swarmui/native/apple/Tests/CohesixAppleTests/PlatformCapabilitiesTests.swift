// Author: Lukas Bower
// Purpose: Check that the native probe never reports unavailable capabilities as ready.
// Copyright 2026 Lukas Bower

import CohesixApple
import Testing

@Test func unavailableModelIsNotAdvertised() {
    let capabilities = PlatformCapabilities(
        foundationModelAvailable: false,
        foundationModelSupportsLocale: true,
        metalDeviceAvailable: true,
        unifiedMemory: true
    )
    #expect(capabilities.summary.contains("Apple assistance: unavailable"))
    #expect(capabilities.summary.contains("local Metal: available"))
    #expect(capabilities.summary.contains("No hive action was requested"))
}

@Test func unsuitableMetalIsNotAdvertised() {
    let capabilities = PlatformCapabilities(
        foundationModelAvailable: true,
        foundationModelSupportsLocale: true,
        metalDeviceAvailable: true,
        unifiedMemory: false
    )
    #expect(capabilities.summary.contains("Apple assistance: available"))
    #expect(capabilities.summary.contains("local Metal: unavailable"))
}
