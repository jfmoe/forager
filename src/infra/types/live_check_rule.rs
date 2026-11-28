use serde::{Serialize, Serializer};

/// A rule a Live Check can break: a shape rule of a contract's field, a rule of its sample, or a
/// rule of the run itself.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveCheckRule {
    Present,
    Type,
    Nonblank,
    AtLeast,
    MinItems,
    InSomeItems,
    NoRuntimeAttempt,
    KeyedRead,
    AnonymousRead,
    ResolvedByPayout,
    LiveQuotes,
    CommandSucceeds,
    TimeLimit,
    ValidJson,
    CommandRuns,
}

impl LiveCheckRule {
    /// Every rule, in the order the product skill lists them.
    pub const VOCABULARY: [Self; 15] = [
        Self::Present,
        Self::Type,
        Self::Nonblank,
        Self::AtLeast,
        Self::MinItems,
        Self::InSomeItems,
        Self::NoRuntimeAttempt,
        Self::KeyedRead,
        Self::AnonymousRead,
        Self::ResolvedByPayout,
        Self::LiveQuotes,
        Self::CommandSucceeds,
        Self::TimeLimit,
        Self::ValidJson,
        Self::CommandRuns,
    ];

    /// The name a report and a run record show.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Present => "present",
            Self::Type => "type",
            Self::Nonblank => "nonblank",
            Self::AtLeast => "at_least",
            Self::MinItems => "min_items",
            Self::InSomeItems => "in_some_items",
            Self::NoRuntimeAttempt => "no_runtime_attempt",
            Self::KeyedRead => "keyed_read",
            Self::AnonymousRead => "anonymous_read",
            Self::ResolvedByPayout => "resolved_by_payout",
            Self::LiveQuotes => "live_quotes",
            Self::CommandSucceeds => "command_succeeds",
            Self::TimeLimit => "time_limit",
            Self::ValidJson => "valid_json",
            Self::CommandRuns => "command_runs",
        }
    }
}

impl Serialize for LiveCheckRule {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}
