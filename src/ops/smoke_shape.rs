//! What a live case's result must show to pass: the shape of each command's payload.

use serde_json::Value;

use super::POLYMARKET_CANARY_WINNER;

#[derive(Clone, Copy)]
pub(super) enum ResultShape {
    MainSearch,
    /// A platform answer, such as an X ask.
    Answer,
    PipelineSearch,
    SupplementalSearch,
    Fetch,
    Exa,
    Map,
    PlatformSearch,
    PlatformFetch,
    PlatformFullText,
    XiaohongshuComments,
    Comments,
    PolymarketResolved,
    PolymarketLive,
    PolymarketSettlement,
}

/// Whether the payload of a live case command has the fields its shape needs.
pub(super) fn result_shape_is_nonempty(shape: ResultShape, payload: &Value) -> bool {
    match shape {
        ResultShape::MainSearch | ResultShape::Answer => nonempty_string(payload, "answer"),
        ResultShape::PipelineSearch => {
            nonempty_string(payload, "answer") && nonempty_array(payload, "extra_sources")
        }
        ResultShape::SupplementalSearch | ResultShape::Exa | ResultShape::Map => {
            nonempty_array(payload, "results")
        }
        ResultShape::PlatformSearch => nonempty_array(payload, "items"),
        // X posts have no title; their text proves the read.
        ResultShape::PlatformFetch => {
            nonempty_string(payload, "ref")
                && (nonempty_string(payload, "title") || nonempty_string(payload, "text"))
        }
        // Many Xiaohongshu notes have no title, so only the body proves the read.
        ResultShape::PlatformFullText => {
            nonempty_string(payload, "ref")
                && payload["content_len"].as_u64().is_some_and(|len| len > 0)
        }
        // A read after the keys failed proves the Anonymous Fallback, not the keys.
        ResultShape::Fetch => {
            nonempty_string(payload, "content")
                && payload["provider_attempts"]
                    .as_array()
                    .is_some_and(|attempts| attempts.iter().all(|a| a["anonymous"] == false))
        }
        // A note without comments is a legitimate result, but it proves nothing here.
        ResultShape::XiaohongshuComments => nonempty_string(payload, "note"),
        // The canary video has millions of comments, so an empty page means a broken read.
        ResultShape::Comments => nonempty_array(payload, "comments"),
        // A resolved market is priced by its payout, so the fetch never asks the CLOB.
        ResultShape::PolymarketResolved => {
            let winner = elements(payload, "markets")
                .find(|market| market["ref"] == POLYMARKET_CANARY_WINNER)
                .unwrap_or(&Value::Null);
            let yes = &winner["outcomes"][0];
            elements(payload, "provider_attempts").count() == 1
                && winner["status"] == "resolved"
                && yes["name"] == "Yes"
                && yes["price"] == 1.0
                && yes["basis"] == "resolution"
        }
        // Gamma's cached prices stand in when the CLOB fails, so an event with an open market
        // proves the live read only with `clob`.
        ResultShape::PolymarketLive => {
            (payload["price_source"] == "clob"
                || elements(payload, "markets").all(|market| market["status"] != "open"))
                && nonempty_string(payload, "prices_as_of")
                && nonempty_array(payload, "markets")
                && elements(payload, "markets").all(|market| {
                    nonempty_array(market, "outcomes")
                        && elements(market, "outcomes").all(|outcome| {
                            nonempty_string(outcome, "name") && outcome["basis"].is_string()
                        })
                })
        }
        // The canary resolved Yes, so its whole history ends in a payout of 1 kept out of the
        // series.
        ResultShape::PolymarketSettlement => {
            nonempty_array(payload, "points") && payload["settlement"]["price"] == 1.0
        }
    }
}

fn elements<'a>(payload: &'a Value, field: &str) -> impl Iterator<Item = &'a Value> {
    payload[field].as_array().into_iter().flatten()
}

fn nonempty_string(payload: &Value, field: &str) -> bool {
    payload[field]
        .as_str()
        .is_some_and(|value| !value.trim().is_empty())
}

fn nonempty_array(payload: &Value, field: &str) -> bool {
    payload[field]
        .as_array()
        .is_some_and(|values| !values.is_empty())
}
