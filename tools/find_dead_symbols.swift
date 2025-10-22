#!/usr/bin/env swift
// Author: Lukas Bower

import Foundation

struct DeadSymbol: Codable {
    let file: String
    let line: Int
    let column: Int
    let message: String
    let symbol: String
}

struct DeadSymbolReport: Codable {
    let unused: [DeadSymbol]
}

enum OutputFormat {
    case text
    case json
}

struct Options {
    var roots: [String] = [FileManager.default.currentDirectoryPath]
    var format: OutputFormat = .text
}

enum ToolError: Error, CustomStringConvertible {
    case swiftcNotFound(String)
    case invocationFailed(Int32)

    var description: String {
        switch self {
        case .swiftcNotFound(let error):
            return "Unable to locate swiftc: \(error)"
        case .invocationFailed(let code):
            return "swiftc invocation failed with exit code \(code)"
        }
    }
}

func parseArguments() -> Options {
    var iterator = CommandLine.arguments.dropFirst().makeIterator()
    var options = Options()
    while let argument = iterator.next() {
        switch argument {
        case "--root":
            if let value = iterator.next() {
                options.roots.append(value)
            }
        case "--json":
            options.format = .json
        default:
            continue
        }
    }
    if options.roots.isEmpty {
        options.roots = [FileManager.default.currentDirectoryPath]
    }
    return options
}

func discoverSwiftFiles(roots: [String]) -> [String] {
    var results: [String] = []
    let fileManager = FileManager.default
    for root in roots {
        let rootURL = URL(fileURLWithPath: root)
        guard let enumerator = fileManager.enumerator(at: rootURL, includingPropertiesForKeys: nil) else {
            continue
        }
        for case let fileURL as URL in enumerator {
            if fileURL.pathExtension == "swift" {
                results.append(fileURL.path)
            }
        }
    }
    return results
}

func locateSwiftc() throws -> String {
    let process = Process()
    let pipe = Pipe()
    process.standardOutput = pipe
    process.standardError = pipe
    process.launchPath = "/usr/bin/env"
    process.arguments = ["xcrun", "--find", "swiftc"]
    try process.run()
    process.waitUntilExit()
    if process.terminationStatus == 0,
       let data = try? pipe.fileHandleForReading.readToEnd(),
       let path = String(data: data, encoding: .utf8)?.trimmingCharacters(in: .whitespacesAndNewlines),
       !path.isEmpty {
        return path
    }
    if FileManager.default.isExecutableFile(atPath: "/usr/bin/swiftc") {
        return "/usr/bin/swiftc"
    }
    throw ToolError.swiftcNotFound("xcrun could not locate swiftc and /usr/bin/swiftc is missing")
}

func runSwiftc(swiftcPath: String, files: [String]) throws -> String {
    let process = Process()
    let pipe = Pipe()
    process.standardError = pipe
    process.standardOutput = Pipe()
    process.launchPath = swiftcPath
    process.arguments = ["-typecheck",
                         "-warn-unused-function",
                         "-warn-unused-variable",
                         "-Xfrontend", "-warn-unused-private-decls",
                         "-Xfrontend", "-warn-unused-imports"] + files
    try process.run()
    process.waitUntilExit()
    if process.terminationStatus != 0 {
        throw ToolError.invocationFailed(process.terminationStatus)
    }
    if let data = try? pipe.fileHandleForReading.readToEnd(),
       let output = String(data: data, encoding: .utf8) {
        return output
    }
    return ""
}

func parseDeadSymbols(from diagnostics: String, relativeTo roots: [String]) -> [DeadSymbol] {
    let lines = diagnostics.split(separator: "\n")
    var symbols: [DeadSymbol] = []
    let warningPattern = try! NSRegularExpression(pattern: "^(.*\\.swift):(\\d+):(\\d+): warning: (.*)$", options: [])
    let symbolPattern = try! NSRegularExpression(pattern: "'([^']+)'", options: [])
    for line in lines {
        let lineString = String(line)
        let range = NSRange(location: 0, length: lineString.utf16.count)
        guard let match = warningPattern.firstMatch(in: lineString, options: [], range: range) else {
            continue
        }
        let fileRange = Range(match.range(at: 1), in: lineString)!
        let lineNumberRange = Range(match.range(at: 2), in: lineString)!
        let columnRange = Range(match.range(at: 3), in: lineString)!
        let messageRange = Range(match.range(at: 4), in: lineString)!
        let message = String(lineString[messageRange])
        guard message.contains("never used") || message.contains("never read") else {
            continue
        }
        let filePath = String(lineString[fileRange])
        let relativeFile = relativise(path: filePath, roots: roots)
        let lineNumber = Int(lineString[lineNumberRange]) ?? 0
        let column = Int(lineString[columnRange]) ?? 0
        let symbolMatch = symbolPattern.firstMatch(in: message, options: [], range: NSRange(location: 0, length: message.utf16.count))
        let symbol: String
        if let symbolMatch,
           let symbolRange = Range(symbolMatch.range(at: 1), in: message) {
            symbol = String(message[symbolRange])
        } else {
            symbol = message
        }
        symbols.append(DeadSymbol(file: relativeFile, line: lineNumber, column: column, message: message, symbol: symbol))
    }
    return symbols
}

func relativise(path: String, roots: [String]) -> String {
    let pathURL = URL(fileURLWithPath: path)
    for root in roots {
        let rootURL = URL(fileURLWithPath: root)
        if pathURL.path.hasPrefix(rootURL.path) {
            let relative = pathURL.path.replacingOccurrences(of: rootURL.path, with: "")
            return relative.trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        }
    }
    return path
}

func emitReport(_ report: DeadSymbolReport, format: OutputFormat) {
    switch format {
    case .json:
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        if let data = try? encoder.encode(report) {
            FileHandle.standardOutput.write(data)
            FileHandle.standardOutput.write(Data([0x0a]))
        }
    case .text:
        if report.unused.isEmpty {
            print("No dead symbols detected.")
        } else {
            print("Dead symbols detected:\n")
            for symbol in report.unused {
                print("- \(symbol.symbol) — \(symbol.file):\(symbol.line)")
                print("  \(symbol.message)")
            }
        }
    }
}

let options = parseArguments()
let swiftFiles = discoverSwiftFiles(roots: options.roots)
if swiftFiles.isEmpty {
    emitReport(DeadSymbolReport(unused: []), format: options.format)
    exit(EXIT_SUCCESS)
}

do {
    let swiftc = try locateSwiftc()
    let diagnostics = try runSwiftc(swiftcPath: swiftc, files: swiftFiles)
    let deadSymbols = parseDeadSymbols(from: diagnostics, relativeTo: options.roots)
    let report = DeadSymbolReport(unused: deadSymbols)
    emitReport(report, format: options.format)
    exit(deadSymbols.isEmpty ? EXIT_SUCCESS : EXIT_FAILURE)
} catch {
    fputs("find_dead_symbols.swift error: \(error)\n", stderr)
    exit(EXIT_FAILURE)
}
