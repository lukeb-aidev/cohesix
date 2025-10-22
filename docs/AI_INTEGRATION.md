<!-- Author: Lukas Bower -->
# Apple Intelligence Integration Guide

## System Roles
- **Rust (`domain-intents`)** — Owns curated datasets, parameter resolution,
  contextual question generation, and donation payload construction.
- **Swift (`ios/Concierge/ConciergeHomeView.swift`)** — Presents the home view,
  dispatches FFI calls into Rust, schedules preview notifications, and donates
  results back to Apple Intelligence.
- **App Intents Layer** — Consumes the `IntentDonation` payloads and surfaces
  shortcuts, Siri responses, and proactive suggestions. No business logic lives
  here; it simply forwards metadata from Rust.

## Parameter Resolution Flow
1. Swift receives a natural-language request (e.g., “Play something upbeat for a
   late-night focus session”).
2. The request is converted into a `MediaIntentParameters` struct and passed to
   `PlaySomethingIntent::resolve_parameters`.
3. Rust returns a `MediaIntentResolution` containing:
   - an optional `MediaPickQuery` if enough information was supplied;
   - a vector of `ContextualQuestion` values describing missing context;
   - implicit permission checks via `PermissionGrant` questions.
4. Swift renders the questions verbatim using `ContextualQuestion::prompt()` and
   only proceeds to recommendation when the query is available.

## Donation Workflow
1. After `PlaySomethingIntent::recommend` or `FinanceSnapshotIntent::snapshot`
   succeeds, call the respective `donation_payload` helper.
2. Convert the returned `IntentDonation` into an `INRelevantShortcut` or
   `INInteraction` and donate immediately.
3. Donation metadata mirrors the curated catalog, ensuring Siri answers (“What
   did Ralph suggest tonight?”) remain deterministic across reboots.

## Contextual Questions
- Always display the prompts returned by `ContextualQuestion::prompt()`; do not
  rephrase.
- Treat `PermissionGrant` as blocking. The home view should prompt the user to
  open Settings or present an inline explanation before reattempting.
- For finance flows, require explicit confirmation of `FinanceDataAccess` before
  rendering totals or donating results.

## Notification Hand-off
- `ConciergeNotificationCoordinator` is the sole notification entry point. It
  schedules lightweight preview notifications that direct the user back to Ralph
  without leaking personalised data.
- Notifications trigger the same resolution flow as manual requests. On receipt,
  Swift replays the request through `resolve_parameters` to obtain updated
  contextual questions before presenting the refreshed suggestion.

## Dead Code Discipline
- Run `tools/find_dead_symbols.swift --root ios/Concierge` during CI. The script
  executes `swiftc -typecheck` with unused-declaration warnings enabled and
  prints a JSON report of any unused functions, methods, or stored properties.
- Update `dead_code_report.md` after each run. A green build requires `0 dead
  symbols` to ensure the concierge UI stays lean.

## Future Hooks
- Additional intents must extend `ContextualQuestion` rather than introducing ad
  hoc enums on the Swift side. This keeps localisation and testing centralised in
  Rust.
- Any new dataset must be reflected in `docs/SUGGESTION_CATALOG.md` so Apple
  Intelligence documentation, tests, and user-facing copy remain synchronised.
