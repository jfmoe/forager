//! Hacker News arguments and request construction.

use std::path::PathBuf;
use std::time::Duration;

use chrono::NaiveDate;
use clap::{Args, Subcommand, ValueEnum};

use super::{
    ContentInput, PageInput, PlatformCommonArgs, PlatformInput, fetch, parse_date, preflight_error,
    search,
};
use crate::app::args::{ContentOutputFormat, OutputFormat};
use crate::app::dispatch::{
    AppError, CommandOutput, NetworkDependencies, Presentation, provider_attempt_log,
};
use crate::platform_operation;
use crate::types::{
    ContentDepth, Deadline, HnRef, HnSearchOptions, HnSearchType, HnSort, HnStoriesList,
    HnStoriesRequest, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum HnCommand {
    /// Search Hacker News stories, comments, and other items; each result carries its ref,
    /// points, comment count, and its own text when it has one.
    Search(HnSearchArgs),
    /// Fetch one Hacker News item; by default its whole discussion is written to a local
    /// Markdown file, top-level comments in the rank Hacker News shows.
    Fetch(HnFetchArgs),
    /// List the current items of an official Hacker News list in rank order.
    Stories(HnStoriesArgs),
}

#[derive(Debug, Args)]
pub(in crate::app) struct HnSearchArgs {
    /// Words to match; "double quotes" match a phrase and -word excludes a word.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
    /// Result order.
    #[arg(long, value_enum, default_value_t = HnSortArg::Relevance, conflicts_with = "cursor")]
    sort: HnSortArg,
    /// Item type; repeat to match any of several.
    #[arg(
        long = "type",
        value_name = "TYPE",
        value_enum,
        conflicts_with = "cursor"
    )]
    types: Vec<HnTypeArg>,
    /// Only items by this Hacker News user.
    #[arg(long, value_name = "USER", conflicts_with = "cursor")]
    author: Option<String>,
    /// Only items in the discussion of this story ID.
    #[arg(long, value_name = "ID", value_parser = parse_item_id, conflicts_with = "cursor")]
    story: Option<u64>,
    /// First included publication day (UTC).
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor")]
    from: Option<NaiveDate>,
    /// Last included publication day (UTC); the whole day is included.
    #[arg(long, value_name = "YYYY-MM-DD", value_parser = parse_date, conflicts_with = "cursor")]
    to: Option<NaiveDate>,
    /// Only items with at least this many points; comments have none.
    #[arg(long, value_name = "N", conflicts_with = "cursor")]
    min_points: Option<u64>,
    /// Only items with at least this many comments.
    #[arg(long, value_name = "N", conflicts_with = "cursor")]
    min_comments: Option<u64>,
    /// Maximum results on this page; a search reaches at most its first 1000 results.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=100), conflicts_with = "cursor")]
    limit: u16,
    /// Opaque `next_cursor` from a previous page; it restores the complete original request.
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct HnFetchArgs {
    /// An `hn:<id>` ref or a news.ycombinator.com item URL.
    reference: String,
    /// `thread` writes the discussion to a file; `metadata` returns the item alone with live
    /// counts; `full_text` reads the page a story links to, or the item's own text.
    #[arg(long, value_enum, default_value_t = HnDepthArg::Thread)]
    depth: HnDepthArg,
    /// Directory for the discussion or full-text Markdown file; defaults to a new directory
    /// under the system temporary directory.
    #[arg(long, value_name = "DIR")]
    content_dir: Option<PathBuf>,
    /// `content` prints the discussion or the full text to stdout and writes no file.
    #[arg(long, value_enum, default_value_t = ContentOutputFormat::Json)]
    format: ContentOutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct HnStoriesArgs {
    /// The list: `top` is the front page.
    #[arg(value_enum, default_value_t = HnListArg::Top)]
    list: HnListArg,
    /// Maximum items. The list has no cursor; it changes constantly.
    #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u16).range(1..=100))]
    limit: u16,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HnSortArg {
    Relevance,
    Date,
}

impl From<HnSortArg> for HnSort {
    fn from(value: HnSortArg) -> Self {
        match value {
            HnSortArg::Relevance => Self::Relevance,
            HnSortArg::Date => Self::Date,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HnTypeArg {
    Story,
    Comment,
    #[value(name = "ask_hn")]
    AskHn,
    #[value(name = "show_hn")]
    ShowHn,
    #[value(name = "launch_hn")]
    LaunchHn,
    Job,
    Poll,
}

impl From<HnTypeArg> for HnSearchType {
    fn from(value: HnTypeArg) -> Self {
        match value {
            HnTypeArg::Story => Self::Story,
            HnTypeArg::Comment => Self::Comment,
            HnTypeArg::AskHn => Self::AskHn,
            HnTypeArg::ShowHn => Self::ShowHn,
            HnTypeArg::LaunchHn => Self::LaunchHn,
            HnTypeArg::Job => Self::Job,
            HnTypeArg::Poll => Self::Poll,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HnDepthArg {
    Thread,
    Metadata,
    Snippet,
    Abstract,
    #[value(name = "full_text")]
    FullText,
}

impl From<HnDepthArg> for ContentDepth {
    fn from(value: HnDepthArg) -> Self {
        match value {
            HnDepthArg::Thread => Self::Thread,
            HnDepthArg::Metadata => Self::Metadata,
            HnDepthArg::Snippet => Self::Snippet,
            HnDepthArg::Abstract => Self::Abstract,
            HnDepthArg::FullText => Self::FullText,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum HnListArg {
    Top,
    New,
    Best,
    Ask,
    Show,
    Job,
}

impl From<HnListArg> for HnStoriesList {
    fn from(value: HnListArg) -> Self {
        match value {
            HnListArg::Top => Self::Top,
            HnListArg::New => Self::New,
            HnListArg::Best => Self::Best,
            HnListArg::Ask => Self::Ask,
            HnListArg::Show => Self::Show,
            HnListArg::Job => Self::Job,
        }
    }
}

fn parse_item_id(value: &str) -> Result<u64, String> {
    HnRef::from_id(value)
        .map(HnRef::id)
        .ok_or_else(|| format!("`{value}` is not a Hacker News item ID"))
}

pub(in crate::app) fn run(command: HnCommand) -> Result<CommandOutput, AppError> {
    match command {
        HnCommand::Search(arguments) => hn_search(arguments),
        HnCommand::Fetch(arguments) => hn_fetch(arguments),
        HnCommand::Stories(arguments) => hn_stories(arguments),
    }
}

fn hn_search(arguments: HnSearchArgs) -> Result<CommandOutput, AppError> {
    let PlatformInput {
        request: input,
        format,
        common,
    } = hn_search_input(arguments)?;
    search(Platform::Hn, input, format, &common)
}

/// Builds the page a search asks for and checks its cross-field rules before any request.
fn hn_search_input(
    arguments: HnSearchArgs,
) -> Result<PlatformInput<PageInput<PlatformSearchRequest>, OutputFormat>, AppError> {
    let HnSearchArgs {
        query,
        sort,
        types,
        author,
        story,
        from,
        to,
        min_points,
        min_comments,
        limit,
        cursor,
        format,
        common,
    } = arguments;
    let input = if let Some(cursor) = cursor {
        PageInput::Cursor(cursor)
    } else {
        let options = HnSearchOptions {
            sort: sort.into(),
            types: types.into_iter().map(HnSearchType::from).collect(),
            author,
            story,
            from,
            to,
            min_points,
            min_comments,
        };
        let request = PlatformSearchRequest {
            query: query.unwrap_or_default(),
            limit,
            options: PlatformSearchOptions::Hn(options.normalized()),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    Ok(PlatformInput {
        request: input,
        format,
        common,
    })
}

fn hn_fetch(arguments: HnFetchArgs) -> Result<CommandOutput, AppError> {
    let ContentInput {
        request,
        format,
        content_dir,
        common,
    } = hn_fetch_input(arguments)?;
    fetch(Platform::Hn, request, format, content_dir, &common)
}

/// Reads the item ref before any request.
fn hn_fetch_input(arguments: HnFetchArgs) -> Result<ContentInput<PlatformFetchRequest>, AppError> {
    let HnFetchArgs {
        reference,
        depth,
        content_dir,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Hn, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest::new(reference, depth.into());
    Ok(ContentInput {
        request,
        format,
        content_dir,
        common,
    })
}

fn hn_stories(arguments: HnStoriesArgs) -> Result<CommandOutput, AppError> {
    let HnStoriesArgs {
        list,
        limit,
        format,
        common,
    } = arguments;
    let request = HnStoriesRequest {
        list: list.into(),
        limit,
    };
    let dependencies = NetworkDependencies::load()?;
    let plan = platform_operation::plan_stories(
        dependencies.platform(Platform::Hn)?,
        &dependencies.config.platform_routes,
        request,
    )
    .map_err(preflight_error)?;
    let result = dependencies
        .runtime
        .block_on(platform_operation::run_stories(
            plan,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(common.timeout)),
        ));
    let attempt_log = provider_attempt_log(dependencies.config.log_level, &result, |listing| {
        &listing.attempts
    });
    Ok(CommandOutput::HnStories {
        result,
        presentation: Presentation {
            format,
            output: common.output.target(),
            verbose: common.verbose,
            attempt_log,
        },
    })
}

/// The Hacker News commands' argument rules, proven by parsing their definition in process.
#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use clap::error::ErrorKind;

    use super::super::{PageInput, PlatformCommand, parse_platform};
    use super::{HnCommand, hn_fetch_input, hn_search_input};
    use crate::app::AppError;
    use crate::types::{
        HnSearchOptions, HnSearchType, HnSort, PlatformSearchOptions, PlatformSearchRequest,
    };

    fn command(arguments: &[&str]) -> Result<HnCommand, clap::Error> {
        let command = std::iter::once("hn").chain(arguments.iter().copied());
        match parse_platform(&command.collect::<Vec<_>>())? {
            PlatformCommand::Hn { command } => Ok(command),
            _ => unreachable!("the arguments name a hn command"),
        }
    }

    /// The first page a search asks for, or the argument error its checks raise.
    fn search_request(arguments: &[&str]) -> Result<PlatformSearchRequest, AppError> {
        let search = ["search"].into_iter().chain(arguments.iter().copied());
        let Ok(HnCommand::Search(arguments)) = command(&search.collect::<Vec<_>>()) else {
            panic!("the parser accepts a hn search of {arguments:?}");
        };
        match hn_search_input(arguments)?.request {
            PageInput::Request(request) => Ok(request),
            PageInput::Cursor(_) => panic!("the arguments name no cursor"),
        }
    }

    fn day(value: &str) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
    }

    #[test]
    fn search_options_become_one_request_with_repeated_types_dropped_and_a_default_limit() {
        let filtered = search_request(&[
            "\"sync service\" -dropbox",
            "--sort",
            "date",
            "--type",
            "show_hn",
            "--type",
            "comment",
            "--type",
            "show_hn",
            "--author",
            "pg",
            "--story",
            "8863",
            "--from",
            "2024-01-01",
            "--to",
            "2024-01-31",
            "--min-comments",
            "5",
            "--limit",
            "10",
        ]);
        let plain = search_request(&["dropbox"]);

        assert_eq!(
            [filtered.ok(), plain.ok()],
            [
                Some(PlatformSearchRequest {
                    query: "\"sync service\" -dropbox".to_owned(),
                    limit: 10,
                    options: PlatformSearchOptions::Hn(HnSearchOptions {
                        sort: HnSort::Date,
                        types: vec![HnSearchType::ShowHn, HnSearchType::Comment],
                        author: Some("pg".to_owned()),
                        story: Some(8863),
                        from: day("2024-01-01"),
                        to: day("2024-01-31"),
                        min_points: None,
                        min_comments: Some(5),
                    }),
                    page: None,
                }),
                Some(PlatformSearchRequest {
                    query: "dropbox".to_owned(),
                    limit: 20,
                    options: PlatformSearchOptions::Hn(HnSearchOptions::default()),
                    page: None,
                }),
            ]
        );
    }

    #[test]
    fn contradictory_search_options_are_an_argument_error_before_any_request() {
        assert!(matches!(
            search_request(&["rust", "--type", "comment", "--min-points", "10"]),
            Err(AppError::Argument(_))
        ));
    }

    #[test]
    fn values_outside_their_definition_are_refused_while_parsing() {
        for arguments in [
            &["search", "rust", "--story", "08863"][..],
            &["search", "rust", "--limit", "101"],
            &["stories", "--limit", "0"],
            &["stories", "--limit", "101"],
            &["stories", "front"],
        ] {
            let error = command(arguments)
                .map(|_| ())
                .expect_err("the parser refuses the value");

            assert!(
                matches!(
                    error.kind(),
                    ErrorKind::ValueValidation | ErrorKind::InvalidValue
                ),
                "arguments: {arguments:?}\n{error}"
            );
        }
    }

    #[test]
    fn a_fetch_of_what_names_no_hacker_news_item_is_an_argument_error_before_any_request() {
        let Ok(HnCommand::Fetch(arguments)) =
            command(&["fetch", "https://news.ycombinator.com/user?id=pg"])
        else {
            panic!("the parser accepts any reference");
        };

        let result = hn_fetch_input(arguments).map(|_| ());

        assert!(
            matches!(&result, Err(AppError::Argument(message)) if message.contains("pass an `hn:<id>` ref or a Hacker News item URL")),
            "{result:?}"
        );
    }
}
