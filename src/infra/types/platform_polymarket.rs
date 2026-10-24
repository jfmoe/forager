//! Polymarket shapes: event and market refs and their URLs, search options, and item fields.
//! How a market's status and outcome prices are derived lives in `platform_polymarket_price`.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::platform::{Platform, PlatformRef, PlatformRefError};
pub(crate) use super::platform_polymarket_price::{
    BookTop, Price, QuoteSource, event_status, market_status, price_market,
};

/// The most events one search page may ask for.
pub(crate) const POLYMARKET_MAX_LIMIT: u16 = 20;
const REF_PREFIX: &str = "polymarket:";
const MARKET_SEGMENT: &str = "market/";
const SITE: &str = "https://polymarket.com";
const SITE_HOSTS: [&str; 2] = ["polymarket.com", "www.polymarket.com"];
const US_HOSTS: [&str; 3] = [
    "polymarket.us",
    "www.polymarket.us",
    "gateway.polymarket.us",
];
/// The language prefixes polymarket.com puts before a page path; English has none.
const LOCALES: [&str; 16] = [
    "bn", "de", "es", "fr", "hi", "id", "it", "ja", "pl", "ru", "th", "tl", "uk", "vi", "zh",
    "zh-hant",
];
const MAX_SLUG_LENGTH: usize = 255;

const REF_HINT: &str = "pass a `polymarket:<event_slug>` or `polymarket:market/<market_slug>` ref, or a polymarket.com URL such as https://polymarket.com/event/<slug>, https://polymarket.com/event/<event>/<market>, or https://polymarket.com/market/<slug>";
const US_HINT: &str = "Polymarket US (polymarket.us) is a separate product with its own events and markets; forager reads only polymarket.com";
const NUMERIC_HINT: &str = "a numeric ID has no polymarket.com page, so it is not a ref; pass the slug from a polymarket.com URL or from a forager search result";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// What a Polymarket ref names.
pub enum PolymarketKind {
    /// An event: one question or topic holding one or more markets.
    Event,
    /// One market of an event, which resolves on its own.
    Market,
}

impl PolymarketKind {
    /// Returns the stable kind identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Event => "event",
            Self::Market => "market",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
/// A Polymarket event or market, by the slug its polymarket.com page shows. Slugs are
/// case-sensitive and have no versions; Polymarket may change or reuse one, so programmatic
/// consumers key on the output IDs instead.
pub struct PolymarketRef {
    kind: PolymarketKind,
    slug: String,
}

impl PolymarketRef {
    /// Parses a `polymarket:` ref or a polymarket.com event or market URL.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformRefError`] for any other input, including numeric IDs and Polymarket US
    /// URLs.
    pub fn parse(input: &str) -> Result<Self, PlatformRefError> {
        let trimmed = input.trim();
        let parsed = match trimmed.get(..REF_PREFIX.len()) {
            Some(prefix) if prefix.eq_ignore_ascii_case(REF_PREFIX) => {
                parse_ref_body(&trimmed[REF_PREFIX.len()..])
            }
            _ => parse_url(trimmed),
        };
        parsed.map_err(|hint| PlatformRefError {
            platform: Platform::Polymarket,
            input: Some(input.to_owned()),
            hint: hint.into(),
        })
    }

    /// Returns the ref of an event slug, or `None` for a string that is no slug.
    #[must_use]
    pub fn event(slug: &str) -> Option<Self> {
        Self::new(PolymarketKind::Event, slug)
    }

    /// Returns the ref of a market slug, or `None` for a string that is no slug.
    #[must_use]
    pub fn market(slug: &str) -> Option<Self> {
        Self::new(PolymarketKind::Market, slug)
    }

    fn new(kind: PolymarketKind, slug: &str) -> Option<Self> {
        is_slug(slug).then(|| Self {
            kind,
            slug: slug.to_owned(),
        })
    }

    /// Returns what the ref names.
    #[must_use]
    pub const fn kind(&self) -> PolymarketKind {
        self.kind
    }

    /// Returns the slug.
    #[must_use]
    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// Returns the polymarket.com page; a market page redirects to its place in the event.
    #[must_use]
    pub fn canonical_url(&self) -> String {
        format!("{SITE}/{}/{}", self.kind.as_str(), self.slug)
    }
}

impl fmt::Display for PolymarketRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            PolymarketKind::Event => formatter.write_str(&self.slug),
            PolymarketKind::Market => write!(formatter, "{MARKET_SEGMENT}{}", self.slug),
        }
    }
}

/// A slug Gamma can look up: ASCII letters, digits, `-`, and `_`. A string of digits alone is a
/// numeric ID, not a slug. A few legacy slugs hold a `.`, which Gamma's slug lookup rejects.
fn is_slug(value: &str) -> bool {
    (1..=MAX_SLUG_LENGTH).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        && !value.bytes().all(|byte| byte.is_ascii_digit())
}

/// Parses what follows `polymarket:`: an event slug or `market/<slug>`, the kind segment in any
/// letter case.
fn parse_ref_body(body: &str) -> Result<PolymarketRef, &'static str> {
    if body.bytes().all(|byte| byte.is_ascii_digit()) && !body.is_empty() {
        return Err(NUMERIC_HINT);
    }
    let parsed = match body.get(..MARKET_SEGMENT.len()) {
        Some(segment) if segment.eq_ignore_ascii_case(MARKET_SEGMENT) => {
            PolymarketRef::market(&body[MARKET_SEGMENT.len()..])
        }
        _ => PolymarketRef::event(body),
    };
    parsed.ok_or(REF_HINT)
}

/// Parses a polymarket.com page URL over http or https, ignoring a language prefix, the query,
/// the fragment, and a trailing `/`. `/event/<e>/<m>` names market `<m>`: a market slug is
/// unique on its own.
fn parse_url(input: &str) -> Result<PolymarketRef, &'static str> {
    let (scheme, rest) = input.split_once("://").ok_or(REF_HINT)?;
    if !(scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")) {
        return Err(REF_HINT);
    }
    let rest = rest.split(['#', '?']).next().unwrap_or_default();
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let on = |hosts: &[&str]| hosts.iter().any(|known| host.eq_ignore_ascii_case(known));
    if on(&US_HOSTS) {
        return Err(US_HINT);
    }
    if !on(&SITE_HOSTS) {
        return Err(REF_HINT);
    }
    let mut segments = path.trim_end_matches('/').split('/').collect::<Vec<_>>();
    if segments
        .first()
        .is_some_and(|first| LOCALES.contains(first))
    {
        segments.remove(0);
    }
    let (kind, slug) = match segments.as_slice() {
        ["event", slug] => (PolymarketKind::Event, *slug),
        ["event", _, slug] | ["market", slug] => (PolymarketKind::Market, *slug),
        _ => return Err(REF_HINT),
    };
    if !slug.is_empty() && slug.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(NUMERIC_HINT);
    }
    match kind {
        PolymarketKind::Event => PolymarketRef::event(slug),
        PolymarketKind::Market => PolymarketRef::market(slug),
    }
    .ok_or(REF_HINT)
}

/// Optional Polymarket search settings. A cursor encodes them completely.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct PolymarketSearchOptions {
    /// Whether closed events match too; by default only events still open match.
    pub include_closed: bool,
}

impl PolymarketSearchOptions {
    pub(super) fn validate(query: &str, limit: u16) -> Result<(), String> {
        if !(1..=POLYMARKET_MAX_LIMIT).contains(&limit) {
            return Err(format!(
                "--limit must be between 1 and {POLYMARKET_MAX_LIMIT}"
            ));
        }
        if query.trim().is_empty() {
            return Err("polymarket search needs a query".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Whether a market trades, and whether it has resolved. An event is `resolved` when all its
/// markets are, `open` when any market is, and `closed` otherwise; it is never `suspended`.
pub enum PolymarketStatus {
    /// It accepts orders.
    Open,
    /// It is not closed but accepts no orders.
    Suspended,
    /// It stopped trading and awaits its resolution or a dispute.
    Closed,
    /// It resolved; its outcomes carry their payouts.
    Resolved,
}

impl PolymarketStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Suspended => "suspended",
            Self::Closed => "closed",
            Self::Resolved => "resolved",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Why an outcome has the price it has, or has none.
pub enum PolymarketBasis {
    /// The market resolved; the price is the outcome's payout, not a probability.
    Resolution,
    /// The midpoint of the live best bid and best ask.
    Midpoint,
    /// Gamma's cached midpoint, used when the live order book was unavailable; it may lag.
    MidpointCached,
    /// The order book lacks a bid or an ask, so there is no price.
    NoTwoSidedBook,
    /// The market is closed or suspended, so there is no price.
    NotTrading,
}

impl PolymarketBasis {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Resolution => "resolution",
            Self::Midpoint => "midpoint",
            Self::MidpointCached => "midpoint_cached",
            Self::NoTwoSidedBook => "no_two_sided_book",
            Self::NotTrading => "not_trading",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
/// Where the prices of a result come from.
pub enum PolymarketPriceSource {
    /// The live CLOB order books.
    Clob,
    /// Gamma, whose quotes are cached for up to five minutes.
    Gamma,
}

impl PolymarketPriceSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Clob => "clob",
            Self::Gamma => "gamma",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
/// One outcome of a market with its price and top of book.
pub struct PolymarketOutcome {
    pub name: String,
    /// The Market-Implied Probability, or the payout of a resolved market; `null` when `basis`
    /// says there is none.
    pub price: Option<f64>,
    pub basis: PolymarketBasis,
    pub best_bid: Option<f64>,
    pub best_ask: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
/// The fields of one market.
pub struct PolymarketMarketFields {
    pub market_id: String,
    /// The on-chain identity: the stable storage key of a market.
    pub condition_id: Option<String>,
    pub question: String,
    /// The short label of the market inside its event.
    pub label: String,
    pub status: PolymarketStatus,
    pub end_date: Option<String>,
    /// The Yes price of a Yes/No market; `null` for other outcome names.
    pub probability: Option<f64>,
    pub outcomes: Vec<PolymarketOutcome>,
    /// The first outcome's best ask minus its best bid.
    pub spread: Option<f64>,
    pub last_trade_price: Option<f64>,
    pub one_day_change: Option<f64>,
    pub one_week_change: Option<f64>,
    pub one_month_change: Option<f64>,
    pub volume: Option<f64>,
    pub liquidity: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
/// One market of a fetched event.
pub struct PolymarketMarket {
    #[serde(rename = "ref")]
    pub reference: PlatformRef,
    pub url: String,
    #[serde(flatten)]
    pub fields: PolymarketMarketFields,
    /// The market's resolution rules, at `abstract` depth, when they differ from the event's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rules: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
/// The fields every event result carries.
pub struct PolymarketEventFields {
    /// The stable storage key of an event.
    pub event_id: String,
    pub status: PolymarketStatus,
    pub end_date: Option<String>,
    pub volume: Option<f64>,
    pub volume_24h: Option<f64>,
    pub liquidity: Option<f64>,
    /// Whether exactly one market of the event resolves Yes; the probabilities still need not
    /// sum to 1.
    pub neg_risk: bool,
    pub market_count: usize,
    pub price_source: PolymarketPriceSource,
}

#[derive(Clone, Debug, Serialize)]
/// One market of a search result, summarized.
pub struct PolymarketTopMarket {
    #[serde(rename = "ref")]
    pub reference: PlatformRef,
    pub label: String,
    pub probability: Option<f64>,
    /// The basis of the first outcome's price.
    pub basis: PolymarketBasis,
    pub status: PolymarketStatus,
}

#[derive(Clone, Debug, Serialize)]
/// The event a fetched market belongs to.
pub struct PolymarketEventLink {
    #[serde(rename = "ref")]
    pub reference: PlatformRef,
    pub url: String,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
/// The Polymarket fields of an item.
pub enum PolymarketItemData {
    /// A search result: an event with its most probable markets, priced from Gamma.
    SearchEvent {
        #[serde(flatten)]
        event: PolymarketEventFields,
        top_markets: Vec<PolymarketTopMarket>,
    },
    /// A fetched event with every market.
    Event {
        #[serde(flatten)]
        event: PolymarketEventFields,
        /// When the prices were read: the CLOB response, or Gamma's last update.
        prices_as_of: Option<String>,
        markets: Vec<PolymarketMarket>,
        /// The event's resolution rules, at `abstract` depth.
        #[serde(skip_serializing_if = "Option::is_none")]
        rules: Option<String>,
    },
    /// A fetched market.
    Market {
        event_id: Option<String>,
        #[serde(flatten)]
        market: Box<PolymarketMarketFields>,
        neg_risk: bool,
        event: Option<PolymarketEventLink>,
        price_source: PolymarketPriceSource,
        prices_as_of: Option<String>,
        /// The market's resolution rules, at `abstract` depth.
        #[serde(skip_serializing_if = "Option::is_none")]
        rules: Option<String>,
    },
}

impl PolymarketItemData {
    /// Returns the resolution rules a fetch at `abstract` depth inlined: the event's or the
    /// market's own, then each market's that differ, under its label.
    #[must_use]
    pub fn rules_document(&self) -> Option<String> {
        match self {
            Self::SearchEvent { .. } => None,
            Self::Market { rules, .. } => rules.clone(),
            Self::Event { rules, markets, .. } => {
                let sections = markets.iter().filter_map(|market| {
                    let own = market.rules.as_ref()?;
                    Some(format!("\n\n## {}\n\n{own}", market.fields.label))
                });
                Some(std::iter::once(rules.clone()?).chain(sections).collect())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::types::{Platform, PlatformRef, PlatformSearchOptions, PlatformSearchRequest};

    use super::PolymarketSearchOptions;

    fn polymarket(input: &str) -> Option<String> {
        PlatformRef::parse(Platform::Polymarket, input)
            .ok()
            .map(|reference| reference.to_string())
    }

    #[test]
    fn event_forms_parse_to_the_event_ref() {
        let parsed = [
            "polymarket:how-many-fed-rate-cuts-in-2026",
            "Polymarket:how-many-fed-rate-cuts-in-2026",
            " polymarket:how-many-fed-rate-cuts-in-2026 ",
            "https://polymarket.com/event/how-many-fed-rate-cuts-in-2026",
            "http://polymarket.com/event/how-many-fed-rate-cuts-in-2026",
            "HTTPS://WWW.Polymarket.com/event/how-many-fed-rate-cuts-in-2026",
            "https://polymarket.com/event/how-many-fed-rate-cuts-in-2026/",
            "https://polymarket.com/zh/event/how-many-fed-rate-cuts-in-2026",
            "https://polymarket.com/zh-hant/event/how-many-fed-rate-cuts-in-2026",
            "https://polymarket.com/event/how-many-fed-rate-cuts-in-2026?tid=1761200000000",
            "https://polymarket.com/event/how-many-fed-rate-cuts-in-2026#comments",
        ]
        .map(polymarket);

        assert_eq!(
            parsed,
            [(); 11].map(|()| Some("polymarket:how-many-fed-rate-cuts-in-2026".to_owned()))
        );
    }

    #[test]
    fn market_forms_parse_to_the_market_ref() {
        let parsed = [
            "polymarket:market/will-no-fed-rate-cuts-happen-in-2026",
            "polymarket:MARKET/will-no-fed-rate-cuts-happen-in-2026",
            "https://polymarket.com/market/will-no-fed-rate-cuts-happen-in-2026",
            "https://polymarket.com/event/how-many-fed-rate-cuts-in-2026/will-no-fed-rate-cuts-happen-in-2026",
            "https://polymarket.com/de/event/how-many-fed-rate-cuts-in-2026/will-no-fed-rate-cuts-happen-in-2026?tid=1",
            "https://www.polymarket.com/ja/market/will-no-fed-rate-cuts-happen-in-2026/",
        ]
        .map(polymarket);

        assert_eq!(
            parsed,
            [(); 6].map(|()| Some(
                "polymarket:market/will-no-fed-rate-cuts-happen-in-2026".to_owned()
            ))
        );
    }

    #[test]
    fn slugs_keep_their_case_and_underscores_but_not_dots() {
        let parsed = [
            "polymarket:will-whale_alert-tweet-count-be-over-61300-on-january-31-10-am-et",
            "polymarket:market/MixedCase-Slug",
            "polymarket:will-joe-bidens-approval-rating-be-49.0-or-higher-on-september-1",
        ]
        .map(polymarket);

        assert_eq!(
            parsed,
            [
                Some(
                    "polymarket:will-whale_alert-tweet-count-be-over-61300-on-january-31-10-am-et"
                        .to_owned()
                ),
                Some("polymarket:market/MixedCase-Slug".to_owned()),
                None,
            ]
        );
    }

    #[test]
    fn numeric_ids_other_paths_lookalike_hosts_and_bad_slugs_are_rejected() {
        let parsed = [
            "polymarket:51456",
            "polymarket:market/616902",
            "https://polymarket.com/event/51456",
            "https://polymarket.com/sports/nfl/games",
            "https://polymarket.com/profile/0xabc",
            "https://polymarket.com/event",
            "https://polymarket.com/en/event/how-many-fed-rate-cuts-in-2026",
            "https://polymarket.com/event/a/b/c",
            "https://polymarket.com.evil.example/event/how-many-fed-rate-cuts-in-2026",
            "https://gamma-api.polymarket.com/events/slug/how-many-fed-rate-cuts-in-2026",
            "ftp://polymarket.com/event/how-many-fed-rate-cuts-in-2026",
            "how-many-fed-rate-cuts-in-2026",
            "polymarket:",
            "polymarket:market/",
            "polymarket:has space",
            "polymarket:caf\u{e9}",
            "polymarket:event/how-many-fed-rate-cuts-in-2026",
        ]
        .map(polymarket);

        assert_eq!(parsed, [const { None }; 17]);
    }

    #[test]
    fn polymarket_us_and_numeric_ids_explain_why_they_are_not_refs() {
        let message = |input: &str| {
            PlatformRef::parse(Platform::Polymarket, input)
                .unwrap_err()
                .to_string()
        };

        assert_eq!(
            [
                message("https://polymarket.us/event/fedfnds-ondt-2027-12-31"),
                message("https://gateway.polymarket.us/v1/events/1"),
                message("polymarket:51456"),
                message("https://polymarket.com/sports/nfl"),
            ],
            [
                "unrecognized polymarket reference `https://polymarket.us/event/fedfnds-ondt-2027-12-31`; Polymarket US (polymarket.us) is a separate product with its own events and markets; forager reads only polymarket.com".to_owned(),
                "unrecognized polymarket reference `https://gateway.polymarket.us/v1/events/1`; Polymarket US (polymarket.us) is a separate product with its own events and markets; forager reads only polymarket.com".to_owned(),
                "unrecognized polymarket reference `polymarket:51456`; a numeric ID has no polymarket.com page, so it is not a ref; pass the slug from a polymarket.com URL or from a forager search result".to_owned(),
                "unrecognized polymarket reference `https://polymarket.com/sports/nfl`; pass a `polymarket:<event_slug>` or `polymarket:market/<market_slug>` ref, or a polymarket.com URL such as https://polymarket.com/event/<slug>, https://polymarket.com/event/<event>/<market>, or https://polymarket.com/market/<slug>".to_owned(),
            ]
        );
    }

    #[test]
    fn canonical_urls_round_trip_to_the_ref() {
        let round_trips = [
            "polymarket:how-many-fed-rate-cuts-in-2026",
            "polymarket:market/will-no-fed-rate-cuts-happen-in-2026",
        ]
        .map(|input| {
            let reference = PlatformRef::parse(Platform::Polymarket, input).expect("ref");
            let url = reference.canonical_url();
            let round_trip = PlatformRef::parse(Platform::Polymarket, &url).ok();
            (url, reference.kind(), round_trip == Some(reference))
        });

        assert_eq!(
            round_trips,
            [
                (
                    "https://polymarket.com/event/how-many-fed-rate-cuts-in-2026".to_owned(),
                    "event",
                    true
                ),
                (
                    "https://polymarket.com/market/will-no-fed-rate-cuts-happen-in-2026".to_owned(),
                    "market",
                    true
                ),
            ]
        );
    }

    #[test]
    fn an_empty_query_or_an_out_of_range_limit_is_rejected() {
        let request = |query: &str, limit| PlatformSearchRequest {
            query: query.into(),
            limit,
            options: PlatformSearchOptions::Polymarket(PolymarketSearchOptions::default()),
            page: None,
        };

        let results = [request(" ", 10), request("fed", 0), request("fed", 21)]
            .map(|request| request.validate().err());

        assert_eq!(
            results,
            [
                Some("polymarket search needs a query".into()),
                Some("--limit must be between 1 and 20".into()),
                Some("--limit must be between 1 and 20".into()),
            ]
        );
    }
}
