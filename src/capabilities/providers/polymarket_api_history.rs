//! Polymarket Price History: the market and outcome token from Gamma, then the series from the
//! Data API v2 `GET /v2/prices-history`, the only source of it, so either failure fails the
//! operation. The Data API ends a resolved market's series with its payout at
//! `resolution_seconds` 0; that point becomes the settlement, never part of the series.

use chrono::{DateTime, Days, NaiveDate, NaiveTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::gamma::{Entry, Market, Number};
use super::{Call, Endpoint, PolymarketApi};
use crate::catalog::PlatformOperation;
use crate::providers::execution::ExecutionOutcome;
use crate::providers::shared::{Refusal, Trail, utc_second};
use crate::types::{
    AttemptErrorKind, HISTORY, PlatformRef, PolymarketHistoryMarket, PolymarketHistoryOutcome,
    PolymarketHistoryRequest, PolymarketPricePoint, PolymarketStatus, PolymarketWindow,
    ProviderError,
};

/// The most pages one history reads; a longer series is cut with a diagnostic.
const MAX_PAGES: usize = 5;
/// How many markets the diagnostic about an event with several of them lists.
const LISTED_MARKETS: usize = 10;

/// What the Gamma read found: the market of the history with the index of its requested
/// outcome, or the markets of an event that holds several.
enum Selection {
    One { market: Box<Market>, outcome: usize },
    Several(Vec<Market>),
}

/// One page of `GET /v2/prices-history`, as the Data API writes it.
#[derive(Deserialize)]
struct PricePage {
    data: Vec<PricePoint>,
    pagination: PagePosition,
}

#[derive(Deserialize)]
struct PricePoint {
    /// The observation's time in epoch seconds: a bucket's start, or the tick's own time.
    timestamp: i64,
    price: Number,
    /// The width of the window the price was observed in; 0 for an exact tick or a payout.
    resolution_seconds: i64,
}

#[derive(Deserialize)]
struct PagePosition {
    has_more: bool,
    next_cursor: Option<String>,
}

/// A decoded page: its points, and the cursor of the next page when one follows.
struct Page {
    points: Vec<Point>,
    next_cursor: Option<String>,
}

struct Point {
    time: DateTime<Utc>,
    price: f64,
    /// Whether the price is a single observation rather than a bucket's.
    exact: bool,
}

impl Point {
    fn output(self) -> PolymarketPricePoint {
        PolymarketPricePoint {
            t: utc_second(self.time),
            price: self.price,
        }
    }
}

impl PolymarketApi {
    /// Reads the price series of one outcome of one market, following the Data API cursor for
    /// up to five pages.
    pub(crate) async fn history(
        &self,
        request: &PolymarketHistoryRequest,
    ) -> Result<PolymarketHistoryOutcome, ProviderError> {
        let mut trail = Trail::default();
        let (market, outcome) = match trail.required(self.history_market(request).await)? {
            Selection::One { market, outcome } => (*market, outcome),
            // The failure message is cut at 500 characters, which ten long refs would pass, so
            // the markets to pick from go to stderr in full.
            Selection::Several(markets) => {
                trail.note(market_listing(&markets));
                return Err(trail.error(
                    AttemptErrorKind::Parameter,
                    format!(
                        "polymarket:{} has {} markets and history reads one; pass a market ref, such as one listed on stderr",
                        request.market,
                        markets.len()
                    ),
                ));
            }
        };
        let outcome_name = market.names[outcome].clone();
        let Some(token) = market.tokens.get(outcome).cloned() else {
            return Err(trail.error(
                AttemptErrorKind::Runtime,
                format!(
                    "Polymarket Gamma API returned polymarket:{} without a token for outcome `{outcome_name}`",
                    market.reference
                ),
            ));
        };
        let mut query = vec![("token_id", token)];
        query.extend(window_query(request.window));
        query.push(("bucket_seconds", request.bucket.seconds().to_string()));
        let mut points = Vec::new();
        let mut cut = false;
        for page in 1..=MAX_PAGES {
            let read = trail.required(
                self.send(
                    Endpoint::Data,
                    Call::Get {
                        path: "v2/prices-history",
                        query: &query,
                    },
                    HISTORY,
                    decode_page,
                )
                .await,
            )?;
            points.extend(read.points);
            let Some(cursor) = read.next_cursor else {
                break;
            };
            if page == MAX_PAGES {
                cut = true;
                break;
            }
            // The cursor holds only the position; the Data API needs the same query with it.
            query.retain(|(name, _)| *name != "cursor");
            query.push(("cursor", cursor));
        }
        if cut {
            trail.note(format!(
                "Polymarket price history has more than {MAX_PAGES} pages; listed the first {} points; pass a larger --bucket to list the whole window",
                points.len()
            ));
        }
        let settlement = (!cut
            && market.status == PolymarketStatus::Resolved
            && points.last().is_some_and(|point| point.exact))
        .then(|| points.pop())
        .flatten();
        Ok(PolymarketHistoryOutcome {
            market: PolymarketHistoryMarket {
                url: market.reference.canonical_url(),
                reference: PlatformRef::Polymarket(market.reference),
                question: market.question,
                status: market.status,
            },
            outcome: outcome_name,
            points: points.into_iter().map(Point::output).collect(),
            settlement: settlement.map(Point::output),
            diagnostic: trail.diagnostic(),
            attempts: trail.attempts,
        })
    }

    /// Asks the Data API for its service status, which proves the endpoint answers.
    pub(crate) async fn probe_data(&self) -> Result<(), ProviderError> {
        self.send(
            Endpoint::Data,
            Call::Get {
                path: "v2/status",
                query: &[],
            },
            PlatformOperation::Search.as_str(),
            |body| {
                serde_json::from_str::<Value>(body)
                    .ok()
                    .filter(|status| status["data"].is_object())
                    .map(|_| ())
                    .ok_or_else(|| Refusal::runtime("invalid Polymarket Data API status"))
            },
        )
        .await
        .map(|_| ())
    }

    /// Reads the market of a history from Gamma and picks its outcome. An event stands for its
    /// market only when it holds exactly one; one with several returns them to choose from.
    async fn history_market(
        &self,
        request: &PolymarketHistoryRequest,
    ) -> Result<ExecutionOutcome<Selection>, ProviderError> {
        let reference = &request.market;
        self.read_entry(reference, HISTORY, |entry| {
            let market = match entry {
                Entry::Market(market) => market,
                Entry::Event(event) => match <[Market; 1]>::try_from(event.markets) {
                    Ok([market]) => market,
                    Err(markets) if markets.is_empty() => {
                        return Err(Refusal::runtime(format!(
                            "Polymarket Gamma API returned polymarket:{reference} without markets"
                        )));
                    }
                    Err(markets) => return Ok(Selection::Several(markets)),
                },
            };
            let outcome = pick_outcome(&market, request.outcome.as_deref())?;
            Ok(Selection::One {
                market: Box::new(market),
                outcome,
            })
        })
        .await
    }
}

/// Names the first markets of an event, with their labels, for the caller to pick one.
fn market_listing(markets: &[Market]) -> String {
    let listed = markets
        .iter()
        .take(LISTED_MARKETS)
        .map(|market| format!("polymarket:{} ({})", market.reference, market.label))
        .collect::<Vec<_>>()
        .join(", ");
    let more = match markets.len().saturating_sub(LISTED_MARKETS) {
        0 => String::new(),
        more => format!(", and {more} more"),
    };
    format!("markets: {listed}{more}")
}

/// The index of the requested outcome, matched by name in any letter case; the first outcome
/// when none is requested.
fn pick_outcome(market: &Market, wanted: Option<&str>) -> Result<usize, Refusal> {
    if market.names.is_empty() {
        return Err(Refusal::runtime(format!(
            "Polymarket Gamma API returned polymarket:{} without outcomes",
            market.reference
        )));
    }
    let Some(wanted) = wanted.map(str::trim) else {
        return Ok(0);
    };
    let lowered = wanted.to_lowercase();
    market
        .names
        .iter()
        .position(|name| name.to_lowercase() == lowered)
        .ok_or_else(|| {
            Refusal::new(
                AttemptErrorKind::Parameter,
                format!(
                    "polymarket:{} has no outcome `{wanted}`; its outcomes are {}",
                    market.reference,
                    market.names.join(", ")
                ),
            )
        })
}

/// The Data API's window parameters: an interval preset, or epoch seconds from the start of
/// `from` to the end of `to`, which the Data API takes as exclusive.
fn window_query(window: PolymarketWindow) -> Vec<(&'static str, String)> {
    match window {
        PolymarketWindow::Interval(interval) => vec![("interval", interval.as_str().to_owned())],
        PolymarketWindow::Range { from, to } => vec![
            ("start", epoch(from).to_string()),
            ("end", epoch(to + Days::new(1)).to_string()),
        ],
    }
}

fn epoch(day: NaiveDate) -> i64 {
    day.and_time(NaiveTime::MIN).and_utc().timestamp()
}

fn decode_page(body: &str) -> Result<Page, Refusal> {
    let invalid = |detail: String| {
        Refusal::runtime(format!(
            "invalid Polymarket Data API price history: {detail}"
        ))
    };
    let page =
        serde_json::from_str::<PricePage>(body).map_err(|error| invalid(error.to_string()))?;
    let next_cursor = match (page.pagination.has_more, page.pagination.next_cursor) {
        (true, None) => return Err(invalid("`has_more` without `next_cursor`".to_owned())),
        (true, cursor) => cursor,
        (false, _) => None,
    };
    let points = page
        .data
        .into_iter()
        .map(|point| {
            let time = DateTime::from_timestamp(point.timestamp, 0)
                .ok_or_else(|| invalid(format!("timestamp {}", point.timestamp)))?;
            let price = point
                .price
                .value()
                .ok_or_else(|| invalid(format!("price at {}", point.timestamp)))?;
            Ok(Point {
                time,
                price,
                exact: point.resolution_seconds == 0,
            })
        })
        .collect::<Result<_, Refusal>>()?;
    Ok(Page {
        points,
        next_cursor,
    })
}
