//! The arXiv commands' argument rules, proven by parsing their definition in process.

use clap::Parser;

use super::{ArxivCommand, ArxivSearchArgs, PlatformCommand, arxiv_search_input};
use crate::app::AppError;

#[derive(Parser)]
struct PlatformCli {
    #[command(subcommand)]
    command: PlatformCommand,
}

fn search_args(arguments: &[&str]) -> Result<ArxivSearchArgs, clap::Error> {
    let cli = PlatformCli::try_parse_from(
        ["platform", "arxiv", "search"]
            .into_iter()
            .chain(arguments.iter().copied()),
    )?;
    match cli.command {
        PlatformCommand::Arxiv {
            command: ArxivCommand::Search(arguments),
        } => Ok(arguments),
        _ => unreachable!("the arguments name arxiv search"),
    }
}

#[test]
fn option_values_outside_their_definition_are_refused_while_parsing() {
    for (arguments, message) in [
        (&["x", "--sort", "newest"][..], "invalid value"),
        (&["x", "--limit", "0"], "invalid value"),
        (&["x", "--limit", "101"], "invalid value"),
        (&["x", "--submitted-from", "2024-13-01"], "calendar date"),
    ] {
        let error = search_args(arguments).expect_err("the parser refuses the value");

        assert!(
            error.to_string().contains(message),
            "arguments: {arguments:?}\n{error}"
        );
    }
}

#[test]
fn searches_the_options_cannot_express_are_argument_errors_before_any_request() {
    for (arguments, message) in [
        (
            &["x", "--category", "cs.AI OR all:x"][..],
            "invalid arXiv category",
        ),
        (&["x", "--author", " "], "--author must contain"),
        (&[r#""world model"#], "unmatched double quote"),
        (
            &[
                "x",
                "--submitted-from",
                "2024-02-01",
                "--submitted-to",
                "2024-01-01",
            ],
            "--submitted-from must not be later than --submitted-to",
        ),
        (
            &["--submitted-from", "2024-01-01"],
            "needs a query or at least one of --category, --author, --title",
        ),
    ] {
        let arguments = search_args(arguments).expect("the parser accepts the values");

        let result = arxiv_search_input(arguments).map(|_| ());

        assert!(
            matches!(&result, Err(AppError::Argument(found)) if found.contains(message)),
            "expected an argument error about {message:?}, got {result:?}"
        );
    }
}
