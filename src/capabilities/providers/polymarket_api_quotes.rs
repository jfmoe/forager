//! Live CLOB quotes for Polymarket fetch, and the fallback to Gamma's cache: the best bid and
//! ask of every outcome token from `POST /prices`, and each token's last trade from
//! `POST /last-trades-prices`. The CLOB only enriches Gamma's result, so a failed price read
//! prices every market from Gamma's cache with one diagnostic.

use std::collections::HashMap;

use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use super::gamma::{Market, MarketQuotes, Number, Quotes};
use super::{Call, Endpoint, PolymarketApi};
use crate::catalog::PlatformOperation;
use crate::providers::shared::{Refusal, Trail, utc_second};
use crate::types::{BookTop, PolymarketStatus, ProviderError};

/// The most entries one CLOB batch request takes; a larger batch is refused with HTTP 400
/// (measured 2026-10-24). A price entry asks for one side of one token.
const MAX_BATCH_ITEMS: usize = 500;
const STALE_QUOTES: &str = "prices are Gamma's cached quotes, which may lag up to 5 minutes";

/// What the CLOB said about a set of outcome tokens.
struct LiveQuotes {
    /// The top of book of every token the CLOB answered for.
    tops: HashMap<String, BookTop>,
    /// The last trade of every token that has traded; `None` when that read failed.
    last_trades: Option<HashMap<String, f64>>,
    /// When the price read completed, in UTC.
    as_of: String,
}

/// One side of a token's best price, as `POST /prices` answers: BUY is the best bid and SELL
/// the best ask.
#[derive(Deserialize)]
struct Sides {
    #[serde(rename = "BUY")]
    buy: Option<Number>,
    #[serde(rename = "SELL")]
    sell: Option<Number>,
}

#[derive(Deserialize)]
struct LastTrade {
    token_id: String,
    price: Number,
}

impl PolymarketApi {
    /// Reads the live quotes of the open ones of `markets`. With no open market, or when the
    /// CLOB fails, every market prices from Gamma's cache.
    pub(super) async fn quote(&self, markets: &[Market], trail: &mut Trail) -> Quotes {
        let open = markets
            .iter()
            .filter(|market| market.status == PolymarketStatus::Open)
            .collect::<Vec<_>>();
        if open.is_empty() {
            return Quotes::Cached;
        }
        let tokens = open
            .iter()
            .flat_map(|market| market.tokens.iter().cloned())
            .collect::<Vec<_>>();
        let live = match self.live_quotes(&tokens, trail).await {
            Ok(live) => live,
            Err(error) => {
                trail.note(format!(
                    "Polymarket CLOB API unavailable ({}); {STALE_QUOTES}",
                    error.message
                ));
                return Quotes::Cached;
            }
        };
        let missing = tokens
            .iter()
            .filter(|token| !live.tops.contains_key(*token))
            .count();
        if missing > 0 {
            trail.note(format!(
                "Polymarket CLOB API returned no quote for {missing} {}; {} no price",
                if missing == 1 { "outcome" } else { "outcomes" },
                if missing == 1 { "it has" } else { "they have" }
            ));
        }
        let markets = open
            .iter()
            .map(|market| {
                let quotes = MarketQuotes::Live {
                    tops: market
                        .tokens
                        .iter()
                        .map(|token| live.tops.get(token).copied())
                        .collect(),
                    last_trade: live
                        .last_trades
                        .as_ref()
                        .and_then(|trades| {
                            market.tokens.first().and_then(|token| trades.get(token))
                        })
                        .copied(),
                };
                (market.reference.slug().to_owned(), quotes)
            })
            .collect();
        Quotes::Live {
            markets,
            as_of: live.as_of,
        }
    }

    /// Reads the live quotes of `tokens`. A failed price read is the error of the whole read;
    /// a failed last-trade read only leaves the last trades out, with a note on `trail`.
    async fn live_quotes(
        &self,
        tokens: &[String],
        trail: &mut Trail,
    ) -> Result<LiveQuotes, ProviderError> {
        let operation = PlatformOperation::Fetch.as_str();
        let mut tops = HashMap::new();
        for batch in tokens.chunks(MAX_BATCH_ITEMS / 2) {
            let body = Value::Array(
                batch
                    .iter()
                    .flat_map(|token| {
                        ["BUY", "SELL"].map(|side| json!({"token_id": token, "side": side}))
                    })
                    .collect(),
            );
            let answered = trail.optional(
                self.send(
                    Endpoint::Clob,
                    Call::Post {
                        path: "prices",
                        body: &body,
                    },
                    operation,
                    decode_prices,
                )
                .await,
            )?;
            tops.extend(answered);
        }
        let as_of = utc_second(Utc::now());
        let mut last_trades = HashMap::new();
        for batch in tokens.chunks(MAX_BATCH_ITEMS) {
            let body = Value::Array(
                batch
                    .iter()
                    .map(|token| json!({"token_id": token}))
                    .collect(),
            );
            let read = trail.optional(
                self.send(
                    Endpoint::Clob,
                    Call::Post {
                        path: "last-trades-prices",
                        body: &body,
                    },
                    operation,
                    decode_last_trades,
                )
                .await,
            );
            match read {
                Ok(trades) => last_trades.extend(trades),
                Err(error) => {
                    trail.note(format!(
                        "Polymarket CLOB API last trades unavailable ({}); last_trade_price is null",
                        error.message
                    ));
                    return Ok(LiveQuotes {
                        tops,
                        last_trades: None,
                        as_of,
                    });
                }
            }
        }
        Ok(LiveQuotes {
            tops,
            last_trades: Some(last_trades),
            as_of,
        })
    }

    /// Asks the CLOB for its clock, which proves the endpoint answers.
    pub(crate) async fn probe_clob(&self) -> Result<(), ProviderError> {
        self.send(
            Endpoint::Clob,
            Call::Get {
                path: "time",
                query: &[],
            },
            PlatformOperation::Search.as_str(),
            |body| {
                serde_json::from_str::<u64>(body.trim())
                    .map(|_| ())
                    .map_err(|error| {
                        Refusal::runtime(format!("invalid Polymarket CLOB API time: {error}"))
                    })
            },
        )
        .await
        .map(|_| ())
    }
}

fn decode_prices(body: &str) -> Result<HashMap<String, BookTop>, Refusal> {
    let sides = serde_json::from_str::<HashMap<String, Sides>>(body).map_err(|error| {
        Refusal::runtime(format!("invalid Polymarket CLOB API prices: {error}"))
    })?;
    Ok(sides
        .into_iter()
        .map(|(token, sides)| {
            let top = BookTop::new(
                sides.buy.as_ref().and_then(Number::price),
                sides.sell.as_ref().and_then(Number::price),
            );
            (token, top)
        })
        .collect())
}

fn decode_last_trades(body: &str) -> Result<HashMap<String, f64>, Refusal> {
    let trades = serde_json::from_str::<Vec<LastTrade>>(body).map_err(|error| {
        Refusal::runtime(format!("invalid Polymarket CLOB API last trades: {error}"))
    })?;
    Ok(trades
        .into_iter()
        .filter_map(|trade| Some((trade.token_id, trade.price.value()?)))
        .collect())
}
