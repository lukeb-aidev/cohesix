<!-- Author: Lukas Bower -->
# Domain Intents & Apple Intelligence Surfaces

## Overview
Cohesix exposes deterministic intent handlers in the `domain-intents` crate so
Apple Intelligence can orchestrate media and finance experiences without
reaching external services. The crate wraps curated catalogs and permission
stores, returning stable data that mirrors the automated tests. The concierge
SwiftUI layer (`ios/Concierge/ConciergeHomeView.swift`) binds directly to these
Rust primitives via FFI, translating conversational requests into
`MediaPickQuery` and `FinanceSnapshot` responses.

## PlaySomethingIntent Flow
1. **Parameter Resolution** — Incoming requests populate
   [`MediaIntentParameters`](../crates/domain-intents/src/lib.rs) with optional
   domains, dayparts, runtime hints, and tone descriptors. The
   [`resolve_parameters`](../crates/domain-intents/src/lib.rs) method evaluates
   each parameter and returns a [`MediaIntentResolution`](../crates/domain-intents/src/lib.rs)
   containing the compiled query (if sufficient context exists) alongside
   follow-up [`ContextualQuestion`](../crates/domain-intents/src/lib.rs) prompts.
   - Missing domains trigger `ContextualQuestion::MediaDomain` so Ralph can ask
     “Would you like music, TV, podcasts, or a book recommendation?”
   - Empty dayparts or runtime hints surface `PreferredDaypart` and
     `IdealDuration` prompts that keep the conversation on-device.
   - If the requested domain requires a capability ticket that the user has not
     granted (e.g., Apple TV+), the resolution flags a
     `PermissionGrant { provider }` question that the Swift layer converts into a
     permission dialog.
2. **Suggestion Scoring** — With a fully populated query the intent executes the
   deterministic `MediaLibrary::curated` search. Scores are derived from runtime
   and cadence alignment against the caller’s [`UserPreferences`], ensuring that
   repeated donations gradually reflect the household’s habits.
3. **Donation Payload** — The selected [`MediaSuggestion`] is passed to
   [`donation_payload`](../crates/domain-intents/src/lib.rs) which emits an
   `IntentDonation` structure. The Swift App Intent publishes the donation so
   SiriKit shortcuts and Spotlight can re-surface the suggestion with identical
   metadata (`domain`, `provider`, `score`, `rationale`).
4. **Contextual Questions** — The concierge home view pulls the questions from
   `MediaIntentResolution` to show inline coaching. The same prompts are exposed
   to the notifications pipeline so follow-up alerts stay consistent with the
   live conversation.

## FinanceSnapshotIntent Flow
1. **Permission Gate** — Finance flows require explicit approval to use local
   data. [`FinanceSnapshotIntent::contextual_questions`](../crates/domain-intents/src/lib.rs)
   returns `ContextualQuestion::FinanceDataAccess` whenever
   `FinancePermissions::allow_local_data` is `false`, ensuring the Swift layer
   cannot render totals until consent is granted.
2. **Snapshot Assembly** — `FinanceDataset::mock()` provides deterministic daily
   and weekly inflow/outflow totals along with subscription metadata. The
   [`snapshot`](../crates/domain-intents/src/lib.rs) method converts this into a
   `FinanceSnapshot` struct that downstream code renders without network access.
3. **Donation Payload** — Calling
   [`FinanceSnapshotIntent::donation_payload`](../crates/domain-intents/src/lib.rs)
   produces an `IntentDonation` containing formatted net changes and counts of
   tracked subscriptions. The Swift App Intent donates this payload so Siri can
   answer contextual finance questions (“How is my weekly spending doing?”)
   using purely on-device data.

## Conversational Loop Expectations
- All contextual prompts are sourced from the Rust layer. The Swift UI must not
  invent copy; instead it displays the result of `ContextualQuestion::prompt()`
  to stay aligned with localisation and testing expectations.
- Ralph’s concierge home view is the only UI surface that renders these flows.
  All other UIKit or storyboard artefacts were removed; legacy entry points are
  tagged with `@available(*, deprecated, message: "Replaced by RalphConcierge")`
  so new work cannot accidentally revive them.
- Donations are emitted immediately after successful recommendations. The
  Swift App Intent is responsible for calling `donation_payload` and forwarding
  the result to `INInteraction` or `INRelevantShortcutStore`.

## Testing Hooks
- Unit tests inside `crates/domain-intents` validate parameter resolution, the
  contextual question catalogue, donation payload serialisation, and permission
  gating. Whenever the curated catalog changes, refresh the tables in
  `docs/SUGGESTION_CATALOG.md` and re-run `cargo test -p domain-intents`.
- `tools/find_dead_symbols.swift` must report zero unused Swift types. This
  keeps the concierge surface lean and guarantees the docs reflect the running
  system.
