//! Whether a Live Check's output shows what its contract requires.

use serde_json::Value;

use crate::catalog::LiveContract;

/// Returns why `payload` breaks `contract`, or `None` when it holds.
pub(super) fn violation(contract: LiveContract, payload: &Value) -> Option<&'static str> {
    if contains_runtime_error(payload) {
        return Some("the output includes a runtime-class provider attempt");
    }
    let holds = match contract {
        LiveContract::Answer => nonempty_string(payload, "answer"),
        LiveContract::PipelineSearch => {
            nonempty_string(payload, "answer") && nonempty_array(payload, "extra_sources")
        }
        LiveContract::Results => nonempty_array(payload, "results"),
        // A read after the keys failed proves the Anonymous Fallback, not the keys; a keyless
        // read proves the anonymous path only when no attempt sent a key.
        LiveContract::Fetch { anonymous } => {
            nonempty_string(payload, "content")
                && payload["provider_attempts"]
                    .as_array()
                    .is_some_and(|attempts| {
                        !attempts.is_empty()
                            && attempts
                                .iter()
                                .all(|attempt| attempt["anonymous"] == anonymous)
                    })
        }
        LiveContract::PlatformItems => nonempty_array(payload, "items"),
        // X posts have no title; their text proves the read.
        LiveContract::PlatformItem => {
            nonempty_string(payload, "ref")
                && (nonempty_string(payload, "title") || nonempty_string(payload, "text"))
        }
        // Many Xiaohongshu notes have no title, so only the body proves the read.
        LiveContract::PlatformFullText => {
            nonempty_string(payload, "ref")
                && payload["content_len"].as_u64().is_some_and(|len| len > 0)
        }
        // The canary video has millions of comments, so an empty page means a broken read.
        LiveContract::Comments => nonempty_array(payload, "comments"),
        LiveContract::XiaohongshuComments => return xiaohongshu_comments(payload),
        // A resolved market is priced by its payout, so the fetch never asks the CLOB.
        LiveContract::PolymarketResolved { winner } => {
            let winner = elements(payload, "markets")
                .find(|market| market["ref"] == winner)
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
        LiveContract::PolymarketLive => {
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
        LiveContract::PolymarketSettlement => {
            nonempty_array(payload, "points") && payload["settlement"]["price"] == 1.0
        }
    };
    (!holds).then_some("the output is empty or lacks a field the contract requires")
}

/// A note without comments is a legitimate result, but it proves nothing here: the read must
/// reach the second comment page (10 top-level comments each) and expand one comment's replies
/// (a comment carries one reply before it is expanded).
fn xiaohongshu_comments(payload: &Value) -> Option<&'static str> {
    if !nonempty_string(payload, "note") {
        return Some("the output names no note");
    }
    let comments = payload["comments"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    if comments.len() <= 10 {
        return Some("the note has 10 or fewer comments, so the second comment page was not read");
    }
    let expanded = comments.iter().any(|comment| {
        comment["replies"]
            .as_array()
            .is_some_and(|replies| replies.len() > 1)
    });
    (!expanded).then_some("no returned comment had more replies to expand")
}

fn contains_runtime_error(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            object
                .get("error_kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "runtime")
                || object.values().any(contains_runtime_error)
        }
        Value::Array(values) => values.iter().any(contains_runtime_error),
        _ => false,
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

#[cfg(test)]
#[path = "live_contract_tests.rs"]
mod tests;
