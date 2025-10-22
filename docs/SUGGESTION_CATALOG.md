<!-- Author: Lukas Bower -->
# Concierge Suggestion Catalog

## Media Library (PlaySomethingIntent)
| ID | Title | Domain | Provider | Duration (min) | Dayparts | Weeknight | Tags |
| --- | --- | --- | --- | --- | --- | --- | --- |
| musickit-midnight-cities | Midnight Cities — Synthwave mix for focused evenings | Music | MusicKit | 18 | Evening, Late night | Yes | focus, instrumental |
| appletv-tranquil | The Tranquil Paradox — Season 1, Episode 4 | TV | TV+ | 28 | Evening | Yes | mystery, serialized |
| podcasts-maker-habits | Maker Habits: Field Notes — Episode 92 | Podcast | Podcasts | 24 | Morning, Afternoon | Yes | productivity, interview |
| books-astro-journal | Astro Journal — Evening reflection excerpt | Book | Books | 35 | Evening, Late night | Yes | mindfulness, shortform |
| appletv-sunday-special | Sunday Special — Docuseries episode | TV | TV+ | 52 | Afternoon | No | documentary |

- Durations and dayparts match the static constructors in
  [`MediaLibrary::curated`](../crates/domain-intents/src/lib.rs).
- Tags map to tone descriptors consumed by `MediaIntentParameters` and surfaced
  in contextual questions.
- All deeplinks use the provider-specific scheme validated by `Deeplink::new`.

## Finance Dataset (FinanceSnapshotIntent)
| Name | Category | Monthly Cost (USD) | Renews within week | Deeplink |
| --- | --- | --- | --- | --- |
| Apple One Premier | Subscription | 32.95 | No | `prefs:root=SUBSCRIPTIONS&name=appleone` |
| Creative Cloud | Subscription | 59.99 | Yes | `prefs:root=SUBSCRIPTIONS&name=adobe` |
| Metropolitan Transit | Subscription | 48.50 | Yes | `prefs:root=SUBSCRIPTIONS&name=transit` |

- Daily totals: inflow 620.00, outflow 410.00 (net +210.00).
- Weekly totals: inflow 3820.00, outflow 2645.00 (net +1175.00).
- These values are encoded in [`FinanceDataset::mock`](../crates/domain-intents/src/lib.rs)
  and referenced in tests and donation payloads.

## Maintenance Checklist
1. Update this table whenever `MediaLibrary::curated` or `FinanceDataset::mock`
   change.
2. Re-run `cargo test -p domain-intents` to ensure rationale strings and
   donation metadata stay aligned.
3. Verify `tools/find_dead_symbols.swift` reports `0 dead symbols` after
   removing or renaming entries that the concierge UI no longer presents.
