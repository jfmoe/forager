//! Gamma records and how they become Polymarket items: the event and market shapes Gamma
//! returned on 2026-10-24, the JSON-encoded string fields inside them, and the item fields built
//! from them once the quotes are known.

use std::collections::HashMap;

use serde::Deserialize;

use crate::providers::shared::utc_timestamp;
use crate::types::{
    BookTop, ContentDepth, PlatformItem, PlatformItemData, PlatformRef, PolymarketEventFields,
    PolymarketEventLink, PolymarketItemData, PolymarketMarket, PolymarketMarketFields,
    PolymarketPriceSource, PolymarketRef, PolymarketStatus, PolymarketTopMarket, Price,
    QuoteSource, event_status, market_status, price_market,
};

/// How many markets a search result summarizes.
const TOP_MARKETS: usize = 5;

/// A number Gamma writes as a JSON number in one place and as a decimal string in another.
#[derive(Deserialize)]
#[serde(untagged)]
pub(super) enum Number {
    Value(f64),
    Text(String),
}

impl Number {
    pub(super) fn value(&self) -> Option<f64> {
        match self {
            Self::Value(value) => Some(*value),
            Self::Text(text) => text.trim().parse().ok(),
        }
    }

    pub(super) fn price(&self) -> Option<Price> {
        match self {
            Self::Value(value) => Price::from_f64(*value),
            Self::Text(text) => Price::parse(text),
        }
    }
}

/// An ID Gamma writes as a string or a number.
#[derive(Deserialize)]
#[serde(untagged)]
enum Id {
    Text(String),
    Value(u64),
}

impl Id {
    fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Value(value) => value.to_string(),
        }
    }
}

/// One Gamma event; every field Gamma may omit is optional.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GammaEvent {
    id: Id,
    pub(super) slug: Option<String>,
    title: Option<String>,
    description: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    volume: Option<Number>,
    #[serde(rename = "volume24hr")]
    volume_24h: Option<Number>,
    liquidity: Option<Number>,
    neg_risk: Option<bool>,
    updated_at: Option<String>,
    pub(super) markets: Option<Vec<GammaMarket>>,
}

/// One Gamma market. `outcomes`, `outcomePrices`, and `clobTokenIds` are JSON arrays encoded as
/// strings; `positionIds` is a real array.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GammaMarket {
    id: Id,
    pub(super) slug: Option<String>,
    question: Option<String>,
    group_item_title: Option<String>,
    condition_id: Option<String>,
    outcomes: Option<String>,
    outcome_prices: Option<String>,
    clob_token_ids: Option<String>,
    position_ids: Option<Vec<String>>,
    version: Option<String>,
    best_bid: Option<Number>,
    best_ask: Option<Number>,
    last_trade_price: Option<Number>,
    one_day_price_change: Option<Number>,
    one_week_price_change: Option<Number>,
    one_month_price_change: Option<Number>,
    volume: Option<Number>,
    volume_num: Option<Number>,
    liquidity: Option<Number>,
    liquidity_num: Option<Number>,
    start_date: Option<String>,
    end_date: Option<String>,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    accepting_orders: bool,
    uma_resolution_status: Option<String>,
    neg_risk: Option<bool>,
    description: Option<String>,
    updated_at: Option<String>,
    /// The events a market belongs to; only the market slug lookup embeds them.
    events: Option<Vec<GammaEventLink>>,
}

#[derive(Deserialize)]
pub(super) struct GammaEventLink {
    id: Option<Id>,
    slug: Option<String>,
    title: Option<String>,
}

/// A Gamma market decoded into what pricing and output need.
pub(super) struct Market {
    pub(super) reference: PolymarketRef,
    pub(super) status: PolymarketStatus,
    names: Vec<String>,
    settled: Vec<Price>,
    /// The CLOB token of each outcome, in outcome order.
    pub(super) tokens: Vec<String>,
    gamma_top: BookTop,
    gamma_last_trade: Option<f64>,
    id: String,
    condition_id: Option<String>,
    question: String,
    label: String,
    end_date: Option<String>,
    one_day_change: Option<f64>,
    one_week_change: Option<f64>,
    one_month_change: Option<f64>,
    volume: Option<f64>,
    liquidity: Option<f64>,
    neg_risk: bool,
    start_date: Option<String>,
    description: String,
    updated_at: Option<String>,
    events: Vec<GammaEventLink>,
}

/// The quotes one market is priced from: the live books of its outcomes, or Gamma's cache.
pub(super) enum MarketQuotes {
    Live {
        tops: Vec<Option<BookTop>>,
        last_trade: Option<f64>,
    },
    Cached,
}

impl Market {
    /// Decodes a Gamma market. Its JSON-encoded fields must decode, and an open market, whose
    /// books the CLOB serves, must have one token per outcome.
    pub(super) fn decode(market: GammaMarket) -> Result<Self, String> {
        let slug = market.slug.unwrap_or_default();
        let reference = PolymarketRef::market(&slug).ok_or_else(|| {
            format!("Polymarket Gamma API returned an invalid market slug `{slug}`")
        })?;
        let names = decode_list::<String>(market.outcomes.as_deref(), "outcomes", &reference)?;
        let settled = decode_list::<String>(
            market.outcome_prices.as_deref(),
            "outcomePrices",
            &reference,
        )?
        .iter()
        .map(|price| Price::parse(price))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| {
            format!(
                "Polymarket Gamma API returned polymarket:{reference} with an invalid outcome price"
            )
        })?;
        let tokens = if market.version.as_deref() == Some("v2") {
            market.position_ids.unwrap_or_default()
        } else {
            decode_list::<String>(market.clob_token_ids.as_deref(), "clobTokenIds", &reference)?
        };
        let resolved = market.uma_resolution_status.as_deref() == Some("resolved");
        let status = market_status(market.closed, resolved, market.accepting_orders);
        if status == PolymarketStatus::Open && tokens.len() != names.len() {
            return Err(format!(
                "Polymarket Gamma API returned polymarket:{reference} with {} outcomes but {} tokens",
                names.len(),
                tokens.len()
            ));
        }
        let question = market.question.unwrap_or_default();
        Ok(Self {
            status,
            settled,
            tokens,
            gamma_top: BookTop::new(
                market.best_bid.as_ref().and_then(Number::price),
                market.best_ask.as_ref().and_then(Number::price),
            ),
            gamma_last_trade: market.last_trade_price.as_ref().and_then(Number::value),
            id: market.id.text(),
            condition_id: market.condition_id,
            label: market
                .group_item_title
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| question.clone()),
            question,
            end_date: market.end_date.as_deref().and_then(utc_timestamp),
            one_day_change: market.one_day_price_change.as_ref().and_then(Number::value),
            one_week_change: market
                .one_week_price_change
                .as_ref()
                .and_then(Number::value),
            one_month_change: market
                .one_month_price_change
                .as_ref()
                .and_then(Number::value),
            volume: market
                .volume_num
                .or(market.volume)
                .as_ref()
                .and_then(Number::value),
            liquidity: market
                .liquidity_num
                .or(market.liquidity)
                .as_ref()
                .and_then(Number::value),
            neg_risk: market.neg_risk.unwrap_or_default(),
            start_date: market.start_date.as_deref().and_then(utc_timestamp),
            description: market.description.unwrap_or_default(),
            updated_at: market.updated_at.as_deref().and_then(utc_timestamp),
            events: market.events.unwrap_or_default(),
            names,
            reference,
        })
    }

    /// Prices the market and returns its fields. A resolved, closed, or suspended market never
    /// reads quotes; an open one reads the live books, or Gamma's cache when they failed.
    fn fields(&self, quotes: &MarketQuotes) -> PolymarketMarketFields {
        let (source, last_trade_price) = match quotes {
            MarketQuotes::Live { tops, last_trade } => {
                (QuoteSource::Clob(tops.clone()), *last_trade)
            }
            MarketQuotes::Cached => (
                QuoteSource::Gamma(self.gamma_top),
                self.gamma_last_trade
                    .filter(|_| self.status == PolymarketStatus::Open),
            ),
        };
        let pricing = price_market(self.status, &self.names, &self.settled, &source);
        PolymarketMarketFields {
            market_id: self.id.clone(),
            condition_id: self.condition_id.clone(),
            question: self.question.clone(),
            label: self.label.clone(),
            status: self.status,
            end_date: self.end_date.clone(),
            probability: pricing.probability,
            outcomes: pricing.outcomes,
            spread: pricing.spread,
            last_trade_price,
            one_day_change: self.one_day_change,
            one_week_change: self.one_week_change,
            one_month_change: self.one_month_change,
            volume: self.volume,
            liquidity: self.liquidity,
        }
    }

    fn summary(&self) -> PolymarketTopMarket {
        let fields = self.fields(&MarketQuotes::Cached);
        PolymarketTopMarket {
            reference: PlatformRef::Polymarket(self.reference.clone()),
            label: self.label.clone(),
            probability: fields.probability,
            basis: fields
                .outcomes
                .first()
                .map_or(crate::types::PolymarketBasis::NoTwoSidedBook, |outcome| {
                    outcome.basis
                }),
            status: self.status,
        }
    }

    /// The market's rules, empty when Gamma has none.
    pub(super) fn rules(&self) -> &str {
        self.description.trim()
    }

    /// Builds the item of a fetched market.
    pub(super) fn into_item(
        self,
        depth: ContentDepth,
        quotes: &MarketQuotes,
        price_source: PolymarketPriceSource,
        prices_as_of: Option<String>,
    ) -> PlatformItem {
        let fields = self.fields(quotes);
        let parent = self.events.first();
        let event = parent.and_then(|parent| {
            let reference = PolymarketRef::event(parent.slug.as_deref()?)?;
            Some(PolymarketEventLink {
                url: reference.canonical_url(),
                reference: PlatformRef::Polymarket(reference),
                title: parent.title.clone(),
            })
        });
        let rules = (depth == ContentDepth::Abstract).then(|| self.rules().to_owned());
        PlatformItem {
            url: self.reference.canonical_url(),
            reference: PlatformRef::Polymarket(self.reference),
            depth,
            title: Some(fields.question.clone()),
            authors: Vec::new(),
            published: self.start_date,
            data: PlatformItemData::Polymarket(PolymarketItemData::Market {
                event_id: parent.and_then(|parent| parent.id.as_ref()).map(Id::text),
                market: Box::new(fields),
                neg_risk: self.neg_risk,
                event,
                price_source,
                prices_as_of: prices_as_of.or(self.updated_at),
                rules,
            }),
        }
    }
}

/// A Gamma event decoded with its markets.
pub(super) struct Event {
    pub(super) reference: PolymarketRef,
    pub(super) markets: Vec<Market>,
    id: String,
    title: Option<String>,
    description: String,
    start_date: Option<String>,
    end_date: Option<String>,
    volume: Option<f64>,
    volume_24h: Option<f64>,
    liquidity: Option<f64>,
    neg_risk: bool,
    pub(super) updated_at: Option<String>,
}

impl Event {
    /// Decodes a Gamma event and every market in it; an event without a `markets` list or with
    /// a market that does not decode fails.
    pub(super) fn decode(event: GammaEvent) -> Result<Self, String> {
        let slug = event.slug.unwrap_or_default();
        let reference = PolymarketRef::event(&slug).ok_or_else(|| {
            format!("Polymarket Gamma API returned an invalid event slug `{slug}`")
        })?;
        let markets = event
            .markets
            .ok_or_else(|| {
                format!("Polymarket Gamma API returned polymarket:{reference} without markets")
            })?
            .into_iter()
            .map(Market::decode)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            markets,
            id: event.id.text(),
            title: event.title,
            description: event.description.unwrap_or_default(),
            start_date: event.start_date.as_deref().and_then(utc_timestamp),
            end_date: event.end_date.as_deref().and_then(utc_timestamp),
            volume: event.volume.as_ref().and_then(Number::value),
            volume_24h: event.volume_24h.as_ref().and_then(Number::value),
            liquidity: event.liquidity.as_ref().and_then(Number::value),
            neg_risk: event.neg_risk.unwrap_or_default(),
            updated_at: event.updated_at.as_deref().and_then(utc_timestamp),
            reference,
        })
    }

    /// The event's rules, empty when Gamma has none.
    pub(super) fn rules(&self) -> &str {
        self.description.trim()
    }

    fn fields(&self, price_source: PolymarketPriceSource) -> PolymarketEventFields {
        PolymarketEventFields {
            event_id: self.id.clone(),
            status: event_status(self.markets.iter().map(|market| market.status)),
            end_date: self.end_date.clone(),
            volume: self.volume,
            volume_24h: self.volume_24h,
            liquidity: self.liquidity,
            neg_risk: self.neg_risk,
            market_count: self.markets.len(),
            price_source,
        }
    }

    fn item(self, depth: ContentDepth, data: PolymarketItemData) -> PlatformItem {
        PlatformItem {
            url: self.reference.canonical_url(),
            reference: PlatformRef::Polymarket(self.reference),
            depth,
            title: self.title,
            authors: Vec::new(),
            published: self.start_date,
            data: PlatformItemData::Polymarket(data),
        }
    }

    /// Builds a search result: the event priced from Gamma alone, with its most probable
    /// markets first and markets without a probability after them.
    pub(super) fn into_search_item(self) -> PlatformItem {
        let mut top_markets = self.markets.iter().map(Market::summary).collect::<Vec<_>>();
        top_markets.sort_by(|left, right| match (left.probability, right.probability) {
            (Some(left), Some(right)) => right.total_cmp(&left),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        });
        top_markets.truncate(TOP_MARKETS);
        let event = self.fields(PolymarketPriceSource::Gamma);
        self.item(
            ContentDepth::Metadata,
            PolymarketItemData::SearchEvent { event, top_markets },
        )
    }

    /// Builds the item of a fetched event. At `abstract` depth it carries the event's rules,
    /// and each market's own only where they differ from the event's.
    pub(super) fn into_item(
        mut self,
        depth: ContentDepth,
        quotes: &HashMap<String, MarketQuotes>,
        price_source: PolymarketPriceSource,
        prices_as_of: Option<String>,
    ) -> PlatformItem {
        let event_rules = self.rules().to_owned();
        let event = self.fields(price_source);
        let markets = std::mem::take(&mut self.markets)
            .into_iter()
            .map(|market| {
                let quotes = quotes
                    .get(market.reference.slug())
                    .unwrap_or(&MarketQuotes::Cached);
                let rules = (depth == ContentDepth::Abstract
                    && !market.rules().is_empty()
                    && market.rules() != event_rules)
                    .then(|| market.rules().to_owned());
                PolymarketMarket {
                    url: market.reference.canonical_url(),
                    fields: market.fields(quotes),
                    reference: PlatformRef::Polymarket(market.reference),
                    rules,
                }
            })
            .collect::<Vec<_>>();
        let prices_as_of = prices_as_of.or_else(|| self.updated_at.clone());
        let rules = (depth == ContentDepth::Abstract).then_some(event_rules);
        self.item(
            depth,
            PolymarketItemData::Event {
                event,
                prices_as_of,
                markets,
                rules,
            },
        )
    }
}

/// Decodes a JSON array encoded as a string; an absent field is an empty list.
fn decode_list<T: serde::de::DeserializeOwned>(
    value: Option<&str>,
    field: &str,
    reference: &PolymarketRef,
) -> Result<Vec<T>, String> {
    value.map_or(Ok(Vec::new()), |text| {
        serde_json::from_str(text).map_err(|error| {
            format!("Polymarket Gamma API returned polymarket:{reference} with an undecodable `{field}`: {error}")
        })
    })
}
