//! Binary-side rendering of platform command results: search pages, fetched items,
//! Xiaohongshu comments, X answers, and Hacker News story lists.

use std::fmt::Write as _;

use forager::app::{DocsOutputFormat, OutputFormat, OutputTarget, ProviderError};
use forager::types::{
    ContentDepth, HnStoriesResult, PlatformFetchResult, PlatformItem, PlatformItemData,
    PlatformSearchPage, ScholarItemData, XAskResult, XPostData, XiaohongshuCommentsPage,
};

use crate::{RenderedOutput, apply_tee, format_failure_json, postflight_exit_code};

pub(crate) fn render_search(
    result: Result<PlatformSearchPage, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(page) => (format_platform_page(&page, format)?, 0, page.diagnostic),
        Err(error) => {
            let stdout = match format {
                OutputFormat::Json => format_failure_json(&error)?,
                OutputFormat::Markdown => format!(
                    "# Platform search failed\n\n**{}**: {}",
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
    let (stdout, exit_code, diagnostic) = match result {
        Ok(listing) => (format_stories(&listing, format)?, 0, listing.diagnostic),
        Err(error) => {
            let stdout = match format {
                OutputFormat::Json => format_failure_json(&error)?,
                OutputFormat::Markdown => format!(
                    "# HN stories failed\n\n**{}**: {}",
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
        | PlatformItemData::Hn(_) => None,
        PlatformItemData::Youtube(data) => data.description.as_deref(),
    }
}

fn item_snippet(item: &PlatformItem) -> Option<&str> {
    match &item.data {
        PlatformItemData::Arxiv(_)
        | PlatformItemData::Scholar(ScholarItemData::Cluster(_))
        | PlatformItemData::Xiaohongshu(_)
        | PlatformItemData::XiaohongshuNote(_) => None,
        PlatformItemData::Ssrn(data) => data.snippet.as_deref(),
        PlatformItemData::Scholar(ScholarItemData::Result(data)) => data.snippet.as_deref(),
        PlatformItemData::X(data) => Some(&data.text),
        PlatformItemData::Hn(data) => data.text.as_deref(),
        PlatformItemData::Youtube(data) => data.snippet.as_deref(),
    }
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
    result: Result<XiaohongshuCommentsPage, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(page) => (format_comments(&page, format)?, 0, page.diagnostic),
        Err(error) => {
            let stdout = match format {
                OutputFormat::Json => format_failure_json(&error)?,
                OutputFormat::Markdown => format!(
                    "# Comments failed\n\n**{}**: {}",
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

fn format_comments(page: &XiaohongshuCommentsPage, format: OutputFormat) -> Result<String, String> {
    if format == OutputFormat::Json {
        return serde_json::to_string(page).map_err(|error| error.to_string());
    }
    let mut markdown = format!(
        "# {} comments on `{}` ({})\n",
        page.platform, page.note, page.provider
    );
    let one_line = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
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
    Ok(markdown)
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
        Err(error) => {
            let stdout = match format {
                DocsOutputFormat::Json => format_failure_json(&error)?,
                DocsOutputFormat::Markdown | DocsOutputFormat::Content => format!(
                    "# Platform fetch failed\n\n**{}**: {}",
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
        format == DocsOutputFormat::Json,
        output,
        diagnostic,
    )
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
    result: Result<XAskResult, ProviderError>,
    format: OutputFormat,
    output: Option<OutputTarget>,
) -> Result<RenderedOutput, String> {
    let (stdout, exit_code, diagnostic) = match result {
        Ok(answer) => (format_ask(&answer, format)?, 0, answer.diagnostic),
        Err(error) => {
            let stdout = match format {
                OutputFormat::Json => format_failure_json(&error)?,
                OutputFormat::Markdown => format!(
                    "# X ask failed\n\n**{}**: {}",
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

fn format_ask(answer: &XAskResult, format: OutputFormat) -> Result<String, String> {
    if format == OutputFormat::Json {
        return serde_json::to_string(answer).map_err(|error| error.to_string());
    }
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
    Ok(markdown)
}
