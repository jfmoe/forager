//! The contract rules against forager's own output. `live_contract_outputs.json` holds, for
//! every declared check, an output in the shape its command prints that meets its contract.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{Detail, Observed, Rule, violation};
use crate::catalog::{Field, LiveContract, SampleRule};
use crate::live::declared;

fn passing_outputs() -> BTreeMap<String, Value> {
    serde_json::from_str(include_str!("live_contract_outputs.json")).expect("parse the outputs")
}

/// Removes the field `path` names from every item it reaches; a path that ends in a list's items
/// leaves each item null instead.
fn remove(value: &mut Value, path: &str) {
    let (step, rest) = path
        .split_once('.')
        .map_or((path, None), |(step, rest)| (step, Some(rest)));
    let (key, into_items) = step
        .strip_suffix("[]")
        .map_or((step, false), |key| (key, true));
    let Some(object) = value.as_object_mut() else {
        return;
    };
    match (rest, into_items) {
        (None, false) => {
            object.remove(key);
        }
        (None, true) => {
            for item in object
                .get_mut(key)
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                *item = Value::Null;
            }
        }
        (Some(rest), false) => {
            if let Some(inner) = object.get_mut(key) {
                remove(inner, rest);
            }
        }
        (Some(rest), true) => {
            for item in object
                .get_mut(key)
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                remove(item, rest);
            }
        }
    }
}

#[test]
fn every_declared_check_has_a_passing_output_that_meets_its_contract() {
    let outputs = passing_outputs();
    let declared = declared();

    let failures = declared
        .iter()
        .filter_map(|entry| match outputs.get(entry.check.id) {
            None => Some((entry.check.id, "no passing output".to_owned())),
            Some(output) => violation(&entry.check.contract, output)
                .map(|detail| (entry.check.id, detail.describe())),
        })
        .collect::<Vec<_>>();

    assert_eq!(
        (failures, outputs.len()),
        (Vec::new(), declared.len()),
        "every check needs one passing output, and every output a check"
    );
}

#[test]
fn removing_any_contracted_field_fails_the_contract_naming_that_field() {
    let outputs = passing_outputs();
    let mut unnoticed = Vec::new();
    for entry in declared() {
        let Some(output) = outputs.get(entry.check.id) else {
            continue;
        };
        for field in entry.check.contract.fields {
            let mut broken = output.clone();
            remove(&mut broken, field.path);
            let named = violation(&entry.check.contract, &broken).and_then(|detail| detail.path);
            if named != Some(field.path) {
                unnoticed.push((entry.check.id, field.path, named));
            }
        }
    }

    assert_eq!(unnoticed, Vec::new());
}

/// `path` with each item index replaced by `[]`, as a contract names it.
fn declared_form(path: &str) -> String {
    let mut form = String::new();
    let mut in_index = false;
    for character in path.chars() {
        match character {
            '[' => {
                in_index = true;
                form.push('[');
            }
            ']' => {
                in_index = false;
                form.push(']');
            }
            _ if in_index => {}
            _ => form.push(character),
        }
    }
    form
}

#[test]
fn removing_the_object_that_holds_a_contracted_field_fails_the_contract_naming_where_it_stopped() {
    let outputs = passing_outputs();
    let mut unnoticed = Vec::new();
    for entry in declared() {
        let Some(output) = outputs.get(entry.check.id) else {
            continue;
        };
        for field in entry.check.contract.fields {
            let steps = field.path.split('.').collect::<Vec<_>>();
            for end in 1..steps.len() {
                if steps[end - 1].ends_with("[]") {
                    continue;
                }
                let parent = steps[..end].join(".");
                let mut broken = output.clone();
                remove(&mut broken, &parent);
                let stopped = violation(&entry.check.contract, &broken)
                    .and_then(|detail| detail.at.or(detail.path.map(str::to_owned)))
                    .map(|at| declared_form(&at));
                if stopped.as_deref() != Some(parent.as_str()) {
                    unnoticed.push((entry.check.id, parent, stopped));
                }
            }
        }
    }

    assert_eq!(unnoticed, Vec::new());
}

#[test]
fn a_list_in_a_contract_is_declared_before_the_fields_of_its_items() {
    let mut misplaced = Vec::new();
    for entry in declared() {
        let fields = entry.check.contract.fields;
        for (position, field) in fields.iter().enumerate() {
            let mut prefix = String::new();
            for step in field.path.split('.') {
                if let Some(list) = step.strip_suffix("[]") {
                    let list_path = format!("{prefix}{list}");
                    if !fields[..position]
                        .iter()
                        .any(|earlier| earlier.path == list_path)
                        && field.path != format!("{list_path}[]")
                    {
                        misplaced.push((entry.check.id, field.path));
                    }
                }
                prefix.push_str(step);
                prefix.push('.');
            }
        }
    }

    assert_eq!(misplaced, Vec::new());
}

const CONTRACT: LiveContract = LiveContract::fields(&[
    Field::text("title"),
    Field::count("views", 1),
    Field::number("price", 0.0),
    Field::boolean("open"),
    Field::list("items", 2),
    Field::text("items[].ref"),
    Field::text("items[].link").in_some_items(),
    Field::text("owner.name"),
]);

fn meeting() -> Value {
    json!({
        "title": "A title",
        "views": 3,
        "price": 0.5,
        "open": true,
        "items": [{"ref": "a:1", "link": "https://example.com/"}, {"ref": "a:2"}],
        "owner": {"name": "An owner"},
    })
}

/// What the contract says about the passing payload with the value at `pointer` replaced.
fn broken(pointer: &str, value: Value) -> Option<(Rule, Option<String>, Option<Observed>)> {
    let mut payload = meeting();
    *payload
        .pointer_mut(pointer)
        .expect("a field of the payload") = value;
    violation(&CONTRACT, &payload).map(|detail| (detail.rule, detail.at, detail.observed))
}

#[test]
fn each_shape_rule_names_the_rule_the_item_and_what_it_found() {
    assert_eq!(
        (
            violation(&CONTRACT, &meeting()),
            broken("/title", Value::Null),
            broken("/title", json!(7)),
            broken("/title", json!("  ")),
            broken("/views", json!(0)),
            broken("/price", json!(-0.5)),
            broken("/open", json!("yes")),
            broken("/items", json!([{"ref": "a:1"}])),
            broken("/items/1", json!({"ref": ""})),
            broken("/items", json!([{"ref": "a:1"}, {"ref": "a:2"}])),
            broken("/owner", Value::Null),
            broken("/items/1", Value::Null),
        ),
        (
            None,
            Some((Rule::Present, None, Some(Observed::Null))),
            Some((Rule::Type, None, Some(Observed::Number))),
            Some((Rule::Nonblank, None, Some(Observed::String { length: 2 }))),
            Some((Rule::AtLeast, None, Some(Observed::Number))),
            Some((Rule::AtLeast, None, Some(Observed::Number))),
            Some((Rule::Type, None, Some(Observed::String { length: 3 }))),
            Some((Rule::MinItems, None, Some(Observed::Array { count: 1 }))),
            Some((
                Rule::Nonblank,
                Some("items[1].ref".to_owned()),
                Some(Observed::String { length: 0 })
            )),
            Some((
                Rule::InSomeItems,
                None,
                Some(Observed::Items {
                    with_field: 0,
                    count: 2
                })
            )),
            Some((
                Rule::Present,
                Some("owner".to_owned()),
                Some(Observed::Null)
            )),
            Some((
                Rule::Present,
                Some("items[1]".to_owned()),
                Some(Observed::Null)
            )),
        )
    );
}

#[test]
fn a_failure_detail_carries_no_value_of_the_output() {
    const MARKERS: [&str; 3] = ["MARKER-TITLE", "MARKER-URL", "MARKER-TEXT"];
    let payload = json!({
        "title": "MARKER-TITLE",
        "views": "MARKER-TEXT",
        "items": [{"ref": "MARKER-URL", "link": "https://example.com/MARKER-URL"}],
    });

    let detail = violation(&CONTRACT, &payload).expect("views is not a count");
    let serialized = format!(
        "{}{}",
        serde_json::to_string(&detail).expect("serialize the detail"),
        detail.describe()
    );

    assert_eq!(
        (
            detail.rule,
            MARKERS
                .iter()
                .filter(|marker| serialized.contains(*marker))
                .collect::<Vec<_>>()
        ),
        (Rule::Type, Vec::<&&str>::new()),
        "{serialized}"
    );
}

fn fetched(anonymous: &[bool]) -> Value {
    json!({
        "content": "Page body",
        "provider_attempts": anonymous
            .iter()
            .map(|anonymous| json!({"provider": "jina", "anonymous": anonymous, "error_kind": null}))
            .collect::<Vec<_>>(),
    })
}

#[test]
fn a_keyed_fetch_fails_when_any_attempt_was_anonymous_and_an_anonymous_one_when_any_sent_a_key() {
    const KEYED: LiveContract = LiveContract::fields(&[Field::text("content")])
        .with_sample(&[SampleRule::Credentials { anonymous: false }]);
    const ANONYMOUS: LiveContract = LiveContract::fields(&[Field::text("content")])
        .with_sample(&[SampleRule::Credentials { anonymous: true }]);
    let contract = |anonymous| if anonymous { ANONYMOUS } else { KEYED };
    let cases = [
        (false, &[false][..]),
        (false, &[false, true]),
        (true, &[true]),
        (true, &[false, true]),
        (true, &[]),
    ];

    assert_eq!(
        cases.map(
            |(anonymous, attempts)| violation(&contract(anonymous), &fetched(attempts))
                .map(|detail| (detail.rule, detail.path))
        ),
        [
            None,
            Some((Rule::KeyedRead, Some("provider_attempts[].anonymous"))),
            None,
            Some((Rule::AnonymousRead, Some("provider_attempts[].anonymous"))),
            Some((Rule::AnonymousRead, Some("provider_attempts"))),
        ]
    );
}

const WINNER: &str = "polymarket:winner";

/// A resolved event whose winning market is `winner`, read in `attempts` provider attempts.
fn resolved_event(winner: &Value, attempts: usize) -> Value {
    json!({
        "ref": "polymarket:event",
        "markets": [
            {
                "ref": "polymarket:loser",
                "status": "resolved",
                "outcomes": [{"name": "Yes", "price": 0.0, "basis": "resolution"}],
            },
            winner,
        ],
        "provider_attempts": vec![json!({"provider": "polymarket_api", "error_kind": null}); attempts],
    })
}

fn winner(status: &str, name: &str, price: f64, basis: &str) -> Value {
    json!({
        "ref": WINNER,
        "status": status,
        "outcomes": [
            {"name": name, "price": price, "basis": basis},
            {"name": "No", "price": 0.0, "basis": basis},
        ],
    })
}

#[test]
fn a_resolved_event_needs_one_attempt_and_its_winner_resolved_yes_at_one_by_resolution() {
    const CONTRACT: LiveContract =
        LiveContract::fields(&[]).with_sample(&[SampleRule::ResolvedByPayout { winner: WINNER }]);
    let contract = CONTRACT;
    let cases = [
        resolved_event(&winner("resolved", "Yes", 1.0, "resolution"), 1),
        resolved_event(&winner("resolved", "Yes", 1.0, "resolution"), 2),
        resolved_event(&winner("open", "Yes", 1.0, "resolution"), 1),
        resolved_event(&winner("resolved", "No", 1.0, "resolution"), 1),
        resolved_event(&winner("resolved", "Yes", 0.0, "resolution"), 1),
        resolved_event(&winner("resolved", "Yes", 1.0, "last_trade"), 1),
    ];

    assert_eq!(
        cases
            .iter()
            .map(|payload| violation(&contract, payload).map(|detail| detail
                .at
                .unwrap_or_else(|| detail.path.unwrap_or_default().to_owned())))
            .collect::<Vec<_>>(),
        [
            None,
            Some("provider_attempts".to_owned()),
            Some("markets[1].status".to_owned()),
            Some("markets[1].outcomes[0].name".to_owned()),
            Some("markets[1].outcomes[0].price".to_owned()),
            Some("markets[1].outcomes[0].basis".to_owned()),
        ]
    );
}

fn live_event(price_source: &str, statuses: &[&str]) -> Value {
    json!({
        "ref": "polymarket:event",
        "price_source": price_source,
        "markets": statuses
            .iter()
            .map(|status| json!({"status": status}))
            .collect::<Vec<_>>(),
    })
}

#[test]
fn a_live_event_needs_clob_prices_unless_none_of_its_markets_is_open() {
    const CONTRACT: LiveContract = LiveContract::fields(&[]).with_sample(&[SampleRule::LiveQuotes]);
    let contract = CONTRACT;
    let cases = [
        live_event("clob", &["open", "resolved"]),
        live_event("gamma", &["open", "resolved"]),
        live_event("gamma", &["resolved", "closed"]),
    ];

    assert_eq!(
        cases
            .iter()
            .map(|payload| violation(&contract, payload).map(|detail| detail.rule))
            .collect::<Vec<_>>(),
        [None, Some(Rule::LiveQuotes), None]
    );
}

#[test]
fn a_runtime_class_attempt_fails_a_contract_the_output_otherwise_meets() {
    const CONTRACT: LiveContract = LiveContract::fields(&[Field::text("answer")]);
    let contract = CONTRACT;
    let mut answered = json!({
        "answer": "Rust 1.91 is the latest stable release.",
        "provider_attempts": [{"provider": "xai", "error_kind": null}],
    });
    let holds = violation(&contract, &answered);
    answered["provider_attempts"]
        .as_array_mut()
        .expect("attempts")
        .insert(0, json!({"provider": "openai", "error_kind": "runtime"}));

    assert_eq!(
        (holds, violation(&contract, &answered)),
        (
            None,
            Some(Detail {
                rule: Rule::NoRuntimeAttempt,
                path: Some("provider_attempts[].error_kind"),
                at: Some("provider_attempts[0].error_kind".to_owned()),
                observed: Some(Observed::String { length: 7 }),
            })
        )
    );
}
