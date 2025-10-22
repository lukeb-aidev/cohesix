<!-- Author: Lukas Bower -->
# Dead Code Report

## Summary
- Scope: `ios/Concierge` and supporting Swift tooling.
- Tool: `tools/find_dead_symbols.swift --root ios/Concierge`.
- Result: `0` dead symbols detected.

## Command Transcript
```
$ tools/find_dead_symbols.swift --root ios/Concierge
No dead symbols detected.
```

The script should be executed on the macOS ARM64 toolchain described in
`docs/TOOLCHAIN_MAC_ARM64.md`. It invokes `swiftc -typecheck` with unused
symbol warnings enabled and surfaces any stale declarations as JSON or
human-readable text. Keep this report updated whenever Swift sources change.
