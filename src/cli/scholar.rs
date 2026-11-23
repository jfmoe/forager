//! Google Scholar arguments and request construction.

use clap::{Args, Subcommand, ValueEnum};

use super::{PageInput, PlatformCommonArgs, fetch, scholar_cited_by, search};
use crate::app::args::{ContentOutputFormat, OutputFormat};
use crate::app::dispatch::{AppError, CommandOutput};
use crate::types::{
    ContentDepth, Platform, PlatformFetchRequest, PlatformRef, PlatformSearchOptions,
    PlatformSearchRequest, ScholarCitedByRequest, ScholarCitedBySort, ScholarRef,
    ScholarSearchOptions,
};

#[derive(Debug, Subcommand)]
pub(in crate::app) enum ScholarCommand {
    /// Search Google Scholar; each result carries its cluster ref, byline, citation count, and
    /// links, with a snippet when Google Scholar shows one.
    Search(ScholarSearchArgs),
    /// List the versions Google Scholar groups under one paper; metadata only.
    Fetch(ScholarFetchArgs),
    /// List the works Google Scholar counts as citing one paper, as search results. An unknown
    /// paper and an uncited one both list nothing.
    CitedBy(ScholarCitedByArgs),
}

#[derive(Debug, Args)]
pub(in crate::app) struct ScholarSearchArgs {
    /// Google Scholar query, sent unchanged; its own operators such as "phrase", OR, -term,
    /// author:, and source: apply.
    #[arg(conflicts_with = "cursor")]
    query: Option<String>,
    /// Maximum results on this page; every page costs one search whatever its size.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=20), conflicts_with = "cursor")]
    limit: u16,
    /// First included publication year.
    #[arg(long, value_name = "YYYY", value_parser = clap::value_parser!(u16).range(1000..=9999), conflicts_with = "cursor")]
    year_from: Option<u16>,
    /// Last included publication year.
    #[arg(long, value_name = "YYYY", value_parser = clap::value_parser!(u16).range(1000..=9999), conflicts_with = "cursor")]
    year_to: Option<u16>,
    /// Only review articles.
    #[arg(long, conflicts_with = "cursor")]
    review_only: bool,
    /// Opaque `next_cursor` from a previous page; it restores the complete original request.
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct ScholarFetchArgs {
    /// A `scholar:<cluster_id>` ref, or a scholar.google.com/scholar?cluster=<id> URL.
    reference: String,
    /// Google Scholar provides `metadata` only; other depths are rejected before any request.
    #[arg(long, value_enum, default_value_t = ScholarDepthArg::Metadata)]
    depth: ScholarDepthArg,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Debug, Args)]
pub(in crate::app) struct ScholarCitedByArgs {
    /// The cited paper: a `scholar:<cluster_id>` ref, or a scholar.google.com/scholar?cluster=<id>
    /// URL.
    #[arg(required_unless_present = "cursor", conflicts_with = "cursor")]
    reference: Option<String>,
    /// Only citing works that match this Google Scholar query; its own operators apply.
    #[arg(long, conflicts_with = "cursor")]
    query: Option<String>,
    /// Maximum results on this page; every page costs one search whatever its size.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=20), conflicts_with = "cursor")]
    limit: u16,
    /// First included publication year.
    #[arg(long, value_name = "YYYY", value_parser = clap::value_parser!(u16).range(1000..=9999), conflicts_with = "cursor")]
    year_from: Option<u16>,
    /// Last included publication year.
    #[arg(long, value_name = "YYYY", value_parser = clap::value_parser!(u16).range(1000..=9999), conflicts_with = "cursor")]
    year_to: Option<u16>,
    /// `relevance` keeps Google Scholar's order; `date` lists the most recently indexed citing
    /// works first and takes no year range.
    #[arg(long, value_enum, default_value_t = ScholarCitedBySortArg::Relevance, conflicts_with = "cursor")]
    sort: ScholarCitedBySortArg,
    /// Opaque `next_cursor` from a previous cited-by page; it restores the complete original
    /// request.
    #[arg(long)]
    cursor: Option<String>,
    #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
    format: OutputFormat,
    #[command(flatten)]
    common: PlatformCommonArgs,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ScholarCitedBySortArg {
    Relevance,
    Date,
}

impl From<ScholarCitedBySortArg> for ScholarCitedBySort {
    fn from(value: ScholarCitedBySortArg) -> Self {
        match value {
            ScholarCitedBySortArg::Relevance => Self::Relevance,
            ScholarCitedBySortArg::Date => Self::Date,
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ScholarDepthArg {
    Metadata,
    Snippet,
    Abstract,
    #[value(name = "full_text")]
    FullText,
}

impl From<ScholarDepthArg> for ContentDepth {
    fn from(value: ScholarDepthArg) -> Self {
        match value {
            ScholarDepthArg::Metadata => Self::Metadata,
            ScholarDepthArg::Snippet => Self::Snippet,
            ScholarDepthArg::Abstract => Self::Abstract,
            ScholarDepthArg::FullText => Self::FullText,
        }
    }
}

pub(in crate::app) fn run(command: ScholarCommand) -> Result<CommandOutput, AppError> {
    match command {
        ScholarCommand::Search(arguments) => scholar_search(arguments),
        ScholarCommand::Fetch(arguments) => scholar_fetch(arguments),
        ScholarCommand::CitedBy(arguments) => cited_by(arguments),
    }
}

fn scholar_search(arguments: ScholarSearchArgs) -> Result<CommandOutput, AppError> {
    let (input, format, common) = scholar_search_input(arguments)?;
    search(Platform::Scholar, input, format, &common)
}

/// Builds the page a search asks for and checks it before any request.
fn scholar_search_input(
    arguments: ScholarSearchArgs,
) -> Result<
    (
        PageInput<PlatformSearchRequest>,
        OutputFormat,
        PlatformCommonArgs,
    ),
    AppError,
> {
    let ScholarSearchArgs {
        query,
        limit,
        year_from,
        year_to,
        review_only,
        cursor,
        format,
        common,
    } = arguments;
    let input = if let Some(cursor) = cursor {
        PageInput::Cursor(cursor)
    } else {
        let request = PlatformSearchRequest {
            query: query.unwrap_or_default(),
            limit,
            options: PlatformSearchOptions::Scholar(ScholarSearchOptions {
                year_from,
                year_to,
                review_only,
            }),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    Ok((input, format, common))
}

fn cited_by(arguments: ScholarCitedByArgs) -> Result<CommandOutput, AppError> {
    let (input, format, common) = cited_by_input(arguments)?;
    scholar_cited_by(input, format, &common)
}

/// Builds the page of citing works a command asks for and checks it before any request.
fn cited_by_input(
    arguments: ScholarCitedByArgs,
) -> Result<
    (
        PageInput<ScholarCitedByRequest>,
        OutputFormat,
        PlatformCommonArgs,
    ),
    AppError,
> {
    let ScholarCitedByArgs {
        reference,
        query,
        limit,
        year_from,
        year_to,
        sort,
        cursor,
        format,
        common,
    } = arguments;
    let input = if let Some(cursor) = cursor {
        PageInput::Cursor(cursor)
    } else {
        let cited = ScholarRef::parse(&reference.unwrap_or_default())
            .map_err(|error| AppError::Argument(error.to_string()))?;
        let request = ScholarCitedByRequest {
            cited,
            query,
            limit,
            year_from,
            year_to,
            sort: sort.into(),
            page: None,
        };
        request.validate().map_err(AppError::Argument)?;
        PageInput::Request(request)
    };
    Ok((input, format, common))
}

fn scholar_fetch(arguments: ScholarFetchArgs) -> Result<CommandOutput, AppError> {
    let (request, format, common) = scholar_fetch_input(arguments)?;
    fetch(Platform::Scholar, request, format, None, &common)
}

/// Reads the cluster ref before any request.
fn scholar_fetch_input(
    arguments: ScholarFetchArgs,
) -> Result<
    (
        PlatformFetchRequest,
        ContentOutputFormat,
        PlatformCommonArgs,
    ),
    AppError,
> {
    let ScholarFetchArgs {
        reference,
        depth,
        format,
        common,
    } = arguments;
    let reference = PlatformRef::parse(Platform::Scholar, &reference)
        .map_err(|error| AppError::Argument(error.to_string()))?;
    let request = PlatformFetchRequest::new(reference, depth.into());
    let format = match format {
        OutputFormat::Json => ContentOutputFormat::Json,
        OutputFormat::Markdown => ContentOutputFormat::Markdown,
    };
    Ok((request, format, common))
}

/// The Scholar commands' argument rules, proven by parsing their definition in process.
#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::super::{PageInput, PlatformCommand, parse_platform};
    use super::{
        ScholarCitedByArgs, ScholarCommand, ScholarFetchArgs, ScholarSearchArgs, cited_by_input,
        scholar_fetch_input, scholar_search_input,
    };
    use crate::app::AppError;
    use crate::types::{
        ContentDepth, PlatformSearchOptions, PlatformSearchRequest, ScholarCitedByRequest,
        ScholarCitedBySort, ScholarSearchOptions,
    };

    const CITED: &str = "scholar:18208131694456651388";
    const SEARCH_CURSOR: &str = "v1.serpapi.eyJxdWVyeSI6Im1vbWVudHVtIiwibGltaXQiOjIwLCJvcHRpb25zIjp7InBsYXRmb3JtIjoic2Nob2xhciJ9LCJwYWdlIjoiMjAifQ";
    const CITED_BY_CURSOR: &str =
        "v1.serpapi.eyJjaXRlZCI6MTgyMDgxMzE2OTQ0NTY2NTEzODgsImxpbWl0IjoyMCwicGFnZSI6IjIwIn0";

    fn scholar(arguments: &[&str]) -> Result<ScholarCommand, clap::Error> {
        let command = std::iter::once("scholar").chain(arguments.iter().copied());
        match parse_platform(&command.collect::<Vec<_>>())? {
            PlatformCommand::Scholar { command } => Ok(command),
            _ => unreachable!("the arguments name scholar"),
        }
    }

    fn search_args(arguments: &[&str]) -> Result<ScholarSearchArgs, clap::Error> {
        let search = std::iter::once("search").chain(arguments.iter().copied());
        match scholar(&search.collect::<Vec<_>>())? {
            ScholarCommand::Search(arguments) => Ok(arguments),
            _ => unreachable!("the arguments name scholar search"),
        }
    }

    fn cited_by_args(arguments: &[&str]) -> Result<ScholarCitedByArgs, clap::Error> {
        let cited_by = std::iter::once("cited-by").chain(arguments.iter().copied());
        match scholar(&cited_by.collect::<Vec<_>>())? {
            ScholarCommand::CitedBy(arguments) => Ok(arguments),
            _ => unreachable!("the arguments name scholar cited-by"),
        }
    }

    fn fetch_args(arguments: &[&str]) -> Result<ScholarFetchArgs, clap::Error> {
        let fetch = std::iter::once("fetch").chain(arguments.iter().copied());
        match scholar(&fetch.collect::<Vec<_>>())? {
            ScholarCommand::Fetch(arguments) => Ok(arguments),
            _ => unreachable!("the arguments name scholar fetch"),
        }
    }

    /// The first page a search asks for, or the argument error its checks raise.
    fn search_request(arguments: &[&str]) -> Result<PlatformSearchRequest, AppError> {
        let Ok(parsed) = search_args(arguments) else {
            panic!("the parser accepts a scholar search of {arguments:?}");
        };
        match scholar_search_input(parsed)?.0 {
            PageInput::Request(request) => Ok(request),
            PageInput::Cursor(_) => panic!("the arguments name no cursor"),
        }
    }

    /// The first page of citing works a command asks for, or the argument error its checks raise.
    fn cited_by_request(arguments: &[&str]) -> Result<ScholarCitedByRequest, AppError> {
        let Ok(parsed) = cited_by_args(arguments) else {
            panic!("the parser accepts a scholar cited-by of {arguments:?}");
        };
        match cited_by_input(parsed)?.0 {
            PageInput::Request(request) => Ok(request),
            PageInput::Cursor(_) => panic!("the arguments name no cursor"),
        }
    }

    fn argument_error<T: std::fmt::Debug>(result: Result<T, AppError>) -> String {
        match result {
            Err(AppError::Argument(message)) => message,
            other => panic!("expected an argument error, got {other:?}"),
        }
    }

    // Search.

    #[test]
    fn a_search_asks_for_a_first_page_of_twenty_results_by_default() {
        let request = search_request(&["momentum"]).expect("a request");

        assert_eq!(
            request,
            PlatformSearchRequest {
                query: "momentum".to_owned(),
                limit: 20,
                options: PlatformSearchOptions::Scholar(ScholarSearchOptions::default()),
                page: None,
            }
        );
    }

    #[test]
    fn page_sizes_and_years_outside_their_definition_are_refused_while_parsing() {
        for (arguments, flag) in [
            (&["momentum", "--limit", "0"][..], "--limit"),
            (&["momentum", "--limit", "21"], "--limit"),
            (&["momentum", "--year-from", "999"], "--year-from"),
            (&["momentum", "--year-to", "10000"], "--year-to"),
        ] {
            let error = search_args(arguments).expect_err("the parser refuses the value");

            assert_eq!(
                (error.kind(), error.to_string().contains(flag)),
                (ErrorKind::ValueValidation, true),
                "arguments: {arguments:?}\n{error}"
            );
        }
    }

    #[test]
    fn a_blank_query_or_reversed_years_are_argument_errors_before_any_request() {
        for (arguments, message) in [
            (&["  "][..], "scholar search needs a query"),
            (
                &["momentum", "--year-from", "2025", "--year-to", "2024"],
                "--year-from must not be later than --year-to",
            ),
        ] {
            let found = argument_error(search_request(arguments));

            assert!(
                found.contains(message),
                "arguments: {arguments:?}\nmessage: {found}"
            );
        }
    }

    #[test]
    fn a_cursor_conflicts_with_an_explicit_query_option_or_limit() {
        for extra in [
            &["momentum"][..],
            &["--limit", "20"],
            &["--year-from", "2020"],
            &["--year-to", "2024"],
            &["--review-only"],
        ] {
            let mut arguments = vec!["--cursor", SEARCH_CURSOR];
            arguments.extend_from_slice(extra);

            let error = search_args(&arguments).expect_err("the parser refuses the combination");

            assert_eq!(
                error.kind(),
                ErrorKind::ArgumentConflict,
                "arguments: {arguments:?}\n{error}"
            );
        }
    }

    // Cited-by.

    #[test]
    fn a_cited_by_page_asks_for_twenty_results_in_relevance_order_by_default() {
        let request = cited_by_request(&[CITED]).expect("a request");

        assert_eq!(
            (
                request.cited.to_string(),
                request.query,
                request.limit,
                (request.year_from, request.year_to),
                request.sort,
                request.page
            ),
            (
                "18208131694456651388".to_owned(),
                None,
                20,
                (None, None),
                ScholarCitedBySort::Relevance,
                None
            )
        );
    }

    #[test]
    fn a_missing_target_or_a_page_size_outside_one_to_twenty_is_refused_while_parsing() {
        for (arguments, kind) in [
            (&[][..], ErrorKind::MissingRequiredArgument),
            (&[CITED, "--limit", "0"], ErrorKind::ValueValidation),
            (&[CITED, "--limit", "21"], ErrorKind::ValueValidation),
        ] {
            let error = cited_by_args(arguments).expect_err("the parser refuses the arguments");

            assert_eq!(error.kind(), kind, "arguments: {arguments:?}\n{error}");
        }
    }

    #[test]
    fn cited_by_targets_and_options_google_scholar_cannot_run_are_argument_errors() {
        for (arguments, message) in [
            (
                &["https://scholar.google.com/scholar?cites=18208131694456651388"][..],
                "unrecognized scholar reference",
            ),
            (
                &[CITED, "--year-from", "2025", "--year-to", "2024"],
                "--year-from must not be later than --year-to",
            ),
            (
                &[CITED, "--sort", "date", "--year-from", "2020"],
                "--sort date cannot be combined with --year-from or --year-to",
            ),
            (
                &[CITED, "--sort", "date", "--year-to", "2024"],
                "--sort date cannot be combined with --year-from or --year-to",
            ),
            (&[CITED, "--query", " "], "--query needs a word"),
        ] {
            let found = argument_error(cited_by_request(arguments));

            assert!(
                found.contains(message),
                "arguments: {arguments:?}\nmessage: {found}"
            );
        }
    }

    #[test]
    fn a_cited_by_cursor_conflicts_with_a_target_query_option_or_limit() {
        for extra in [
            &[CITED][..],
            &["--query", "crash"],
            &["--limit", "20"],
            &["--year-from", "2020"],
            &["--year-to", "2024"],
            &["--sort", "date"],
        ] {
            let mut arguments = vec!["--cursor", CITED_BY_CURSOR];
            arguments.extend_from_slice(extra);

            let error = cited_by_args(&arguments).expect_err("the parser refuses the combination");

            assert_eq!(
                error.kind(),
                ErrorKind::ArgumentConflict,
                "arguments: {arguments:?}\n{error}"
            );
        }
    }

    // Fetch.

    #[test]
    fn a_cluster_ref_or_url_fetches_the_cluster_it_names_at_the_depth_asked() {
        for (arguments, depth) in [
            (&[CITED][..], ContentDepth::Metadata),
            (
                &[
                    "https://scholar.google.com/scholar?cluster=18208131694456651388&hl=en&num=20&as_sdt=0,27",
                ],
                ContentDepth::Metadata,
            ),
            (&[CITED, "--depth", "abstract"], ContentDepth::Abstract),
        ] {
            let parsed = fetch_args(arguments).expect("the parser accepts the arguments");

            let (request, _, _) = scholar_fetch_input(parsed).expect("a request");

            assert_eq!(
                (request.reference.to_string(), request.depth),
                ("scholar:18208131694456651388".to_owned(), depth),
                "arguments: {arguments:?}"
            );
        }
    }

    #[test]
    fn urls_that_name_no_single_cluster_are_argument_errors_before_any_request() {
        for input in [
            "https://scholar.google.com/scholar?cites=18208131694456651388",
            "https://scholar.google.com/scholar?cluster=18208131694456651388&cites=18208131694456651388",
            "https://scholar.google.com/scholar?cluster=1&cluster=2",
            "https://scholar.google.com/scholar?cluster=18446744073709551616",
            "https://scholar.google.com/citations?user=q9g8tuAAAAAJ&hl=en",
        ] {
            let parsed = fetch_args(&[input]).expect("the parser accepts any reference");

            let message = argument_error(scholar_fetch_input(parsed).map(|_| ()));

            assert!(
                message.contains("unrecognized scholar reference"),
                "{input}: {message}"
            );
        }
    }
}
