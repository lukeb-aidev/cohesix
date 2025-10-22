<!-- Author: Lukas Bower -->
# Privacy Posture for Concierge & Intents

## Data Boundaries
- All concierge experiences operate entirely on-device. The curated
  media catalog and finance dataset shipped in `domain-intents` are
  deterministic fixtures and never fetch remote data.
- Permissions are explicit: media providers require capability tickets
  from the user, and finance snapshots are blocked until
  `FinancePermissions::allow_local_data` is `true`.
- No identifiers leave the VM. Donation payloads encode deeplinks and
  metadata but are persisted only within Apple Intelligence surfaces.

## Conversational Safeguards
- Contextual questions exposed via `ContextualQuestion::prompt()` avoid
  collecting unnecessary personal data. Each prompt maps to a required
  field (domain, daypart, runtime, tone, or permission) and is only asked
  if missing from the user’s request.
- Finance prompts always ask for consent before referencing inflow/outflow
  totals. Without approval the SwiftUI surface renders guidance copy and
  blocks the donation path.

## Notification Discipline
- `ConciergeNotificationCoordinator` schedules preview notifications with
  a dedicated identifier (`cohesix.concierge.preview`). Notifications are
  opt-in and respect the user’s system-wide alert settings.
- Preview copy references Ralph explicitly (“Ask Ralph for a fresh
  recommendation...”) to prevent confusion about the source of the alert.
- Notifications do not contain personalised content—only prompts that
  direct users back into the home view where permissions and context are
  enforced.

## Logging & Telemetry
- No analytics libraries are embedded in the SwiftUI layer. Errors are
  logged via `NSLog` for local debugging only and are never uploaded.
- Rust-side logging remains in `/log/queen.log`. Apple Intelligence flows
  consume Rust APIs but do not add new logging sinks or network endpoints.

## Data Retention
- Suggested media and finance snapshots are held in memory long enough to
  render the home view and to emit an intent donation. Apple Intelligence
  manages retention per standard iOS policies; Cohesix does not persist
  archives or history files.
- Legacy UIKit surfaces were deleted. The remaining deprecated adapters are
  marked with `@available(*, deprecated, message: "Replaced by RalphConcierge")`
  to prevent new entry points that might bypass these privacy guarantees.
