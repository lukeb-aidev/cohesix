// Author: Lukas Bower
// Purpose: Emit a bounded actual-device Apple AI and Metal capability observation for the M28c host gate.
// Copyright 2026 Lukas Bower

import CohesixApple
import Foundation
import Metal

let capabilities = PlatformCapabilities.current()
let output: [String: Any] = [
    "schema": "cohesix-apple-platform-probe/v1",
    "os_version": ProcessInfo.processInfo.operatingSystemVersionString,
    "locale": Locale.current.identifier,
    "foundation_model_available": capabilities.foundationModelAvailable,
    "foundation_model_supports_locale": capabilities.foundationModelSupportsLocale,
    "metal_device_available": capabilities.metalDeviceAvailable,
    "metal_device_name": MTLCreateSystemDefaultDevice()?.name ?? "",
    "unified_memory": capabilities.unifiedMemory,
]
let data = try JSONSerialization.data(withJSONObject: output, options: [.sortedKeys])
FileHandle.standardOutput.write(data)
FileHandle.standardOutput.write(Data([0x0a]))
