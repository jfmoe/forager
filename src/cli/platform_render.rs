//! Binary-side rendering of platform command results: search pages, fetched items,
//! comments, answers, model transcripts, Hacker News story lists, and Polymarket price histories.

use std::fmt::Write as _;

use forager::app::{DocsOutputFormat, OutputFormat, OutputTarget, ProviderError};
use forager::types::{
    ContentDepth, HnStoriesResult, ModelUsage, Platform, PlatformAskResult, PlatformCommentsPage,
    PlatformFetchResult, PlatformItem, PlatformItemData, PlatformSearchPage,
    PlatformTranscribeResult, PolymarketHistoryResult, PolymarketItemData, PolymarketWindow,
    ScholarItemData, XAskResult, XPostData, XiaohongshuCommentsPage, YoutubeAskResult,
    YoutubeCommentsPage,
};

use crate::{RenderedOutput, apply_tee, format_failure_json, postflight_exit_code};

/// Renders a platform command's result and tees it to `output`: `render` turns a success into
/// stdout and its diagnostic; a failure becomes the failure payload, or in Markdown the
/// `failed` heading with the error.
fn render_result<T>(
    result: Result<T, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
    failed: &str,
    render: impl FnOnce(T) -> Result<(String, Option<String>), String>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(value) => {
            let (stdout, diagnostic) = render(value)?;
            (stdout, 0, diagnostic)
        }
        Err(error) => {
            let stdout = match format {
                OutputFormat::Json => format_failure_json(&error)?,
                OutputFormat::Markdown => format!(
                    "# {failed}\n\n**{}**: {}",
                    error.kind.as_str(),
                    error.message
                ),
            };
            (stdout, postflight_exit_code(error.kind), error.diagnostic)
        }
    };
    apply_tee(
        stdout,
        exit_code,
        format == OutputFormat::Json,
        output,
        diagnostic,
    )
}

pub(crate) fn render_search(
    result: Result<PlatformSearchPage, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    render_result(result, format, output, "Platform search failed", |page| {
        Ok((format_platform_page(&page, format)?, page.diagnostic))
    })
}

fn format_platform_page(page: &PlatformSearchPage, format: OutputFormat) -> Result<String, String> {
    if format == OutputFormat::Json {
        return serde_json::to_string(page).map_err(|error| error.to_string());
    }
    let mut markdown = format!("# {} search ({})\n", page.platform, page.provider);
    for item in &page.items {
        let _ = match &item.title {
            Some(title) => write!(markdown, "\n- [{title}]({}) `{}`", item.url, item.reference),
            None => write!(markdown, "\n- <{}> `{}`", item.url, item.reference),
        };
        append_byline(&mut markdown, item);
        if let PlatformItemData::Polymarket(data) = &item.data {
            markdown.push_str(&polymarket_markdown(data).replace('\n', "\n  "));
        }
        let summary = item_abstract(item).or_else(|| item_snippet(item));
        if let Some(summary) = summary.filter(|summary| !summary.is_empty()) {
            let _ = write!(
                markdown,
                "\n\n  {}",
                summary.split_whitespace().collect::<Vec<_>>().join(" ")
            );
        }
    }
    if page.items.is_empty() {
        markdown.push_str("\n\nNo results.");
    }
    if let Some(cursor) = &page.next_cursor {
        let _ = write!(markdown, "\n\nNext cursor: `{cursor}`");
    }
    Ok(markdown)
}

pub(crate) fn render_stories(
    result: Result<HnStoriesResult, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    render_result(result, format, output, "HN stories failed", |listing| {
        Ok((format_stories(&listing, format)?, listing.diagnostic))
    })
}

fn format_stories(listing: &HnStoriesResult, format: OutputFormat) -> Result<String, String> {
    if format == OutputFormat::Json {
        return serde_json::to_string(listing).map_err(|error| error.to_string());
    }
    let mut markdown = format!(
        "# HN {} stories ({})\n",
        listing.list.as_str(),
        listing.provider
    );
    for ranked in &listing.items {
        let item = &ranked.item;
        let reference = item.reference.to_string();
        let _ = write!(
            markdown,
            "\n{}. [{}]({}) `{reference}`",
            ranked.rank,
            item.title.as_deref().unwrap_or(&reference),
            item.url
        );
        if let PlatformItemData::Hn(data) = &item.data {
            let _ = write!(
                markdown,
                " — {} points, {} comments",
                data.points.unwrap_or_default(),
                data.comments.unwrap_or_default()
            );
        }
        append_byline(&mut markdown, item);
    }
    if listing.items.is_empty() {
        markdown.push_str("\n\nNo stories.");
    }
    Ok(markdown)
}

pub(crate) fn render_history(
    result: Result<PolymarketHistoryResult, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    render_result(
        result,
        format,
        output,
        "Polymarket history failed",
        |history| Ok((format_history(&history, format)?, history.diagnostic)),
    )
}

fn format_history(
    history: &PolymarketHistoryResult,
    format: OutputFormat,
) -> Result<String, String> {
    if format == OutputFormat::Json {
        return serde_json::to_string(history).map_err(|error| error.to_string());
    }
    let market = &history.market;
    let window = match history.window {
        PolymarketWindow::Interval(interval) => format!("interval {}", interval.as_str()),
        PolymarketWindow::Range { from, to } => format!("{from} to {to}"),
    };
    let mut markdown = format!(
        "# Polymarket history ({})\n\n[{}]({}) `{}` — {}\n\n{} outcome, {window}, {}-second buckets\n",
        history.provider,
        market.question,
        market.url,
        market.reference,
        market.status.as_str(),
        history.outcome,
        history.bucket_seconds
    );
    if history.points.is_empty() {
        markdown.push_str("\nNo points.");
    } else {
        markdown.push_str("\n| t | price |\n|---|---|");
        for point in &history.points {
            let _ = write!(markdown, "\n| {} | {} |", point.t, point.price);
        }
    }
    if let Some(settlement) = &history.settlement {
        let _ = write!(
            markdown,
            "\n\nSettlement: {} at {}",
            settlement.price, settlement.t
        );
    }
    Ok(markdown)
}

fn append_byline(markdown: &mut String, item: &PlatformItem) {
    if !item.authors.is_empty() {
        let _ = write!(markdown, " — {}", item.authors.join(", "));
    }
    if let Some(published) = &item.published {
        let _ = write!(markdown, " — {published}");
    }
}

fn item_abstract(item: &PlatformItem) -> Option<&str> {
    match &item.data {
        PlatformItemData::Arxiv(data) => Some(&data.abstract_text),
        PlatformItemData::Ssrn(data) => data.abstract_text.as_deref(),
        PlatformItemData::Scholar(_)
        | PlatformItemData::Xiaohongshu(_)
        | PlatformItemData::XiaohongshuNote(_)
        | PlatformItemData::X(_)
        | PlatformItemData::Hn(_)
        | PlatformItemData::Polymarket(_) => None,
        PlatformItemData::Youtube(data) => data.description.as_deref(),
    }
}

fn item_snippet(item: &PlatformItem) -> Option<&str> {
    match &item.data {
        PlatformItemData::Arxiv(_)
        | PlatformItemData::Scholar(ScholarItemData::Cluster(_))
        | PlatformItemData::Xiaohongshu(_)
        | PlatformItemData::XiaohongshuNote(_)
        | PlatformItemData::Polymarket(_) => None,
        PlatformItemData::Ssrn(data) => data.snippet.as_deref(),
        PlatformItemData::Scholar(ScholarItemData::Result(data)) => data.snippet.as_deref(),
        PlatformItemData::X(data) => Some(&data.text),
        PlatformItemData::Hn(data) => data.text.as_deref(),
        PlatformItemData::Youtube(data) => data.snippet.as_deref(),
    }
}

/// Renders the prices of a Polymarket event or market as a list, one line per market or outcome,
/// then the rules a fetch at `abstract` depth inlined.
fn polymarket_markdown(data: &PolymarketItemData) -> String {
    let price = |value: Option<f64>| {
        value.map_or_else(|| "no price".to_owned(), |value| format!("{value}"))
    };
    let mut markdown = String::new();
    match data {
        PolymarketItemData::SearchEvent { event, top_markets } => {
            let _ = write!(
                markdown,
                "\n\n{} market(s), {}",
                event.market_count,
                event.status.as_str()
            );
            for market in top_markets {
                let _ = write!(
                    markdown,
                    "\n- {} — {} ({}, {}) `{}`",
                    market.label,
                    price(market.probability),
                    market.basis.as_str(),
                    market.status.as_str(),
                    market.reference
                );
            }
        }
        PolymarketItemData::Event {
            event,
            prices_as_of,
            markets,
            ..
        } => {
            let _ = write!(
                markdown,
                "\n\nPrices from {} as of {}",
                event.price_source.as_str(),
                prices_as_of.as_deref().unwrap_or("an unknown time")
            );
            for market in markets {
                let _ = write!(
                    markdown,
                    "\n- {} ({}) `{}`",
                    market.fields.label,
                    market.fields.status.as_str(),
                    market.reference
                );
                for outcome in &market.fields.outcomes {
                    let _ = write!(
                        markdown,
                        "\n  - {}: {} ({})",
                        outcome.name,
                        price(outcome.price),
                        outcome.basis.as_str()
                    );
                }
            }
        }
        PolymarketItemData::Market {
            market,
            price_source,
            prices_as_of,
            ..
        } => {
            let _ = write!(
                markdown,
                "\n\n{}; prices from {} as of {}",
                market.status.as_str(),
                price_source.as_str(),
                prices_as_of.as_deref().unwrap_or("an unknown time")
            );
            for outcome in &market.outcomes {
                let _ = write!(
                    markdown,
                    "\n- {}: {} ({})",
                    outcome.name,
                    price(outcome.price),
                    outcome.basis.as_str()
                );
            }
        }
    }
    if let Some(rules) = data.rules_document() {
        let _ = write!(markdown, "\n\n## Rules\n\n{rules}");
    }
    markdown
}

/// Renders an X post's thread as Markdown, one post per entry in the model's order.
fn x_thread_markdown(data: &XPostData) -> Option<String> {
    let thread = data.thread.as_ref()?;
    let mut markdown = String::new();
    for post in thread {
        let _ = write!(
            markdown,
            "{}- **@{}** ({}, {}) `{}`\n\n  {}\n",
            if markdown.is_empty() { "" } else { "\n" },
            post.author_handle,
            post.role.as_str(),
            post.published.as_deref().unwrap_or("undated"),
            post.reference,
            post.text.split_whitespace().collect::<Vec<_>>().join(" ")
        );
    }
    Some(markdown)
}

pub(crate) fn render_comments(
    result: Result<PlatformCommentsPage, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    render_result(result, format, output, "Comments failed", |page| {
        Ok((format_comments(&page, format)?, page.into_diagnostic()))
    })
}

fn format_comments(page: &PlatformCommentsPage, format: OutputFormat) -> Result<String, String> {
    if format == OutputFormat::Json {
        return serde_json::to_string(page).map_err(|error| error.to_string());
    }
    Ok(match page {
        PlatformCommentsPage::Xiaohongshu(page) => xiaohongshu_comments_markdown(page),
        PlatformCommentsPage::Youtube(page) => youtube_comments_markdown(page),
    })
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn xiaohongshu_comments_markdown(page: &XiaohongshuCommentsPage) -> String {
    let mut markdown = format!(
        "# {} comments on `{}` ({})\n",
        page.platform, page.note, page.provider
    );
    for comment in &page.comments {
        let _ = write!(
            markdown,
            "\n- **{}** ({}): {}",
            comment.author.as_deref().unwrap_or("unknown"),
            comment.published.as_deref().unwrap_or("undated"),
            one_line(&comment.text)
        );
        for reply in &comment.replies {
            let _ = write!(
                markdown,
                "\n  - **{}** ({}): {}",
                reply.author.as_deref().unwrap_or("unknown"),
                reply.published.as_deref().unwrap_or("undated"),
                one_line(&reply.text)
            );
        }
        if comment.replies_has_more {
            markdown.push_str("\n  - more replies not read");
        }
    }
    if page.comments.is_empty() {
        markdown.push_str("\n\nNo comments.");
    }
    if page.has_more {
        markdown.push_str("\n\nMore comments remain.");
    }
    markdown
}

fn youtube_comments_markdown(page: &YoutubeCommentsPage) -> String {
    let mut markdown = format!(
        "# {} comments on `{}` ({})\n",
        page.platform, page.video, page.provider
    );
    for comment in &page.comments {
        let _ = write!(
            markdown,
            "\n- **{}** ({}, {} likes): {}",
            comment.author.as_deref().unwrap_or("unknown"),
            comment.published.as_deref().unwrap_or("undated"),
            comment.likes.unwrap_or_default(),
            one_line(&comment.text)
        );
        for reply in &comment.replies {
            let _ = write!(
                markdown,
                "\n  - **{}** ({}): {}",
                reply.author.as_deref().unwrap_or("unknown"),
                reply.published.as_deref().unwrap_or("undated"),
                one_line(&reply.text)
            );
        }
        if !comment.replies_complete {
            let _ = write!(
                markdown,
                "\n  - {} of {} replies shown",
                comment.replies.len(),
                comment.reply_count
            );
        }
    }
    if page.comments.is_empty() {
        markdown.push_str("\n\nNo comments.");
    }
    if page.has_more {
        markdown.push_str("\n\nMore comments remain.");
    }
    markdown
}

pub(crate) fn render_fetch(
    result: Result<PlatformFetchResult, ProviderError>,
    format: DocsOutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(fetched) => (
            format_platform_fetch(&fetched, format)?,
            0,
            fetched.diagnostic,
        ),
        Err(error) => docs_failure(&error, format, "Platform fetch failed")?,
    };
    apply_tee(
        stdout,
        exit_code,
        format == DocsOutputFormat::Json,
        output,
        diagnostic,
    )
}

/// Renders a model transcript like a fetched full text; JSON adds the model and its usage, and
/// Markdown names them under the reference to the written file.
pub(crate) fn render_transcribe(
    result: Result<PlatformTranscribeResult, ProviderError>,
    format: DocsOutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(transcribed) => {
            let stdout = match format {
                DocsOutputFormat::Json => {
                    serde_json::to_string(&transcribed).map_err(|error| error.to_string())?
                }
                DocsOutputFormat::Content => format_platform_fetch(&transcribed.fetched, format)?,
                DocsOutputFormat::Markdown => {
                    let mut markdown = format_platform_fetch(&transcribed.fetched, format)?;
                    let _ = write!(
                        markdown,
                        "\n\nGenerated by {}{}",
                        transcribed
                            .model
                            .as_deref()
                            .unwrap_or("an unreported model"),
                        usage_note(transcribed.usage.as_ref())
                    );
                    markdown
                }
            };
            (stdout, 0, transcribed.fetched.diagnostic)
        }
        Err(error) => docs_failure(&error, format, "YouTube transcribe failed")?,
    };
    apply_tee(
        stdout,
        exit_code,
        format == DocsOutputFormat::Json,
        output,
        diagnostic,
    )
}

/// Renders a failure of a command whose output is a document, with `title` heading Markdown.
fn docs_failure(
    error: &ProviderError,
    format: DocsOutputFormat,
    title: &str,
) -> Result<(String, u8, Option<String>), String> {
    let stdout = match format {
        DocsOutputFormat::Json => format_failure_json(error)?,
        DocsOutputFormat::Markdown | DocsOutputFormat::Content => format!(
            "# {title}\n\n**{}**: {}",
            error.kind.as_str(),
            error.message
        ),
    };
    Ok((
        stdout,
        postflight_exit_code(error.kind),
        error.diagnostic.clone(),
    ))
}

/// Says how many tokens a model request used, when the service reported it.
fn usage_note(usage: Option<&ModelUsage>) -> String {
    usage
        .and_then(|usage| usage.total_tokens)
        .map(|total| format!(" ({total} tokens)"))
        .unwrap_or_default()
}

/// Renders a fetched item. A full text is printed only by `content`; the other formats reference
/// the written file instead. Shorter content, such as an X post, is printed inline.
fn format_platform_fetch(
    fetched: &PlatformFetchResult,
    format: DocsOutputFormat,
) -> Result<String, String> {
    let item = &fetched.item;
    match format {
        DocsOutputFormat::Json => serde_json::to_string(fetched).map_err(|error| error.to_string()),
        DocsOutputFormat::Content => Ok(match (&fetched.content, &item.data) {
            (Some(content), _) => content.text.clone(),
            (None, PlatformItemData::X(data)) => {
                x_thread_markdown(data).unwrap_or_else(|| data.text.clone())
            }
            (None, PlatformItemData::Hn(data)) => data.text.clone().unwrap_or_default(),
            (None, PlatformItemData::Polymarket(data)) => data.rules_document().unwrap_or_default(),
            (None, _) => item_abstract(item).unwrap_or_default().to_owned(),
        }),
        DocsOutputFormat::Markdown => {
            let reference = item.reference.to_string();
            let mut markdown = format!(
                "# {}\n\n`{}` — <{}> ({}, {})",
                item.title.as_deref().unwrap_or(&reference),
                item.reference,
                item.url,
                fetched.provider,
                item.depth.as_str()
            );
            append_byline(&mut markdown, item);
            if let Some(summary) = item_abstract(item) {
                let _ = write!(markdown, "\n\n## Abstract\n\n{summary}");
            }
            if let PlatformItemData::X(data) = &item.data {
                let _ = write!(markdown, "\n\n{}", data.text);
                if let Some(thread) = x_thread_markdown(data) {
                    let _ = write!(markdown, "\n\n## Thread\n\n{thread}");
                }
            }
            if let PlatformItemData::Hn(data) = &item.data
                && fetched.content.is_none()
                && let Some(text) = &data.text
            {
                let _ = write!(markdown, "\n\n{text}");
            }
            if let PlatformItemData::Polymarket(data) = &item.data {
                markdown.push_str(&polymarket_markdown(data));
            }
            if let Some(content) = &fetched.content {
                let section = if item.depth == ContentDepth::Thread {
                    "Thread"
                } else {
                    "Full text"
                };
                let _ = write!(
                    markdown,
                    "\n\n## {section}\n\n{} characters from <{}> via {}: `{}`",
                    content.len,
                    content.url,
                    content.provider,
                    content.path.as_deref().unwrap_or_default()
                );
                if let Some(pdf_path) = &content.pdf_path {
                    let _ = write!(
                        markdown,
                        "\n\nOriginal PDF ({} bytes): `{pdf_path}`",
                        content.pdf_bytes.unwrap_or_default()
                    );
                }
            }
            Ok(markdown)
        }
    }
}

pub(crate) fn render_ask(
    platform: Platform,
    result: Result<PlatformAskResult, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let failed = format!("{} ask failed", platform_title(platform));
    render_result(result, format, output, &failed, |answer| {
        Ok((format_ask(&answer, format)?, answer.into_diagnostic()))
    })
}

/// Returns the name a Markdown heading gives the platform.
fn platform_title(platform: Platform) -> &'static str {
    match platform {
        Platform::X => "X",
        Platform::Youtube => "YouTube",
        other => other.as_str(),
    }
}

fn format_ask(answer: &PlatformAskResult, format: OutputFormat) -> Result<String, String> {
    if format == OutputFormat::Json {
        return serde_json::to_string(answer).map_err(|error| error.to_string());
    }
    Ok(match answer {
        PlatformAskResult::X(answer) => x_answer_markdown(answer),
        PlatformAskResult::Youtube(answer) => youtube_answer_markdown(answer),
    })
}

fn youtube_answer_markdown(answer: &YoutubeAskResult) -> String {
    let video = &answer.video;
    let reference = video.reference.to_string();
    format!(
        "# YouTube answer ({})\n\n[{}]({}) `{reference}`\n\n**Question:** {}\n\n{}\n\nRelayed by {}{}; check each moment in the video before quoting it.",
        answer.provider,
        video.title.as_deref().unwrap_or(&reference),
        video.url,
        answer.question,
        answer.answer,
        answer.model.as_deref().unwrap_or("an unreported model"),
        usage_note(answer.usage.as_ref())
    )
}

fn x_answer_markdown(answer: &XAskResult) -> String {
    let mut markdown = format!("# X answer ({})\n\n{}", answer.provider, answer.answer);
    if !answer.sources.is_empty() {
        markdown.push_str("\n\n## Sources\n");
        for source in &answer.sources {
            let title = if source.title.is_empty() {
                &source.url
            } else {
                &source.title
            };
            let _ = write!(markdown, "\n- [{title}]({})", source.url);
            if let Some(reference) = &source.reference {
                let _ = write!(markdown, " `{reference}`");
            }
        }
    }
    markdown
}
