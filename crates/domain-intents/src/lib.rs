// Author: Lukas Bower
#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Context-aware domain suggestion engine for media and finance intents.
//!
//! The module focuses on two primary user flows:
//! - [`PlaySomethingIntent`] surfaces music, TV, podcast, and book
//!   recommendations based on contextual filters captured via
//!   [`MediaPickQuery`] and gradually tunes itself using
//!   [`UserPreferences`].
//! - [`FinanceSnapshotIntent`] exposes a consolidated daily and weekly
//!   summary backed by mock data alongside a lightweight subscription
//!   audit suitable for local-only presentation layers.
//!
//! All surfaces return deterministic mock content so unit tests can verify
//! behaviour without reaching external providers such as MusicKit or
//! Apple TV+. Deeplinks are validated at construction time to make sure
//! consumer applications can render actionable links, while permission
//! shortfalls yield descriptive errors instead of panics.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;
use std::time::Duration;

use anyhow::Result;
use thiserror::Error;

/// Media content domains supported by [`PlaySomethingIntent`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaDomain {
    /// Streamable music tracks or playlists.
    Music,
    /// TV+ episodes, films, or specials.
    Tv,
    /// Apple Podcasts episodes.
    Podcast,
    /// Apple Books audiobooks or ebooks.
    Book,
}

impl fmt::Display for MediaDomain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Music => write!(f, "music"),
            Self::Tv => write!(f, "tv"),
            Self::Podcast => write!(f, "podcast"),
            Self::Book => write!(f, "book"),
        }
    }
}

/// Providers mapped to Apple's consumer services used by the intents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Provider {
    /// Apple Music / MusicKit.
    MusicKit,
    /// Apple TV+.
    AppleTvPlus,
    /// Apple Podcasts.
    ApplePodcasts,
    /// Apple Books.
    AppleBooks,
}

impl Provider {
    /// Return a deterministic scheme prefix used for deeplink validation.
    #[must_use]
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::MusicKit => "musickit",
            Self::AppleTvPlus => "appletv",
            Self::ApplePodcasts => "podcasts",
            Self::AppleBooks => "books",
        }
    }
}

impl fmt::Display for Provider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MusicKit => write!(f, "MusicKit"),
            Self::AppleTvPlus => write!(f, "TV+"),
            Self::ApplePodcasts => write!(f, "Podcasts"),
            Self::AppleBooks => write!(f, "Books"),
        }
    }
}

/// Distinct dayparts to model energy and cadence expectations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Daypart {
    /// 6am – 12pm local time.
    Morning,
    /// 12pm – 6pm.
    Afternoon,
    /// 6pm – 10pm.
    Evening,
    /// 10pm onwards.
    LateNight,
}

impl fmt::Display for Daypart {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Morning => write!(f, "morning"),
            Self::Afternoon => write!(f, "afternoon"),
            Self::Evening => write!(f, "evening"),
            Self::LateNight => write!(f, "late night"),
        }
    }
}

/// A validated deeplink for a media item.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Deeplink {
    url: String,
}

impl Deeplink {
    /// Construct a deeplink and validate the provider scheme.
    pub fn new(provider: Provider, url: impl Into<String>) -> Result<Self> {
        let url = url.into();
        let scheme_prefix = format!("{}://", provider.scheme());
        if !url.starts_with(&scheme_prefix) {
            anyhow::bail!(
                "deeplink `{url}` must start with scheme `{scheme}`",
                scheme = scheme_prefix
            );
        }
        Ok(Self { url })
    }

    /// Expose the underlying URL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.url
    }
}

/// Media metadata used by the `PlaySomething` flow.
#[derive(Debug, Clone)]
pub struct MediaItem {
    id: &'static str,
    title: &'static str,
    subtitle: &'static str,
    domain: MediaDomain,
    provider: Provider,
    duration: Duration,
    cadence_hint: Duration,
    available_dayparts: HashSet<Daypart>,
    weeknight_friendly: bool,
    tags: HashSet<&'static str>,
    deeplink: Deeplink,
}

impl MediaItem {
    /// Convenience constructor used by the static catalog.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        id: &'static str,
        title: &'static str,
        subtitle: &'static str,
        domain: MediaDomain,
        provider: Provider,
        duration: Duration,
        cadence_hint: Duration,
        available_dayparts: impl IntoIterator<Item = Daypart>,
        weeknight_friendly: bool,
        tags: impl IntoIterator<Item = &'static str>,
        deeplink: Deeplink,
    ) -> Self {
        Self {
            id,
            title,
            subtitle,
            domain,
            provider,
            duration,
            cadence_hint,
            available_dayparts: available_dayparts.into_iter().collect(),
            weeknight_friendly,
            tags: tags.into_iter().collect(),
            deeplink,
        }
    }

    /// Unique identifier for the media item.
    #[must_use]
    pub fn id(&self) -> &'static str {
        self.id
    }

    /// Media title.
    #[must_use]
    pub fn title(&self) -> &'static str {
        self.title
    }

    /// Additional descriptive copy.
    #[must_use]
    pub fn subtitle(&self) -> &'static str {
        self.subtitle
    }

    /// Domain classification.
    #[must_use]
    pub fn domain(&self) -> MediaDomain {
        self.domain
    }

    /// Provider powering the experience.
    #[must_use]
    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// Typical runtime of the item.
    #[must_use]
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Cadence hint describing how frequently similar content lands.
    #[must_use]
    pub fn cadence_hint(&self) -> Duration {
        self.cadence_hint
    }

    /// Determine whether the item plays well on weeknights.
    #[must_use]
    pub fn weeknight_friendly(&self) -> bool {
        self.weeknight_friendly
    }

    /// Tags describing tone or context.
    #[must_use]
    pub fn tags(&self) -> &HashSet<&'static str> {
        &self.tags
    }

    /// Determine if the item fits a given daypart.
    #[must_use]
    pub fn supports_daypart(&self, daypart: Daypart) -> bool {
        self.available_dayparts.contains(&daypart)
    }

    /// Access the deeplink backing the media item.
    #[must_use]
    pub fn deeplink(&self) -> &Deeplink {
        &self.deeplink
    }
}

/// Query describing contextual filters for media selection.
#[derive(Debug, Clone)]
pub struct MediaPickQuery {
    domains: Option<HashSet<MediaDomain>>,
    min_duration: Option<Duration>,
    max_duration: Option<Duration>,
    dayparts: Option<HashSet<Daypart>>,
    weeknights_only: bool,
    preferred_tags: HashSet<&'static str>,
    limit: usize,
}

impl MediaPickQuery {
    /// Builder entry point.
    #[must_use]
    pub fn builder() -> MediaPickQueryBuilder {
        MediaPickQueryBuilder::default()
    }

    fn matches(&self, item: &MediaItem) -> bool {
        if let Some(domains) = &self.domains {
            if !domains.contains(&item.domain()) {
                return false;
            }
        }

        if let Some(min_duration) = self.min_duration {
            if item.duration() < min_duration {
                return false;
            }
        }

        if let Some(max_duration) = self.max_duration {
            if item.duration() > max_duration {
                return false;
            }
        }

        if let Some(dayparts) = &self.dayparts {
            if !dayparts
                .iter()
                .any(|daypart| item.supports_daypart(*daypart))
            {
                return false;
            }
        }

        if self.weeknights_only && !item.weeknight_friendly() {
            return false;
        }

        if !self.preferred_tags.is_empty()
            && self
                .preferred_tags
                .iter()
                .all(|tag| !item.tags().contains(tag))
        {
            return false;
        }

        true
    }

    fn limit(&self) -> usize {
        self.limit
    }
}

/// Builder for [`MediaPickQuery`].
#[derive(Debug, Default)]
pub struct MediaPickQueryBuilder {
    domains: Option<HashSet<MediaDomain>>,
    min_duration: Option<Duration>,
    max_duration: Option<Duration>,
    dayparts: Option<HashSet<Daypart>>,
    weeknights_only: bool,
    preferred_tags: HashSet<&'static str>,
    limit: usize,
}

impl MediaPickQueryBuilder {
    /// Restrict results to the specified media domains.
    #[must_use]
    pub fn domains(mut self, domains: impl IntoIterator<Item = MediaDomain>) -> Self {
        self.domains = Some(domains.into_iter().collect());
        self
    }

    /// Set the allowed duration range.
    #[must_use]
    pub fn duration_range(mut self, min: Option<Duration>, max: Option<Duration>) -> Self {
        self.min_duration = min;
        self.max_duration = max;
        self
    }

    /// Constrain results to specific dayparts.
    #[must_use]
    pub fn dayparts(mut self, dayparts: impl IntoIterator<Item = Daypart>) -> Self {
        let set: HashSet<Daypart> = dayparts.into_iter().collect();
        self.dayparts = if set.is_empty() { None } else { Some(set) };
        self
    }

    /// Prefer items tagged with any of the supplied descriptors.
    #[must_use]
    pub fn preferred_tags(mut self, tags: impl IntoIterator<Item = &'static str>) -> Self {
        self.preferred_tags = tags.into_iter().collect();
        self
    }

    /// Limit results to weeknight-friendly content.
    #[must_use]
    pub fn weeknights_only(mut self, weeknights_only: bool) -> Self {
        self.weeknights_only = weeknights_only;
        self
    }

    /// Cap the number of results returned by the query.
    #[must_use]
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit;
        self
    }

    /// Finalise query construction.
    #[must_use]
    pub fn build(self) -> MediaPickQuery {
        MediaPickQuery {
            domains: self.domains,
            min_duration: self.min_duration,
            max_duration: self.max_duration,
            dayparts: self.dayparts,
            weeknights_only: self.weeknights_only,
            preferred_tags: self.preferred_tags,
            limit: if self.limit == 0 { 5 } else { self.limit },
        }
    }
}

/// Lightweight permission store controlling provider access.
#[derive(Debug, Clone, Default)]
pub struct PermissionStore {
    granted: HashSet<Provider>,
}

impl PermissionStore {
    /// Grant playback rights for the provider.
    pub fn grant(&mut self, provider: Provider) {
        self.granted.insert(provider);
    }

    /// Revoke playback rights for the provider.
    pub fn revoke(&mut self, provider: Provider) {
        self.granted.remove(&provider);
    }

    /// Determine whether the provider has been granted.
    #[must_use]
    pub fn is_granted(&self, provider: Provider) -> bool {
        self.granted.contains(&provider)
    }
}

/// User interaction tracker capturing cadence and preferred runtimes.
#[derive(Debug, Clone, Default)]
pub struct UserPreferences {
    interactions: usize,
    average_duration_secs: Option<f64>,
    average_cadence_secs: Option<f64>,
}

impl UserPreferences {
    /// Record a completed interaction and refresh the aggregates.
    pub fn record_interaction(
        &mut self,
        cadence_since_last: Option<Duration>,
        consumed_duration: Duration,
    ) {
        self.interactions += 1;
        let count = self.interactions as f64;
        let consumed_secs = consumed_duration.as_secs_f64();
        self.average_duration_secs = Some(match self.average_duration_secs {
            Some(avg) => ((avg * (count - 1.0)) + consumed_secs) / count,
            None => consumed_secs,
        });
        if let Some(cadence) = cadence_since_last {
            let cadence_secs = cadence.as_secs_f64();
            self.average_cadence_secs = Some(match self.average_cadence_secs {
                Some(avg) => ((avg * (count - 1.0)) + cadence_secs) / count,
                None => cadence_secs,
            });
        }
    }

    fn duration_alignment(&self, item: &MediaItem) -> f64 {
        match self.average_duration_secs {
            Some(target) => {
                let diff = (target - item.duration().as_secs_f64()).abs();
                1.0 / (1.0 + diff / 600.0)
            }
            None => 0.5,
        }
    }

    fn cadence_alignment(&self, item: &MediaItem) -> f64 {
        match self.average_cadence_secs {
            Some(target) => {
                let diff = (target - item.cadence_hint().as_secs_f64()).abs();
                1.0 / (1.0 + diff / 1800.0)
            }
            None => 0.25,
        }
    }
}

/// Representation of a media recommendation.
#[derive(Debug, Clone)]
pub struct MediaSuggestion {
    /// Selected media item.
    pub item: MediaItem,
    /// Score used for deterministic sorting.
    pub score: f64,
    /// Narrative rationale describing why the item surfaced.
    pub rationale: Vec<String>,
}

/// Errors produced by [`PlaySomethingIntent`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SuggestionError {
    /// No items matched the supplied filters.
    #[error("no media matched the requested filters")]
    NoMatchingMedia,
    /// Content matched but requires a missing permission.
    #[error("permission missing for provider {provider}")]
    PermissionDenied {
        /// Provider lacking permission.
        provider: Provider,
    },
}

/// Backing media catalog.
#[derive(Debug, Clone)]
pub struct MediaLibrary {
    items: Vec<MediaItem>,
}

impl MediaLibrary {
    /// Return a curated catalog spanning all four domains.
    #[must_use]
    pub fn curated() -> Self {
        let mut items = Vec::new();

        let midnight_cities = MediaItem::new(
            "musickit-midnight-cities",
            "Midnight Cities",
            "Synthwave mix for focused evenings",
            MediaDomain::Music,
            Provider::MusicKit,
            Duration::from_secs(18 * 60),
            Duration::from_secs(48 * 60 * 60),
            [Daypart::Evening, Daypart::LateNight],
            true,
            ["focus", "instrumental"],
            Deeplink::new(Provider::MusicKit, "musickit://playlist/midnight-cities").unwrap(),
        );

        let apple_tv_episode = MediaItem::new(
            "appletv-tranquil",
            "The Tranquil Paradox",
            "Season 1, Episode 4",
            MediaDomain::Tv,
            Provider::AppleTvPlus,
            Duration::from_secs(28 * 60),
            Duration::from_secs(7 * 24 * 60 * 60),
            [Daypart::Evening],
            true,
            ["mystery", "serialized"],
            Deeplink::new(Provider::AppleTvPlus, "appletv://show/tranquil/episode/4").unwrap(),
        );

        let podcast_episode = MediaItem::new(
            "podcasts-maker-habits",
            "Maker Habits: Field Notes",
            "Episode 92",
            MediaDomain::Podcast,
            Provider::ApplePodcasts,
            Duration::from_secs(24 * 60),
            Duration::from_secs(3 * 24 * 60 * 60),
            [Daypart::Morning, Daypart::Afternoon],
            true,
            ["productivity", "interview"],
            Deeplink::new(
                Provider::ApplePodcasts,
                "podcasts://show/maker-habits/episode/92",
            )
            .unwrap(),
        );

        let book_excerpt = MediaItem::new(
            "books-astro-journal",
            "Astro Journal",
            "Evening reflection excerpt",
            MediaDomain::Book,
            Provider::AppleBooks,
            Duration::from_secs(35 * 60),
            Duration::from_secs(2 * 24 * 60 * 60),
            [Daypart::Evening, Daypart::LateNight],
            true,
            ["mindfulness", "shortform"],
            Deeplink::new(
                Provider::AppleBooks,
                "books://audiobook/astro-journal/excerpt",
            )
            .unwrap(),
        );

        let sunday_series = MediaItem::new(
            "appletv-sunday-special",
            "Sunday Special",
            "Docuseries episode",
            MediaDomain::Tv,
            Provider::AppleTvPlus,
            Duration::from_secs(52 * 60),
            Duration::from_secs(14 * 24 * 60 * 60),
            [Daypart::Afternoon],
            false,
            ["documentary"],
            Deeplink::new(
                Provider::AppleTvPlus,
                "appletv://show/sunday-special/episode/1",
            )
            .unwrap(),
        );

        items.extend([
            midnight_cities,
            apple_tv_episode,
            podcast_episode,
            book_excerpt,
            sunday_series,
        ]);

        Self { items }
    }

    fn items(&self) -> impl Iterator<Item = &MediaItem> {
        self.items.iter()
    }
}

/// Intent delivering cross-domain entertainment suggestions.
#[derive(Debug, Clone)]
pub struct PlaySomethingIntent {
    library: MediaLibrary,
    permissions: PermissionStore,
    preferences: UserPreferences,
}

impl PlaySomethingIntent {
    /// Create an intent bound to the provided library and permission store.
    #[must_use]
    pub fn new(
        library: MediaLibrary,
        permissions: PermissionStore,
        preferences: UserPreferences,
    ) -> Self {
        Self {
            library,
            permissions,
            preferences,
        }
    }

    /// Retrieve the current user preferences.
    #[must_use]
    pub fn preferences(&self) -> &UserPreferences {
        &self.preferences
    }

    /// Record an interaction for subsequent recommendations.
    pub fn record_interaction(
        &mut self,
        cadence_since_last: Option<Duration>,
        consumed_duration: Duration,
    ) {
        self.preferences
            .record_interaction(cadence_since_last, consumed_duration);
    }

    /// Resolve a recommendation using the supplied contextual filters.
    pub fn recommend(&self, query: &MediaPickQuery) -> Result<MediaSuggestion, SuggestionError> {
        let mut blocked_provider: Option<Provider> = None;
        let mut scored: Vec<(f64, &MediaItem, Vec<String>)> = self
            .library
            .items()
            .filter(|item| query.matches(item))
            .filter_map(|item| {
                if !self.permissions.is_granted(item.provider()) {
                    blocked_provider = Some(item.provider());
                    return None;
                }
                let mut rationale = Vec::new();
                if query.weeknights_only {
                    rationale.push("Optimised for weeknight pacing".to_string());
                }
                if let Some(dayparts) = &query.dayparts {
                    let joined = dayparts
                        .iter()
                        .map(|d| d.to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    rationale.push(format!("Matches preferred dayparts: {joined}"));
                }
                let duration_alignment = self.preferences.duration_alignment(item);
                let cadence_alignment = self.preferences.cadence_alignment(item);
                rationale.push(format!(
                    "Duration alignment score {:.2}",
                    duration_alignment
                ));
                rationale.push(format!("Cadence alignment score {:.2}", cadence_alignment));
                let base = 1.0;
                let score = base + duration_alignment + cadence_alignment;
                Some((score, item, rationale))
            })
            .collect();

        if scored.is_empty() {
            if let Some(provider) = blocked_provider {
                return Err(SuggestionError::PermissionDenied { provider });
            }
            return Err(SuggestionError::NoMatchingMedia);
        }

        scored.sort_by(|(a_score, _, _), (b_score, _, _)| {
            b_score.partial_cmp(a_score).unwrap_or(Ordering::Equal)
        });
        let limit = query.limit();
        let (score, item, rationale) = scored
            .into_iter()
            .take(limit)
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(Ordering::Equal))
            .expect("at least one element present after filtering");

        Ok(MediaSuggestion {
            item: item.clone(),
            score,
            rationale,
        })
    }
}

/// Finance account categories used by the mock dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FinanceAccountKind {
    /// Cash accounts such as checking.
    Cash,
    /// Investment portfolios.
    Brokerage,
    /// Subscription budgets tracked locally.
    Subscription,
}

impl fmt::Display for FinanceAccountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cash => write!(f, "cash"),
            Self::Brokerage => write!(f, "brokerage"),
            Self::Subscription => write!(f, "subscription"),
        }
    }
}

/// High-level snapshot totals for a period.
#[derive(Debug, Clone, PartialEq)]
pub struct PeriodSummary {
    /// Net change across all accounts.
    pub net_change: f64,
    /// Total inflows during the period.
    pub inflow: f64,
    /// Total outflows during the period.
    pub outflow: f64,
}

impl PeriodSummary {
    fn new(net_change: f64, inflow: f64, outflow: f64) -> Self {
        Self {
            net_change,
            inflow,
            outflow,
        }
    }
}

/// Subscription line item used by the audit.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionAuditLine {
    /// Human readable label.
    pub name: &'static str,
    /// Monthly price in the user's currency.
    pub monthly_cost: f64,
    /// Indicates whether the subscription renews within the next week.
    pub renews_within_week: bool,
    /// Deeplink to manage the subscription locally.
    pub manage_deeplink: &'static str,
}

/// Consolidated finance snapshot payload.
#[derive(Debug, Clone, PartialEq)]
pub struct FinanceSnapshot {
    /// Daily performance summary.
    pub daily: PeriodSummary,
    /// Weekly performance summary.
    pub weekly: PeriodSummary,
    /// Local subscription audit.
    pub subscriptions: Vec<SubscriptionAuditLine>,
}

/// Errors emitted by [`FinanceSnapshotIntent`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FinanceError {
    /// User denied finance permissions.
    #[error("finance data unavailable: permission denied")]
    PermissionDenied,
    /// Mock dataset failed to load.
    #[error("finance data unavailable: dataset missing")]
    DatasetMissing,
}

/// Finance permissions required before presenting data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinancePermissions {
    /// Whether local financial data may be used.
    pub allow_local_data: bool,
}

impl Default for FinancePermissions {
    fn default() -> Self {
        Self {
            allow_local_data: true,
        }
    }
}

/// Intent surface returning finance insights backed by mock data.
#[derive(Debug, Clone)]
pub struct FinanceSnapshotIntent {
    permissions: FinancePermissions,
    dataset: Option<FinanceDataset>,
}

impl FinanceSnapshotIntent {
    /// Instantiate the intent with permissions and an optional dataset.
    #[must_use]
    pub fn new(permissions: FinancePermissions, dataset: Option<FinanceDataset>) -> Self {
        Self {
            permissions,
            dataset,
        }
    }

    /// Produce the finance snapshot.
    pub fn snapshot(&self) -> Result<FinanceSnapshot, FinanceError> {
        if !self.permissions.allow_local_data {
            return Err(FinanceError::PermissionDenied);
        }
        let dataset = self.dataset.as_ref().ok_or(FinanceError::DatasetMissing)?;
        Ok(dataset.compile_snapshot())
    }
}

/// Mock finance dataset powering [`FinanceSnapshotIntent`].
#[derive(Debug, Clone)]
pub struct FinanceDataset {
    daily_inflow: f64,
    daily_outflow: f64,
    weekly_inflow: f64,
    weekly_outflow: f64,
    subscriptions: Vec<SubscriptionAuditLine>,
}

impl FinanceDataset {
    /// Deterministic dataset used by integration tests.
    #[must_use]
    pub fn mock() -> Self {
        Self {
            daily_inflow: 620.0,
            daily_outflow: 410.0,
            weekly_inflow: 3820.0,
            weekly_outflow: 2645.0,
            subscriptions: vec![
                SubscriptionAuditLine {
                    name: "Apple One Premier",
                    monthly_cost: 32.95,
                    renews_within_week: false,
                    manage_deeplink: "prefs:root=SUBSCRIPTIONS&name=appleone",
                },
                SubscriptionAuditLine {
                    name: "Creative Cloud",
                    monthly_cost: 59.99,
                    renews_within_week: true,
                    manage_deeplink: "prefs:root=SUBSCRIPTIONS&name=adobe",
                },
                SubscriptionAuditLine {
                    name: "Metropolitan Transit",
                    monthly_cost: 48.50,
                    renews_within_week: true,
                    manage_deeplink: "prefs:root=SUBSCRIPTIONS&name=transit",
                },
            ],
        }
    }

    fn compile_snapshot(&self) -> FinanceSnapshot {
        let daily_net = self.daily_inflow - self.daily_outflow;
        let weekly_net = self.weekly_inflow - self.weekly_outflow;
        FinanceSnapshot {
            daily: PeriodSummary::new(daily_net, self.daily_inflow, self.daily_outflow),
            weekly: PeriodSummary::new(weekly_net, self.weekly_inflow, self.weekly_outflow),
            subscriptions: self.subscriptions.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_permissions() -> PermissionStore {
        let mut permissions = PermissionStore::default();
        permissions.grant(Provider::MusicKit);
        permissions.grant(Provider::AppleTvPlus);
        permissions.grant(Provider::ApplePodcasts);
        permissions.grant(Provider::AppleBooks);
        permissions
    }

    #[test]
    fn media_pick_query_filters_by_context() {
        let library = MediaLibrary::curated();
        let query = MediaPickQuery::builder()
            .domains([MediaDomain::Tv])
            .duration_range(
                Some(Duration::from_secs(20 * 60)),
                Some(Duration::from_secs(30 * 60)),
            )
            .dayparts([Daypart::Evening])
            .weeknights_only(true)
            .build();
        let matches: Vec<_> = library
            .items()
            .filter(|item| query.matches(item))
            .map(|item| item.id())
            .collect();
        assert_eq!(matches, vec!["appletv-tranquil"]);
    }

    #[test]
    fn play_something_returns_top_match() {
        let library = MediaLibrary::curated();
        let permissions = base_permissions();
        let preferences = UserPreferences::default();
        let intent = PlaySomethingIntent::new(library.clone(), permissions, preferences);
        let query = MediaPickQuery::builder()
            .domains([MediaDomain::Music, MediaDomain::Podcast])
            .duration_range(None, Some(Duration::from_secs(25 * 60)))
            .dayparts([Daypart::Morning, Daypart::Evening])
            .weeknights_only(true)
            .preferred_tags(["productivity"])
            .limit(3)
            .build();

        let suggestion = intent.recommend(&query).expect("suggestion available");
        assert_eq!(suggestion.item.id(), "podcasts-maker-habits");
        assert!(suggestion
            .rationale
            .iter()
            .any(|line| line.contains("Duration alignment")));
        assert!(suggestion
            .item
            .deeplink()
            .as_str()
            .starts_with("podcasts://"));
    }

    #[test]
    fn play_something_surfaces_permission_error() {
        let library = MediaLibrary::curated();
        let mut permissions = base_permissions();
        permissions.revoke(Provider::AppleTvPlus);
        let preferences = UserPreferences::default();
        let intent = PlaySomethingIntent::new(library.clone(), permissions, preferences);
        let query = MediaPickQuery::builder()
            .domains([MediaDomain::Tv])
            .duration_range(None, Some(Duration::from_secs(60 * 60)))
            .dayparts([Daypart::Evening])
            .weeknights_only(true)
            .build();
        let err = intent.recommend(&query).unwrap_err();
        assert_eq!(
            err,
            SuggestionError::PermissionDenied {
                provider: Provider::AppleTvPlus
            }
        );
    }

    #[test]
    fn user_preferences_shift_recommendations_towards_longer_content() {
        let library = MediaLibrary::curated();
        let permissions = base_permissions();
        let mut preferences = UserPreferences::default();
        preferences.record_interaction(None, Duration::from_secs(52 * 60));
        preferences.record_interaction(
            Some(Duration::from_secs(48 * 60 * 60)),
            Duration::from_secs(52 * 60),
        );
        let intent = PlaySomethingIntent::new(library.clone(), permissions, preferences);
        let query = MediaPickQuery::builder()
            .domains([MediaDomain::Tv])
            .duration_range(None, Some(Duration::from_secs(60 * 60)))
            .dayparts([Daypart::Afternoon, Daypart::Evening])
            .weeknights_only(false)
            .build();
        let suggestion = intent.recommend(&query).expect("suggestion available");
        assert_eq!(suggestion.item.id(), "appletv-sunday-special");
    }

    #[test]
    fn finance_snapshot_compiles_daily_and_weekly_summary() {
        let permissions = FinancePermissions::default();
        let dataset = FinanceDataset::mock();
        let intent = FinanceSnapshotIntent::new(permissions, Some(dataset));
        let snapshot = intent.snapshot().expect("snapshot available");
        assert_eq!(snapshot.daily.net_change, 210.0);
        assert_eq!(snapshot.weekly.net_change, 1175.0);
        assert_eq!(snapshot.subscriptions.len(), 3);
        assert!(snapshot
            .subscriptions
            .iter()
            .any(|sub| sub.renews_within_week && sub.name == "Creative Cloud"));
    }

    #[test]
    fn finance_snapshot_respects_permissions() {
        let permissions = FinancePermissions {
            allow_local_data: false,
        };
        let dataset = FinanceDataset::mock();
        let intent = FinanceSnapshotIntent::new(permissions, Some(dataset));
        let err = intent.snapshot().unwrap_err();
        assert_eq!(err, FinanceError::PermissionDenied);
    }
}
