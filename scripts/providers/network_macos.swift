// Author: Lukas Bower
// Purpose: Read bounded native macOS interface identities, addresses, link counters and default routing state without changing host networking.
// Copyright 2026 Lukas Bower
// SPDX-License-Identifier: Apache-2.0
import Darwin
import Foundation
import SystemConfiguration

enum ProbeError: Error { case unavailable, limit, invalid }

func observe() throws -> [String: Any] {
    var first: UnsafeMutablePointer<ifaddrs>?
    guard getifaddrs(&first) == 0 else { throw ProbeError.unavailable }
    defer { freeifaddrs(first) }
    var interfaces: [String: [String: Any]] = [:]
    var addresses: [[String: Any]] = []
    var cursor = first
    var rows = 0
    while let entry = cursor {
        rows += 1
        guard rows <= 256 else { throw ProbeError.limit }
        let value = entry.pointee
        cursor = value.ifa_next
        guard let namePointer = value.ifa_name else { throw ProbeError.invalid }
        let name = String(cString: namePointer)
        guard !name.isEmpty, name.utf8.count <= 32 else { throw ProbeError.invalid }
        var interface = interfaces[name] ?? ["name": name]
        interface["flags"] = value.ifa_flags
        interface["up"] = value.ifa_flags & UInt32(IFF_UP) != 0
        interface["running"] = value.ifa_flags & UInt32(IFF_RUNNING) != 0
        if let address = value.ifa_addr {
            let family = Int32(address.pointee.sa_family)
            if family == AF_LINK, let rawData = value.ifa_data {
                // getifaddrs supplies if_data for an AF_LINK record under Darwin's documented ABI.
                let data = rawData.assumingMemoryBound(to: if_data.self).pointee
                interface["mtu"] = data.ifi_mtu
                interface["received_bytes"] = data.ifi_ibytes
                interface["sent_bytes"] = data.ifi_obytes
                interface["received_packets"] = data.ifi_ipackets
                interface["sent_packets"] = data.ifi_opackets
                interface["receive_errors"] = data.ifi_ierrors
                interface["send_errors"] = data.ifi_oerrors
                interface["receive_drops"] = data.ifi_iqdrops
                interface["counter_width_bits"] = 32
            }
            if family == AF_INET || family == AF_INET6 {
                var output = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                let length = socklen_t(address.pointee.sa_len)
                guard length > 0, getnameinfo(address, length, &output,
                    socklen_t(output.count), nil, 0, NI_NUMERICHOST) == 0 else { throw ProbeError.invalid }
                addresses.append(["interface": name, "family": family,
                    "address": String(cString: output)])
            }
        }
        interfaces[name] = interface
        guard interfaces.count <= 32, addresses.count <= 128 else { throw ProbeError.limit }
    }
    guard let store = SCDynamicStoreCreate(nil, "cohesix-network-observer" as CFString, nil, nil) else {
        throw ProbeError.unavailable
    }
    var routes: [String: Any] = [:]
    for family in ["IPv4", "IPv6"] {
        if let state = SCDynamicStoreCopyValue(store, "State:/Network/Global/\(family)" as CFString) as? [String: Any] {
            var selected: [String: Any] = [:]
            for field in ["Router", "PrimaryInterface", "PrimaryService"] {
                if let value = state[field] as? String, value.utf8.count <= 256 { selected[field] = value }
            }
            routes[family] = selected
        } else { routes[family] = ["state": "unavailable"] }
    }
    return ["schema": "cohesix-macos-network/v1", "source": "Darwin-getifaddrs-and-SystemConfiguration",
        "interfaces": interfaces.keys.sorted().compactMap { interfaces[$0] }, "addresses": addresses,
        "default_routes": routes, "counter_semantics": "native-32-bit-wrapping-snapshot"]
}

do {
    guard CommandLine.arguments.count == 1 else { throw ProbeError.invalid }
    let data = try JSONSerialization.data(withJSONObject: observe(), options: [.sortedKeys])
    guard data.count <= 65536 else { throw ProbeError.limit }
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([10]))
} catch {
    FileHandle.standardError.write(Data("not_supported macOS native network observation\n".utf8))
    exit(2)
}
