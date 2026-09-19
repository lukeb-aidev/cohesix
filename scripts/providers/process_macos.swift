// Author: Lukas Bower
// Purpose: Observe native macOS process incarnation and executable identity without exporting argv or environment.
// Copyright 2026 Lukas Bower
import Foundation
import CryptoKit
import Darwin

func inspect(_ pid: Int32) throws -> [String: Any] {
    var info = proc_bsdinfo()
    let count = proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, Int32(MemoryLayout<proc_bsdinfo>.size))
    if count == 0 && errno == ESRCH {
        return ["schema": "cohesix-macos-process/v1", "pid": pid, "alive": false]
    }
    guard count == MemoryLayout<proc_bsdinfo>.size, info.pbi_pid == pid else {
        throw NSError(domain: "native-process-observation", code: 1)
    }
    // sys/proc_info.h defines PROC_PIDPATHINFO_MAXSIZE as 4 * MAXPATHLEN;
    // Swift cannot import that compound macro.
    var path = [CChar](repeating: 0, count: 4 * Int(MAXPATHLEN))
    guard proc_pidpath(pid, &path, UInt32(path.count)) > 0 else {
        throw NSError(domain: "native-process-path", code: 2)
    }
    let url = URL(fileURLWithPath: String(cString: path))
    let file = try FileHandle(forReadingFrom: url)
    defer { try? file.close() }
    var digest = SHA256()
    var total = 0
    while let chunk = try file.read(upToCount: 65536), !chunk.isEmpty {
        total += chunk.count
        guard total <= 268435456 else { throw NSError(domain: "native-executable-bound", code: 3) }
        digest.update(data: chunk)
    }
    var after = proc_bsdinfo()
    guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &after, Int32(MemoryLayout<proc_bsdinfo>.size)) == MemoryLayout<proc_bsdinfo>.size,
          after.pbi_start_tvsec == info.pbi_start_tvsec, after.pbi_start_tvusec == info.pbi_start_tvusec,
          after.pbi_uid == info.pbi_uid else { throw NSError(domain: "native-process-changed", code: 4) }
    return ["schema": "cohesix-macos-process/v1", "pid": pid, "alive": true,
            "uid": info.pbi_uid, "start_seconds": info.pbi_start_tvsec,
            "start_microseconds": info.pbi_start_tvusec,
            "executable_sha256": digest.finalize().map { String(format: "%02x", $0) }.joined()]
}

do {
    guard CommandLine.arguments.count == 2, let pid = Int32(CommandLine.arguments[1]), pid > 0 else {
        throw NSError(domain: "invalid-native-pid", code: 5)
    }
    let data = try JSONSerialization.data(withJSONObject: inspect(pid), options: [.sortedKeys])
    FileHandle.standardOutput.write(data)
    FileHandle.standardOutput.write(Data([10]))
} catch {
    FileHandle.standardError.write(Data("unavailable native-process-observation\n".utf8))
    exit(1)
}
