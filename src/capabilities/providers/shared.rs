use crate::catalog::ProviderId;
use crate::credentials::CredentialPool;
use crate::net::{AttemptFailure, truncate_message};
use crate::rate_limit::{RateLimiter, RatePermit};
use crate::redact::{redact_url, redact_urls as redact_urls_in_text};
use crate::types::{AttemptErrorKind, Deadline, Platform, ProviderError, Source};

/// Waits for the route's request window; a pacing failure ends the attempt before any send.
pub(super) async fn acquire_window(
    limiter: &RateLimiter,
    deadline: Deadline,
) -> Result<RatePermit, AttemptFailure> {
    limiter
        .acquire(deadline)
        .await
        .map_err(|error| AttemptFailure {
            kind: error.kind(),
            status: None,
            message: error.to_string(),
        })
}

/// Returns a platform route's refusal of a request that belongs to another platform.
pub(super) fn other_platform_message(route: ProviderId, platform: Platform) -> String {
    format!("{} cannot serve {platform} requests", route.name())
}

/// Returns the failure of a request that a route rejects before sending it.
pub(super) fn parameter_error(message: String) -> ProviderError {
    ProviderError {
        kind: AttemptErrorKind::Parameter,
        message,
        attempts: Vec::new(),
        verbose: false,
        diagnostic: None,
        redirected_library_id: None,
    }
}

pub(super) fn normalize_main_search(
    answer: &str,
    sources: Vec<Source>,
    credentials: &CredentialPool,
) -> Result<(String, Vec<Source>), AttemptFailure> {
    let answer = remove_think_blocks(answer);
    let mut text_sources = extract_inline_bindings(&answer);
    let (answer, mut trailing_sources) = extract_trailing_source_block(answer.trim());
    text_sources.append(&mut trailing_sources);
    let answer = answer.trim();
    if answer.is_empty() {
        return Err(AttemptFailure {
            kind: AttemptErrorKind::Runtime,
            status: Some(200),
            message: "main search response contained no answer after normalization".into(),
        });
    }
    let mut sources = sources;
    for source in &mut sources {
        if is_citation_ordinal(&source.title) {
            source.title.clear();
        }
    }
    sources.extend(text_sources);
    let sources = redact_and_deduplicate_sources(sources, credentials);
    Ok((answer.to_owned(), sources))
}

// xAI url_citation annotations carry the citation number as the title.
fn is_citation_ordinal(title: &str) -> bool {
    let title = title.trim();
    !title.is_empty() && title.chars().all(|character| character.is_ascii_digit())
}

pub(super) fn redact_and_deduplicate_sources(
    sources: Vec<Source>,
    credentials: &CredentialPool,
) -> Vec<Source> {
    let mut normalized = Vec::new();
    for mut source in sources {
        source.url = credentials.redact(&redact_url(&source.url));
        source.title = redact_urls(&source.title, credentials);
        if !normalized
            .iter()
            .any(|existing: &Source| existing.url == source.url)
        {
            normalized.push(source);
        }
    }
    normalized
}

fn remove_think_blocks(answer: &str) -> String {
    let lowercase = answer.to_ascii_lowercase();
    let mut cleaned = String::with_capacity(answer.len());
    let mut cursor = 0;
    while let Some(open_offset) = lowercase[cursor..].find("<think>") {
        let open = cursor + open_offset;
        let content_start = open + "<think>".len();
        let Some(close_offset) = lowercase[content_start..].find("</think>") else {
            break;
        };
        let close = content_start + close_offset + "</think>".len();
        cleaned.push_str(&answer[cursor..open]);
        cursor = close;
    }
    cleaned.push_str(&answer[cursor..]);
    cleaned
}

fn extract_trailing_source_block(answer: &str) -> (String, Vec<Source>) {
    let mut line_starts = vec![0];
    line_starts.extend(
        answer
            .match_indices('\n')
            .map(|(newline, _)| newline + '\n'.len_utf8()),
    );
    let Some(heading_start) = line_starts.into_iter().rev().find(|start| {
        let line_end = answer[*start..]
            .find('\n')
            .map_or(answer.len(), |offset| *start + offset);
        is_source_heading(&answer[*start..line_end])
    }) else {
        return (answer.to_owned(), Vec::new());
    };
    let block_start = answer[heading_start..]
        .find('\n')
        .map_or(answer.len(), |offset| heading_start + offset + 1);
    let Some(sources) = extract_source_block(&answer[block_start..]) else {
        return (answer.to_owned(), Vec::new());
    };
    (answer[..heading_start].trim_end().to_owned(), sources)
}

fn extract_source_block(block: &str) -> Option<Vec<Source>> {
    if block.lines().any(is_markdown_heading) {
        return None;
    }
    let sources = extract_link_sources(block);
    (!sources.is_empty()).then_some(sources)
}

fn is_markdown_heading(line: &str) -> bool {
    let line = line.trim_start();
    let marker_len = line.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&marker_len)
        && line[marker_len..]
            .chars()
            .next()
            .is_some_and(char::is_whitespace)
}

fn is_source_heading(line: &str) -> bool {
    let heading = line
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_end_matches([':', '：'])
        .trim();
    ["Sources", "References", "Citations"]
        .iter()
        .any(|candidate| heading.eq_ignore_ascii_case(candidate))
        || matches!(heading, "来源" | "参考资料" | "引用")
}

fn extract_inline_bindings(answer: &str) -> Vec<Source> {
    let mut sources = Vec::new();
    let mut cursor = 0;
    while let Some(open_offset) = answer[cursor..].find("[[") {
        let open = cursor + open_offset;
        let number_start = open + "[[".len();
        let Some(number_end_offset) = answer[number_start..].find("]](") else {
            cursor = number_start;
            continue;
        };
        let number_end = number_start + number_end_offset;
        let number = &answer[number_start..number_end];
        let url_start = number_end + "]](".len();
        let Some(url_end) = delimited_url_end(answer, url_start, |_| false) else {
            cursor = url_start;
            continue;
        };
        let url = answer[url_start..url_end].trim();
        if !number.is_empty()
            && number.chars().all(|character| character.is_ascii_digit())
            && valid_http_url(url)
        {
            sources.push(source("", url));
        }
        cursor = url_end + 1;
    }
    sources
}

// A link destination can contain balanced parentheses (Wikipedia-style URLs), so
// only a terminator or a `)` at depth zero ends it.
fn delimited_url_end(
    text: &str,
    start: usize,
    is_terminator: impl Fn(char) -> bool,
) -> Option<usize> {
    let mut depth = 0usize;
    for (offset, character) in text[start..].char_indices() {
        if is_terminator(character) {
            return Some(start + offset);
        }
        match character {
            '(' => depth += 1,
            ')' => match depth.checked_sub(1) {
                Some(inner) => depth = inner,
                None => return Some(start + offset),
            },
            _ => {}
        }
    }
    None
}

fn extract_link_sources(block: &str) -> Vec<Source> {
    let mut sources = Vec::new();
    let mut markdown_urls = Vec::new();
    let mut cursor = 0;
    while let Some(open_offset) = block[cursor..].find('[') {
        let open = cursor + open_offset;
        let Some(label_end_offset) = block[open + 1..].find("](") else {
            cursor = open + 1;
            continue;
        };
        let label_end = open + 1 + label_end_offset;
        let url_start = label_end + "](".len();
        let Some(url_end) = delimited_url_end(block, url_start, |_| false) else {
            cursor = url_start;
            continue;
        };
        let url = block[url_start..url_end].trim();
        if valid_http_url(url) {
            sources.push((open, source(&block[open + 1..label_end], url)));
            markdown_urls.push(url_start..url_end);
        }
        cursor = url_end + 1;
    }
    for scheme in ["http://", "https://"] {
        let mut cursor = 0;
        while let Some(offset) = block[cursor..].find(scheme) {
            let start = cursor + offset;
            cursor = start + scheme.len();
            if markdown_urls
                .iter()
                .any(|range| range.start <= start && start < range.end)
            {
                continue;
            }
            let end = delimited_url_end(block, start, |character: char| {
                character.is_whitespace() || matches!(character, ']' | '>' | '"' | '\'')
            })
            .unwrap_or(block.len());
            let url = block[start..end].trim_end_matches([
                '.', ',', ';', ':', '!', '?', '，', '。', '；', '：', '！', '？',
            ]);
            if valid_http_url(url) {
                sources.push((start, source("", url)));
            }
        }
    }
    sources.sort_by_key(|(position, _)| *position);
    sources.into_iter().map(|(_, source)| source).collect()
}

fn valid_http_url(value: &str) -> bool {
    reqwest::Url::parse(value)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
}

fn source(title: &str, url: &str) -> Source {
    Source {
        title: title.trim().to_owned(),
        url: url.to_owned(),
        published_date: None,
        author: None,
        text: None,
        highlights: Vec::new(),
        id: None,
        image: None,
        favicon: None,
    }
}

pub(crate) fn redacted_urls_message(message: &str, credentials: &CredentialPool) -> String {
    truncate_message(&redact_urls(message, credentials))
}

pub(super) fn redact_urls(message: &str, credentials: &CredentialPool) -> String {
    credentials.redact(&redact_urls_in_text(message))
}

#[cfg(test)]
mod tests {
    use crate::credentials::CredentialPool;

    use super::{normalize_main_search, redacted_urls_message, source};

    #[test]
    fn main_search_normalizer_removes_complete_think_blocks() {
        let credentials = CredentialPool::new("test", vec![]);

        let Ok((answer, sources)) = normalize_main_search(
            "Before\n<THINK>private\nreasoning</think>\nMiddle\n<think>more</THINK>\nAfter",
            Vec::new(),
            &credentials,
        ) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(answer, "Before\n\nMiddle\n\nAfter");
        assert!(sources.is_empty());
    }

    #[test]
    fn main_search_normalizer_drops_citation_ordinal_titles() {
        let credentials = CredentialPool::new("test", vec![]);
        let sources = vec![
            source("1", "https://example.test/one"),
            source("Rust 2024", "https://example.test/two"),
        ];

        let Ok((_, sources)) = normalize_main_search("Answer", sources, &credentials) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(
            sources
                .iter()
                .map(|source| source.title.as_str())
                .collect::<Vec<_>>(),
            vec!["", "Rust 2024"]
        );
    }

    #[test]
    fn main_search_normalizer_projects_explicit_trailing_source_blocks() {
        let credentials = CredentialPool::new("test", vec![]);
        for (input, expected_answer, expected_sources) in [
            (
                "Answer\n\n## Sources\n- [Rust](https://rust-lang.org/)\n- https://example.test/docs",
                "Answer",
                vec![
                    ("Rust", "https://rust-lang.org/"),
                    ("", "https://example.test/docs"),
                ],
            ),
            (
                "答案\n\n参考资料：\n[资料](http://example.cn/ref)",
                "答案",
                vec![("资料", "http://example.cn/ref")],
            ),
            (
                "答案\n\n来源\n- https://example.cn/source",
                "答案",
                vec![("", "https://example.cn/source")],
            ),
            (
                "答案\n\n引用\n- https://example.cn/citation",
                "答案",
                vec![("", "https://example.cn/citation")],
            ),
            (
                "Keep this\n\nCitations:\n- ftp://example.test/invalid",
                "Keep this\n\nCitations:\n- ftp://example.test/invalid",
                vec![],
            ),
            (
                "Keep this\n\nSources:\n- https://example.test/source\n  Source description.",
                "Keep this",
                vec![("", "https://example.test/source")],
            ),
            (
                "Keep this\n\nSources:\n- https://example.test/source\n\n## Continued answer\nText.",
                "Keep this\n\nSources:\n- https://example.test/source\n\n## Continued answer\nText.",
                vec![],
            ),
        ] {
            let Ok((answer, sources)) = normalize_main_search(input, Vec::new(), &credentials)
            else {
                panic!("answer should normalize successfully");
            };

            assert_eq!(answer, expected_answer, "input={input}");
            assert_eq!(
                sources
                    .iter()
                    .map(|source| (source.title.as_str(), source.url.as_str()))
                    .collect::<Vec<_>>(),
                expected_sources,
                "input={input}"
            );
        }
    }

    #[test]
    fn main_search_normalizer_keeps_inline_bindings_and_orders_unique_text_sources() {
        let credentials = CredentialPool::new("test", vec![]);
        let input = concat!(
            "Claim [[1]](https://example.test/structured) and ",
            "[[2]](https://example.test/inline).\n\n",
            "References\n",
            "- [Tail](https://example.test/tail)"
        );

        let Ok((answer, sources)) = normalize_main_search(
            input,
            vec![source("Structured", "https://example.test/structured")],
            &credentials,
        ) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(
            answer,
            concat!(
                "Claim [[1]](https://example.test/structured) and ",
                "[[2]](https://example.test/inline)."
            )
        );
        assert_eq!(
            sources
                .iter()
                .map(|source| (source.title.as_str(), source.url.as_str()))
                .collect::<Vec<_>>(),
            [
                ("Structured", "https://example.test/structured"),
                ("", "https://example.test/inline"),
                ("Tail", "https://example.test/tail"),
            ]
        );
    }

    #[test]
    fn main_search_normalizer_does_not_truncate_markdown_source_urls_at_balanced_parentheses() {
        let credentials = CredentialPool::new("test", vec![]);
        let input = concat!(
            "Answer\n\n## Sources\n",
            "- [Rust (programming language)](https://en.wikipedia.org/wiki/Rust_(programming_language))"
        );

        let Ok((_, sources)) = normalize_main_search(
            input,
            vec![source(
                "",
                "https://en.wikipedia.org/wiki/Rust_(programming_language)",
            )],
            &credentials,
        ) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(
            sources
                .iter()
                .map(|source| (source.title.as_str(), source.url.as_str()))
                .collect::<Vec<_>>(),
            [(
                "",
                "https://en.wikipedia.org/wiki/Rust_(programming_language)"
            )]
        );
    }

    #[test]
    fn main_search_normalizer_does_not_truncate_inline_binding_urls_at_balanced_parentheses() {
        let credentials = CredentialPool::new("test", vec![]);
        let input =
            "Claim [[1]](https://en.wikipedia.org/wiki/Rust_(programming_language)) and text.";

        let Ok((answer, sources)) = normalize_main_search(input, Vec::new(), &credentials) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(answer, input);
        assert_eq!(
            sources
                .iter()
                .map(|source| source.url.as_str())
                .collect::<Vec<_>>(),
            ["https://en.wikipedia.org/wiki/Rust_(programming_language)"]
        );
    }

    #[test]
    fn main_search_normalizer_does_not_truncate_bare_source_urls_at_balanced_parentheses() {
        let credentials = CredentialPool::new("test", vec![]);
        let input =
            "Answer\n\n## Sources\n- https://en.wikipedia.org/wiki/Rust_(programming_language)";

        let Ok((_, sources)) = normalize_main_search(input, Vec::new(), &credentials) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(
            sources
                .iter()
                .map(|source| source.url.as_str())
                .collect::<Vec<_>>(),
            ["https://en.wikipedia.org/wiki/Rust_(programming_language)"]
        );
    }

    #[test]
    fn main_search_normalizer_ends_bare_source_urls_at_unbalanced_close_parenthesis() {
        let credentials = CredentialPool::new("test", vec![]);
        let input = "Answer\n\n## Sources\n- (https://example.test/rust)";

        let Ok((_, sources)) = normalize_main_search(input, Vec::new(), &credentials) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(
            sources
                .iter()
                .map(|source| source.url.as_str())
                .collect::<Vec<_>>(),
            ["https://example.test/rust"]
        );
    }

    #[test]
    fn main_search_normalizer_redacts_then_deduplicates_public_source_urls() {
        let credentials = CredentialPool::new(
            "test",
            vec!["credential-one".into(), "credential-two".into()],
        );
        let urls = [
            "https://example.test/a?view=one",
            "https://example.test/a?view=one",
            "https://example.test/a?view=two",
            "https://example.test/a?view=one#fragment",
            "https://example.test/a?token=first&view=one",
            "https://example.test/a?token=second&view=one",
            "https://example.test/credential-one/path",
            "https://example.test/credential-two/path",
        ];
        let sources = urls
            .into_iter()
            .enumerate()
            .map(|(index, url)| {
                source(
                    if index == 0 {
                        "credential-one at https://example.test/title?token=secret"
                    } else {
                        ""
                    },
                    url,
                )
            })
            .collect();

        let Ok((_, sources)) = normalize_main_search("Answer", sources, &credentials) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(
            sources
                .iter()
                .map(|source| (source.title.as_str(), source.url.as_str()))
                .collect::<Vec<_>>(),
            [
                (
                    "******** at https://example.test/title?token=********",
                    "https://example.test/a?view=one",
                ),
                ("", "https://example.test/a?view=two"),
                ("", "https://example.test/a?token=********&view=one"),
                ("", "https://example.test/********/path"),
            ]
        );
    }

    #[test]
    fn main_search_normalizer_preserves_unclosed_think_and_policy_discussion() {
        let credentials = CredentialPool::new("test", vec![]);
        for input in [
            "Before <think>unfinished reasoning",
            "System policy and prompt injection defenses are the subject of this answer.",
        ] {
            let Ok((answer, sources)) = normalize_main_search(input, Vec::new(), &credentials)
            else {
                panic!("answer should normalize successfully");
            };
            assert_eq!(answer, input);
            assert!(sources.is_empty());
        }
    }

    #[test]
    fn main_search_normalizer_cleans_think_before_projecting_sources() {
        let credentials = CredentialPool::new("test", vec![]);
        let input = concat!(
            "<think>Hidden [[1]](https://hidden.example/path)</think>\n",
            "Answer [[2]](https://visible.example/path)"
        );

        let Ok((answer, sources)) = normalize_main_search(input, Vec::new(), &credentials) else {
            panic!("answer should normalize successfully");
        };

        assert_eq!(answer, "Answer [[2]](https://visible.example/path)");
        assert_eq!(
            sources
                .iter()
                .map(|source| source.url.as_str())
                .collect::<Vec<_>>(),
            ["https://visible.example/path"]
        );
    }

    #[test]
    fn main_search_normalizer_returns_runtime_when_cleanup_removes_the_answer() {
        let credentials = CredentialPool::new("test", vec![]);
        for input in [
            "<think>Only hidden reasoning</think>",
            "Sources:\n- https://example.test/source",
        ] {
            let Err(error) = normalize_main_search(input, Vec::new(), &credentials) else {
                panic!("empty normalized answer should fail");
            };
            assert_eq!(error.kind, crate::types::AttemptErrorKind::Runtime);
        }
    }

    #[test]
    fn main_search_normalizer_ignores_unsupported_text_source_shapes() {
        let credentials = CredentialPool::new("test", vec![]);
        for input in [
            "Answer\n\nsources([{\"url\":\"https://example.test/source\"}])",
            "Answer\n\n<details>https://example.test/source</details>",
            "Answer\n\n[One](https://example.test/one)\n[Two](https://example.test/two)",
            "Answer [[x]](https://example.test/invalid)",
        ] {
            let Ok((answer, sources)) = normalize_main_search(input, Vec::new(), &credentials)
            else {
                panic!("answer should normalize successfully");
            };
            assert_eq!(answer, input);
            assert!(sources.is_empty(), "input={input}");
        }
    }

    #[test]
    fn redacted_urls_message_masks_credentials_and_endpoint_query_secrets() {
        let credentials = CredentialPool::new("test", vec!["credential-secret".into()]);
        let endpoint = "https://example.test/search?api_key=endpoint-secret";

        assert_eq!(
            redacted_urls_message(
                &format!("request to {endpoint} failed with credential-secret"),
                &credentials,
            ),
            "request to https://example.test/search?api_key=******** failed with ********"
        );
    }
}
