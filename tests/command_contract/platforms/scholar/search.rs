use std::process::Output;

use serde_json::json;

use crate::support::scholar::{config, fixture};
use crate::support::{Fixture, Response, RunEnvironment, ok_json, payload, stderr};

const KEY: &str = "test-serpapi-key-a";

fn search(environment: &RunEnvironment, arguments: &[&str]) -> Output {
    let mut command = vec!["platform", "scholar", "search"];
    command.extend_from_slice(arguments);
    environment.run(&command)
}

/// Runs one search against a fixture that answers in order and returns every request.
fn run(keys: &[&str], arguments: &[&str], responses: Vec<Response>) -> (Output, Vec<String>) {
    let fixture = Fixture::start_sequence(responses);
    let environment = RunEnvironment::new(&config(&fixture.url, keys));
    let output = search(&environment, arguments);
    (output, fixture.finish_all())
}

#[test]
fn search_decodes_results_with_cluster_refs_and_metadata() {
    let source = fixture("search.json");
    let snippet = |index: usize| source["organic_results"][index]["snippet"].clone();
    let (output, _) = run(&[KEY], &["momentum"], vec![ok_json(&source)]);
    let mut page = payload(&output);
    let items = page["items"].as_array_mut().expect("items").split_off(4);
    let next_cursor = page
        .as_object_mut()
        .expect("page")
        .remove("next_cursor")
        .expect("next_cursor");

    assert_eq!(
        (page, items.len(), next_cursor.is_string()),
        (
            json!({
                "platform": "scholar",
                "provider": "serpapi",
                "items": [
                    {
                        "ref": "scholar:18208131694456651388",
                        "url": "https://scholar.google.com/scholar?cluster=18208131694456651388",
                        "depth": "snippet",
                        "title": "Time series momentum",
                        "authors": ["TJ Moskowitz", "YH Ooi", "LH Pedersen"],
                        "published": "2012",
                        "snippet": snippet(0),
                        "link": "https://www.sciencedirect.com/science/article/pii/S0304405X11002613",
                        "source": "TJ Moskowitz, YH Ooi, LH Pedersen - Journal of financial economics, 2012 - Elsevier",
                        "cited_by": 2307,
                        "version_count": 45,
                        "resources": [{
                            "title": "sciencedirect.com",
                            "file_format": "HTML",
                            "url": "https://www.sciencedirect.com/science/article/pii/S0304405X11002613"
                        }],
                        "result_type": "Html"
                    },
                    {
                        "ref": "scholar:10743434366743644145",
                        "url": "https://scholar.google.com/scholar?cluster=10743434366743644145",
                        "depth": "snippet",
                        "title": "Enhancing time series momentum strategies using deep neural networks",
                        "authors": ["B Lim", "S Zohren", "S Roberts"],
                        "published": "2019",
                        "snippet": snippet(1),
                        "link": "https://arxiv.org/abs/1904.04912",
                        "source": "B Lim, S Zohren, S Roberts - arXiv preprint arXiv:1904.04912, 2019 - arxiv.org",
                        "cited_by": 151,
                        "version_count": 18,
                        "resources": [{
                            "title": "arxiv.org",
                            "file_format": "PDF",
                            "url": "https://arxiv.org/pdf/1904.04912"
                        }],
                        "result_type": null
                    },
                    {
                        // No explicit ID: the ref comes from `result_id`, and SerpApi answered a
                        // cluster lookup for this ID with this paper.
                        "ref": "scholar:2175834747627124651",
                        "url": "https://scholar.google.com/scholar?cluster=2175834747627124651",
                        "depth": "snippet",
                        "title": "When Does Risk-Managed Momentum Add Value? An Out-of-Sample, Regime-Conditional, and Statistically Tested Study of a Volatility-Targeted Dual-Momentum …",
                        "authors": ["A Goyal"],
                        "published": "2026",
                        "snippet": snippet(2),
                        "link": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=7143882",
                        "source": "A Goyal - … Volatility-Targeted Dual-Momentum Strategy (June 15 …, 2026 - papers.ssrn.com",
                        "cited_by": null,
                        "version_count": null,
                        "resources": [{
                            "title": "ssrn.com",
                            "file_format": "PDF",
                            "url": "https://papers.ssrn.com/sol3/Delivery.cfm?abstractid=7143882"
                        }],
                        "result_type": null
                    },
                    {
                        "ref": "scholar:15167737520660287132",
                        "url": "https://scholar.google.com/scholar?cluster=15167737520660287132",
                        "depth": "snippet",
                        "title": "Resilience Anomaly and Asset Pricing in the Chinese Stock Market",
                        "authors": ["J Hu", "X Li", "T Wang"],
                        "published": null,
                        "snippet": snippet(3),
                        "link": "https://papers.ssrn.com/sol3/papers.cfm?abstract_id=7506942",
                        "source": "J Hu, X Li, T Wang - papers.ssrn.com",
                        "cited_by": null,
                        "version_count": null,
                        "resources": [{
                            "title": "ssrn.com",
                            "file_format": "PDF",
                            "url": "https://papers.ssrn.com/sol3/Delivery.cfm?abstractid=7506942"
                        }],
                        "result_type": null
                    }
                ]
            }),
            4,
            true
        )
    );
}

#[test]
fn an_empty_key_pool_fails_before_any_request_and_names_the_keys_setting() {
    let fixture = Fixture::start_canary();
    let environment = RunEnvironment::new(&config(&fixture.url, &[]));

    let output = search(&environment, &["momentum"]);

    assert_eq!(
        (
            output.status.code(),
            stderr(&output).contains("providers.serpapi.keys"),
            fixture.finish_all().len()
        ),
        (Some(3), true, 0),
        "stderr: {}",
        stderr(&output)
    );
}
