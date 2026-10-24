//! Live CLOB quotes for Polymarket fetch: the best bid and ask of every outcome token from
//! `POST /prices`, and each token's last trade from `POST /last-trades-prices`. The CLOB only
//! enriches Gamma's result, so a failed price read leaves the caller to price from Gamma's cache.

use std::collections::HashMap;

use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use super::gamma::Number;
use super::{Call, Endpoint, PolymarketApi};
use crate::catalog::PlatformOperation;
use crate::providers::shared::{Refusal, Trail, utc_second};
use crate::types::{BookTop, ProviderError};

/// The most entries one CLOB batch request takes; a larger batch is refused with HTTP 400
/// (measured 2026-10-24). A price entry asks for one side of one token.
const MAX_BATCH_ITEMS: usize = 500;

/// What the CLOB said about a set of outcome tokens.
pub(super) struct LiveQuotes {
    /// The top of book of every token the CLOB answered for.
    pub(super) tops: HashMap<String, BookTop>,
    /// The last trade of every token that has traded; `None` when that read failed.
    pub(super) last_trades: Option<HashMap<String, f64>>,
    /// When the price read completed, in UTC.
    pub(super) as_of: String,
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
    /// Reads the live quotes of `tokens`. A failed price read is the error of the whole read;
    /// a failed last-trade read only leaves the last trades out, with a note on `trail`.
    pub(super) async fn live_quotes(
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
