//! Polymarket Price History: the `history` operation's request, its window and bucket rules, and
//! its result. The rules follow what the Data API v2 serves (measured 2026-10-24).

use chrono::NaiveDate;
use serde::Serialize;

use super::attempt::ProviderAttempt;
use super::platform::{Platform, PlatformRef};
use super::platform_polymarket::{PolymarketRef, PolymarketStatus};

/// The attempt-target name of the Polymarket history operation.
pub(crate) const HISTORY: &str = "history";
/// The longest explicit window the Data API serves; 16 days is refused with HTTP 400.
const MAX_RANGE_DAYS: i64 = 15;
/// The longest explicit window that defaults to five-minute buckets.
const FINE_RANGE_DAYS: i64 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
/// A window that ends now.
pub enum PolymarketInterval {
    #[serde(rename = "1d")]
    Day,
    #[serde(rename = "1w")]
    Week,
    #[serde(rename = "1m")]
    Month,
    /// The market's whole life.
    #[serde(rename = "max")]
    Max,
}

impl PolymarketInterval {
    /// Returns the stable interval identifier, which the Data API takes as is.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Day => "1d",
            Self::Week => "1w",
            Self::Month => "1m",
            Self::Max => "max",
        }
    }

    /// The finest bucket the Data API accepts over this window.
    const fn finest_bucket(self) -> PolymarketBucket {
        match self {
            Self::Day => PolymarketBucket::OneMinute,
            Self::Week => PolymarketBucket::FiveMinutes,
            Self::Month | Self::Max => PolymarketBucket::ThirtyMinutes,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
/// The width of one price point, from the grid the Data API serves.
pub enum PolymarketBucket {
    OneMinute,
    FiveMinutes,
    ThirtyMinutes,
    OneHour,
    ThreeHours,
    TwelveHours,
    OneDay,
}

impl PolymarketBucket {
    /// Returns the bucket identifier commands take.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OneMinute => "1m",
            Self::FiveMinutes => "5m",
            Self::ThirtyMinutes => "30m",
            Self::OneHour => "1h",
            Self::ThreeHours => "3h",
            Self::TwelveHours => "12h",
            Self::OneDay => "1d",
        }
    }

    /// Returns the bucket width in seconds.
    #[must_use]
    pub const fn seconds(self) -> u32 {
        match self {
            Self::OneMinute => 60,
            Self::FiveMinutes => 300,
            Self::ThirtyMinutes => 1_800,
            Self::OneHour => 3_600,
            Self::ThreeHours => 10_800,
            Self::TwelveHours => 43_200,
            Self::OneDay => 86_400,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// The span a price history covers.
pub(crate) enum PolymarketWindow {
    Interval(PolymarketInterval),
    /// Whole UTC days, both ends included.
    Range {
        from: NaiveDate,
        to: NaiveDate,
    },
}

impl PolymarketWindow {
    /// The bucket that keeps the series to tens or a few hundred points.
    fn default_bucket(self) -> PolymarketBucket {
        match self {
            Self::Interval(PolymarketInterval::Day) => PolymarketBucket::FiveMinutes,
            Self::Range { from, to } if days(from, to) <= FINE_RANGE_DAYS => {
                PolymarketBucket::FiveMinutes
            }
            Self::Interval(PolymarketInterval::Week) | Self::Range { .. } => {
                PolymarketBucket::OneHour
            }
            Self::Interval(PolymarketInterval::Month | PolymarketInterval::Max) => {
                PolymarketBucket::OneDay
            }
        }
    }
}

/// The days a range covers, both ends included.
fn days(from: NaiveDate, to: NaiveDate) -> i64 {
    (to - from).num_days() + 1
}

/// A request for the price history of one outcome of one market.
#[derive(Clone, Debug)]
pub(crate) struct PolymarketHistoryRequest {
    /// A market, or an event that must turn out to hold exactly one market.
    pub(crate) market: PolymarketRef,
    /// The outcome by name, in any letter case; the first outcome when absent.
    pub(crate) outcome: Option<String>,
    pub(crate) window: PolymarketWindow,
    pub(crate) bucket: PolymarketBucket,
}

impl PolymarketHistoryRequest {
    /// Builds a request; without `bucket`, the window chooses one.
    ///
    /// # Errors
    ///
    /// Returns the message of a request the Data API would refuse: a range that ends before it
    /// starts or spans more than 15 days, or a bucket finer than the interval allows.
    pub(crate) fn new(
        market: PolymarketRef,
        outcome: Option<String>,
        window: PolymarketWindow,
        bucket: Option<PolymarketBucket>,
    ) -> Result<Self, String> {
        match window {
            PolymarketWindow::Range { from, to } if from > to => {
                return Err(format!("--from {from} is after --to {to}"));
            }
            PolymarketWindow::Range { from, to } if days(from, to) > MAX_RANGE_DAYS => {
                return Err(format!(
                    "--from {from} --to {to} spans {} days, and the Polymarket Data API serves at most {MAX_RANGE_DAYS}; use --interval for a longer span",
                    days(from, to)
                ));
            }
            PolymarketWindow::Interval(interval) => {
                if let Some(bucket) = bucket.filter(|bucket| *bucket < interval.finest_bucket()) {
                    return Err(format!(
                        "--bucket {} is finer than the Polymarket Data API serves over --interval {}; use {} or coarser",
                        bucket.as_str(),
                        interval.as_str(),
                        interval.finest_bucket().as_str()
                    ));
                }
            }
            PolymarketWindow::Range { .. } => {}
        }
        Ok(Self {
            market,
            outcome,
            bucket: bucket.unwrap_or_else(|| window.default_bucket()),
            window,
        })
    }
}

#[derive(Clone, Debug, Serialize)]
/// The market a price history belongs to.
pub struct PolymarketHistoryMarket {
    #[serde(rename = "ref")]
    pub reference: PlatformRef,
    pub url: String,
    pub question: String,
    pub status: PolymarketStatus,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
/// One price at one time.
pub struct PolymarketPricePoint {
    /// UTC ISO 8601 to the second: a bucket's start, or the time of a single observation.
    pub t: String,
    pub price: f64,
}

#[derive(Clone, Debug, Serialize)]
/// The price series of one outcome, oldest first. A resolved market's payout is the
/// `settlement`, never a point of the series.
pub struct PolymarketHistoryResult {
    pub platform: Platform,
    /// The route that read the history.
    pub provider: &'static str,
    pub market: PolymarketHistoryMarket,
    /// The outcome's name as the market writes it.
    pub outcome: String,
    /// The window that ends now; `null` for an explicit range.
    pub interval: Option<PolymarketInterval>,
    /// The first day of an explicit range.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<NaiveDate>,
    /// The last day of an explicit range, included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<NaiveDate>,
    pub bucket_seconds: u32,
    pub points: Vec<PolymarketPricePoint>,
    /// The payout of a resolved market, when the window reaches its settlement.
    pub settlement: Option<PolymarketPricePoint>,
    #[serde(rename = "provider_attempts", skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<ProviderAttempt>,
    #[serde(skip)]
    pub diagnostic: Option<String>,
}

/// One route's history before the chain attaches the route and the planning attempts.
pub(crate) struct PolymarketHistoryOutcome {
    pub(crate) market: PolymarketHistoryMarket,
    pub(crate) outcome: String,
    pub(crate) points: Vec<PolymarketPricePoint>,
    pub(crate) settlement: Option<PolymarketPricePoint>,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{PolymarketBucket, PolymarketInterval, PolymarketWindow};

    fn range(from: &str, to: &str) -> PolymarketWindow {
        PolymarketWindow::Range {
            from: from.parse::<NaiveDate>().expect("from"),
            to: to.parse::<NaiveDate>().expect("to"),
        }
    }

    #[test]
    fn the_default_bucket_keeps_each_window_to_a_readable_series() {
        let defaults = [
            PolymarketWindow::Interval(PolymarketInterval::Day),
            PolymarketWindow::Interval(PolymarketInterval::Week),
            PolymarketWindow::Interval(PolymarketInterval::Month),
            PolymarketWindow::Interval(PolymarketInterval::Max),
            range("2026-10-22", "2026-10-23"),
            range("2026-10-21", "2026-10-23"),
            range("2026-10-09", "2026-10-23"),
        ]
        .map(PolymarketWindow::default_bucket);

        assert_eq!(
            defaults,
            [
                PolymarketBucket::FiveMinutes,
                PolymarketBucket::OneHour,
                PolymarketBucket::OneDay,
                PolymarketBucket::OneDay,
                PolymarketBucket::FiveMinutes,
                PolymarketBucket::OneHour,
                PolymarketBucket::OneHour,
            ]
        );
    }
}
