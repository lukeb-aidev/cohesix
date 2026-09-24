// swift-tools-version: 6.4
// Author: Lukas Bower
// Purpose: Build and test shared SwarmUI Apple capability, gateway and Keychain components.
// Copyright 2026 Lukas Bower

import PackageDescription

let package = Package(
    name: "CohesixApple",
    platforms: [.macOS(.v27)],
    products: [
        .library(name: "CohesixApple", targets: ["CohesixApple"]),
        .executable(name: "CohesixPlatformProbe", targets: ["CohesixPlatformProbe"]),
        .executable(name: "CohesixAssistanceProbe", targets: ["CohesixAssistanceProbe"]),
    ],
    targets: [
        .target(name: "CohesixApple"),
        .executableTarget(name: "CohesixPlatformProbe", dependencies: ["CohesixApple"]),
        .executableTarget(name: "CohesixAssistanceProbe", dependencies: ["CohesixApple"]),
        .testTarget(name: "CohesixAppleTests", dependencies: ["CohesixApple"]),
    ]
)
