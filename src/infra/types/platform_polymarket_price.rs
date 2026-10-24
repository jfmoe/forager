//! Polymarket market status and outcome prices: the Market-Implied Probability rules of
//! ADR 0032 as zero-IO functions over what Gamma and the CLOB return.

use super::platform_polymarket::{PolymarketBasis, PolymarketOutcome, PolymarketStatus};

const MICROS: u32 = 1_000_000;

/// A price in [0, 1], held as millionths so midpoints and complements stay exact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Price(u32);

impl Price {
    /// Reads a price Polymarket writes as a decimal string or a number; `None` outside [0, 1].
    pub(crate) fn parse(value: &str) -> Option<Self> {
        value.trim().parse::<f64>().ok().and_then(Self::from_f64)
    }

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the value is checked to be a whole number in [0, 1_000_000]"
    )]
    pub(crate) fn from_f64(value: f64) -> Option<Self> {
        let micros = (value * f64::from(MICROS)).round();
        (value.is_finite() && (0.0..=f64::from(MICROS)).contains(&micros))
            .then_some(Self(micros as u32))
    }

    pub(crate) fn value(self) -> f64 {
        f64::from(self.0) / f64::from(MICROS)
    }

    const fn complement(self) -> Self {
        Self(MICROS - self.0)
    }
}

/// The best bid and best ask of one outcome; a side without orders is `None`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct BookTop {
    bid: Option<Price>,
    ask: Option<Price>,
}

impl BookTop {
    /// Builds a top of book as Polymarket reports it: a bid of 0 means no buyer and an ask of 1
    /// no seller, because a real order sits at least one tick inside [0, 1].
    pub(crate) fn new(bid: Option<Price>, ask: Option<Price>) -> Self {
        Self {
            bid: bid.filter(|bid| bid.0 > 0),
            ask: ask.filter(|ask| ask.0 < MICROS),
        }
    }

    /// The top of book of the other outcome of a binary market: its bid is 1 minus this ask.
    fn complement(self) -> Self {
        Self {
            bid: self.ask.map(Price::complement),
            ask: self.bid.map(Price::complement),
        }
    }

    fn midpoint(self) -> Option<f64> {
        let (bid, ask) = (self.bid?, self.ask?);
        Some(f64::from(bid.0 + ask.0) / f64::from(2 * MICROS))
    }

    fn spread(self) -> Option<f64> {
        let (bid, ask) = (self.bid?, self.ask?);
        Some((f64::from(ask.0) - f64::from(bid.0)) / f64::from(MICROS))
    }
}

/// Where the quotes of an open market come from.
pub(crate) enum QuoteSource {
    /// The live book of each outcome, in outcome order; `None` for an outcome the CLOB left
    /// out of its answer.
    Clob(Vec<Option<BookTop>>),
    /// Gamma's cached book of the first outcome, used for search and when the CLOB fails.
    Gamma(BookTop),
}

/// The priced outcomes of one market.
pub(crate) struct MarketPricing {
    pub(crate) outcomes: Vec<PolymarketOutcome>,
    /// The Yes price of a Yes/No market.
    pub(crate) probability: Option<f64>,
    pub(crate) spread: Option<f64>,
}

/// Derives a market's status. `active` is never read: a resolved market stays active.
pub(crate) fn market_status(
    closed: bool,
    resolved: bool,
    accepting_orders: bool,
) -> PolymarketStatus {
    match (closed, resolved, accepting_orders) {
        (true, true, _) => PolymarketStatus::Resolved,
        (true, false, _) => PolymarketStatus::Closed,
        (false, _, true) => PolymarketStatus::Open,
        (false, _, false) => PolymarketStatus::Suspended,
    }
}

/// Derives an event's status from its markets' statuses.
pub(crate) fn event_status(
    markets: impl IntoIterator<Item = PolymarketStatus>,
) -> PolymarketStatus {
    let statuses = markets.into_iter().collect::<Vec<_>>();
    if statuses.contains(&PolymarketStatus::Open) {
        PolymarketStatus::Open
    } else if !statuses.is_empty()
        && statuses
            .iter()
            .all(|status| *status == PolymarketStatus::Resolved)
    {
        PolymarketStatus::Resolved
    } else {
        PolymarketStatus::Closed
    }
}

/// Prices each outcome of a market. `settled` is Gamma's `outcomePrices`: the payouts of a
/// resolved market and the cached midpoints of an open one. A resolved market's leftover bid,
/// ask, and last trade are never read. Probabilities are never normalized across a group.
pub(crate) fn price_market(
    status: PolymarketStatus,
    names: &[String],
    settled: &[Price],
    quotes: &QuoteSource,
) -> MarketPricing {
    let outcomes = names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let (price, basis, top) = price_outcome(status, index, settled, quotes);
            PolymarketOutcome {
                name: name.clone(),
                price,
                basis,
                best_bid: top.bid.map(Price::value),
                best_ask: top.ask.map(Price::value),
            }
        })
        .collect::<Vec<_>>();
    let first_top = if status == PolymarketStatus::Open {
        book_top(0, quotes)
    } else {
        BookTop::default()
    };
    let probability = (names == ["Yes", "No"])
        .then(|| outcomes.first().and_then(|outcome| outcome.price))
        .flatten();
    MarketPricing {
        outcomes,
        probability,
        spread: first_top.spread(),
    }
}

fn price_outcome(
    status: PolymarketStatus,
    index: usize,
    settled: &[Price],
    quotes: &QuoteSource,
) -> (Option<f64>, PolymarketBasis, BookTop) {
    match status {
        PolymarketStatus::Resolved => (
            settled.get(index).map(|price| price.value()),
            PolymarketBasis::Resolution,
            BookTop::default(),
        ),
        PolymarketStatus::Closed | PolymarketStatus::Suspended => {
            (None, PolymarketBasis::NotTrading, BookTop::default())
        }
        PolymarketStatus::Open => {
            let top = book_top(index, quotes);
            match (quotes, top.midpoint()) {
                (QuoteSource::Clob(_), Some(midpoint)) => {
                    (Some(midpoint), PolymarketBasis::Midpoint, top)
                }
                (QuoteSource::Gamma(_), Some(_)) => match settled.get(index) {
                    Some(price) => (Some(price.value()), PolymarketBasis::MidpointCached, top),
                    None => (None, PolymarketBasis::NoTwoSidedBook, top),
                },
                (_, None) => (None, PolymarketBasis::NoTwoSidedBook, top),
            }
        }
    }
}

/// The book of outcome `index`: from the CLOB, or derived from Gamma's first-outcome book for
/// the two outcomes of a binary market.
fn book_top(index: usize, quotes: &QuoteSource) -> BookTop {
    match quotes {
        QuoteSource::Clob(tops) => tops.get(index).copied().flatten().unwrap_or_default(),
        QuoteSource::Gamma(top) => match index {
            0 => *top,
            1 => top.complement(),
            _ => BookTop::default(),
        },
    }
}
