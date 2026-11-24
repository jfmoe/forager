use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::catalog::{self, ProviderId};
use crate::providers::SearchType;
use crate::types::{CapabilitySet, FallbackPolicy};

/// Parsed `forager` command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "forager", version, infer_subcommands = false)]
pub struct Cli {
    #[command(subcommand)]
    pub(super) command: Command,
}

impl Cli {
    /// Returns whether preflight failures use the machine-readable JSON channel.
    #[must_use]
    pub fn uses_json_preflight_errors(&self) -> bool {
        matches!(
            &self.command,
            Command::Search {
                format: ContentOutputFormat::Json,
                ..
            } | Command::Fetch {
                format: ContentOutputFormat::Json,
                ..
            }
        )
    }

    /// Returns the requested output destination for a JSON preflight failure.
    #[must_use]
    pub fn json_preflight_output(&self) -> Option<OutputTarget> {
        match &self.command {
            Command::Search {
                format: ContentOutputFormat::Json,
                output,
                ..
            }
            | Command::Fetch {
                format: ContentOutputFormat::Json,
                output,
                ..
            } => output.target(),
            _ => None,
        }
    }
}

#[derive(Debug, Subcommand)]
pub(super) enum Command {
    #[command(visible_alias = "s")]
    Search {
        query: String,
        #[arg(long)]
        capabilities: Option<CapabilitySet>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = 0, value_parser = clap::value_parser!(u16).range(0..=20))]
        extra_sources: u16,
        #[arg(long)]
        fallback: Option<FallbackPolicy>,
        #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(long, value_enum, default_value_t = ContentOutputFormat::Json)]
        format: ContentOutputFormat,
        #[command(flatten)]
        output: OutputArgs,
        #[arg(long)]
        verbose: bool,
    },
    #[command(visible_alias = "f")]
    Fetch {
        url: String,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = ContentOutputFormat::Json)]
        format: ContentOutputFormat,
        #[command(flatten)]
        output: OutputArgs,
        #[arg(long)]
        verbose: bool,
    },
    Map {
        url: String,
        #[arg(long, default_value = "")]
        instructions: String,
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..=5))]
        max_depth: u16,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..=500))]
        max_breadth: u16,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..))]
        limit: u16,
        #[arg(long, default_value_t = 150, value_parser = clap::value_parser!(u64).range(10..=150))]
        timeout: u64,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[command(flatten)]
        output: OutputArgs,
        #[arg(long)]
        verbose: bool,
    },
    Exa {
        #[command(subcommand)]
        command: ExaCommand,
    },
    /// Search or fetch items of a built-in platform through its configured routes.
    Platform {
        #[command(subcommand)]
        command: super::platform::PlatformCommand,
    },
    /// Install the Managed Browser, or log into a platform's Browser Profile.
    Browser {
        #[command(subcommand)]
        command: super::browser::BrowserCommand,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Setup {
        #[arg(long)]
        non_interactive: bool,
        #[arg(long, value_enum)]
        lang: Option<Language>,
    },
    Doctor {
        #[arg(long, value_parser = parse_provider)]
        provider: Option<ProviderId>,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
    },
    Smoke {
        #[arg(long)]
        live: bool,
        #[arg(long, requires = "live")]
        list: bool,
        #[arg(long, requires = "live", default_value_t = 420, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(long, requires = "live", value_name = "CASE_ID=EVIDENCE_URL")]
        outage_evidence: Vec<String>,
        #[arg(long, requires = "live")]
        opt_in: bool,
        #[arg(long, hide = true, conflicts_with = "live", value_parser = ["C05", "C06", "OUTAGE"])]
        probe: Option<String>,
        #[arg(long, hide = true, requires = "probe", default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..))]
        probe_timeout: u64,
        #[arg(long, hide = true, requires = "probe")]
        probe_url: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum ExaCommand {
    Search {
        query: String,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..=100))]
        num_results: u16,
        #[arg(long, value_enum, default_value_t = ExaSearchType::Auto)]
        search_type: ExaSearchType,
        #[arg(long)]
        include_text: bool,
        #[arg(long, default_value_t = 3000, requires = "include_text", value_parser = clap::value_parser!(u32).range(1..))]
        text_max_characters: u32,
        #[arg(long)]
        include_highlights: bool,
        #[arg(long)]
        start_published_date: Option<String>,
        #[arg(long, value_delimiter = ',')]
        include_domains: Vec<String>,
        #[arg(long, value_delimiter = ',')]
        exclude_domains: Vec<String>,
        #[arg(long)]
        category: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[command(flatten)]
        output: OutputArgs,
        #[arg(long)]
        verbose: bool,
    },
    Similar {
        url: String,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..=100))]
        num_results: u16,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[command(flatten)]
        output: OutputArgs,
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum ConfigCommand {
    Path,
    #[command(visible_alias = "ls")]
    List,
    Set {
        key: String,
        #[arg(
            help = "string, number, or boolean as plain text; an array as a TOML literal such as '[\"a\"]'; or - for standard input. Argument values may be saved in shell history"
        )]
        value: String,
    },
    Unset {
        key: String,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum Language {
    Zh,
    En,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum ExaSearchType {
    Neural,
    Keyword,
    Auto,
}

impl From<ExaSearchType> for SearchType {
    fn from(value: ExaSearchType) -> Self {
        match value {
            ExaSearchType::Neural => Self::Neural,
            ExaSearchType::Keyword => Self::Keyword,
            ExaSearchType::Auto => Self::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Json,
    Markdown,
}

/// Where a rendered command result is written besides standard output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputTarget {
    /// Write the rendered result to the file and emit it on standard output unchanged.
    Tee(PathBuf),
    /// Write the rendered result to the file and emit only a short receipt on success.
    Receipt(PathBuf),
}

impl OutputTarget {
    /// Returns the destination file.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        match self {
            Self::Tee(path) | Self::Receipt(path) => path,
        }
    }
}

#[derive(Clone, Debug, Args)]
pub(super) struct OutputArgs {
    #[arg(long)]
    output: Option<PathBuf>,
    #[arg(long, requires = "output")]
    receipt: bool,
}

impl OutputArgs {
    pub(super) fn target(&self) -> Option<OutputTarget> {
        let path = self.output.clone()?;
        Some(if self.receipt {
            OutputTarget::Receipt(path)
        } else {
            OutputTarget::Tee(path)
        })
    }
}

/// The output format of a command whose result has a body of its own, such as a search answer or
/// fetched content: JSON, Markdown, or that body alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum ContentOutputFormat {
    Json,
    Markdown,
    Content,
}

fn parse_provider(value: &str) -> Result<ProviderId, String> {
    ProviderId::parse(value).ok_or_else(|| {
        format!(
            "invalid provider `{value}`; expected one of: {}",
            catalog::registrations()
                .iter()
                .map(|registration| registration.id.name())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })
}

/// Parses `forager` followed by `arguments` with the command-line definition, for the tests of
/// each command's argument rules.
#[cfg(test)]
fn parse(arguments: &[&str]) -> Result<Command, clap::Error> {
    let arguments = std::iter::once("forager").chain(arguments.iter().copied());
    Cli::try_parse_from(arguments).map(|cli| cli.command)
}

/// The `map` command's argument rules, proven by parsing its definition in process.
#[cfg(test)]
mod tavily_map_tests {
    use super::{Command, parse};

    /// The depth, breadth, limit and timeout a `map` with `options` asks for.
    fn map_options(options: &[&str]) -> (u16, u16, u16, u64) {
        let arguments = [&["map", "https://docs.example.test"][..], options].concat();
        match parse(&arguments).expect("the parser accepts the values") {
            Command::Map {
                max_depth,
                max_breadth,
                limit,
                timeout,
                ..
            } => (max_depth, max_breadth, limit, timeout),
            _ => unreachable!("the arguments name map"),
        }
    }

    #[test]
    fn a_map_without_options_asks_for_the_documented_defaults() {
        assert_eq!(map_options(&[]), (1, 20, 50, 150));
    }

    #[test]
    fn the_boundary_values_of_each_range_are_accepted() {
        let boundaries = [
            ["--max-depth", "1", "--max-breadth", "1", "--timeout", "10"],
            [
                "--max-depth",
                "5",
                "--max-breadth",
                "500",
                "--timeout",
                "150",
            ],
        ]
        .map(|options| map_options(&options));

        assert_eq!(boundaries, [(1, 1, 50, 10), (5, 500, 50, 150)]);
    }

    #[test]
    fn option_values_outside_their_range_are_refused_naming_the_option() {
        for (option, value) in [
            ("--timeout", "9"),
            ("--timeout", "151"),
            ("--timeout", "600"),
            ("--max-depth", "0"),
            ("--max-depth", "6"),
            ("--max-breadth", "0"),
            ("--max-breadth", "501"),
            ("--limit", "0"),
        ] {
            let error = parse(&["map", "https://docs.example.test", option, value])
                .expect_err("the parser refuses the value");

            assert!(
                error.to_string().contains(option),
                "{option} {value}: {error}"
            );
        }
    }

    #[test]
    fn the_content_format_is_refused_because_a_site_map_has_no_body() {
        let error = parse(&["map", "https://docs.example.test", "--format", "content"])
            .expect_err("the parser refuses the format");

        assert!(error.to_string().contains("--format"), "{error}");
    }
}

/// The `exa` commands' argument rules, proven by parsing their definitions in process.
#[cfg(test)]
mod exa_tests {
    use super::{Command, ExaCommand, ExaSearchType, parse};

    #[test]
    fn an_exa_search_without_options_asks_for_five_automatic_results_without_contents() {
        let parsed = parse(&["exa", "search", "rust async drop"]).expect("the parser accepts it");

        let Command::Exa {
            command:
                ExaCommand::Search {
                    num_results,
                    search_type,
                    include_text,
                    include_highlights,
                    ..
                },
        } = parsed
        else {
            unreachable!("the arguments name exa search")
        };
        assert_eq!(
            (
                num_results,
                matches!(search_type, ExaSearchType::Auto),
                include_text,
                include_highlights,
            ),
            (5, true, false, false)
        );
    }

    #[test]
    fn a_result_count_outside_its_range_is_refused_naming_the_option() {
        for (command, target) in [
            ("search", "rust async drop"),
            ("similar", "https://example.test/source"),
        ] {
            for count in ["0", "101"] {
                let error = parse(&["exa", command, target, "--num-results", count])
                    .expect_err("the parser refuses the count");

                assert!(
                    error.to_string().contains("--num-results"),
                    "{command} {count}: {error}"
                );
            }
        }
    }
}

/// The `search` command's argument rules, proven by parsing its definition in process.
#[cfg(test)]
mod search_tests {
    use super::{Command, parse};
    use crate::types::Capability;

    /// The capabilities a `search` declares with `--capabilities declaration`.
    fn declared(declaration: &str) -> Vec<Capability> {
        match parse(&["search", "Declared", "--capabilities", declaration])
            .expect("the parser accepts the declaration")
        {
            Command::Search {
                capabilities: Some(capabilities),
                ..
            } => capabilities.iter().collect(),
            _ => unreachable!("the arguments name search with a declaration"),
        }
    }

    #[test]
    fn a_search_without_options_declares_no_capabilities_and_asks_for_no_extra_sources() {
        let parsed = parse(&["search", "Ordinary search"]).expect("the parser accepts it");

        let Command::Search {
            capabilities,
            extra_sources,
            ..
        } = parsed
        else {
            unreachable!("the arguments name search")
        };
        assert_eq!((capabilities.is_none(), extra_sources), (true, 0));
    }

    #[test]
    fn a_capability_declaration_is_deduplicated_in_vocabulary_order_whatever_its_case() {
        assert_eq!(
            (
                declared("WEB_SEARCH,docs_search,web_search"),
                declared("none"),
            ),
            (
                vec![Capability::DocsSearch, Capability::WebSearch],
                Vec::new()
            )
        );
    }

    #[test]
    fn unknown_empty_and_mixed_none_declarations_are_usage_errors_naming_the_option() {
        for declaration in ["unknown", "", "docs_search,", "none,web_search"] {
            let error = parse(&[
                "search",
                "Invalid declaration",
                "--capabilities",
                declaration,
            ])
            .expect_err("the parser refuses the declaration");

            assert_eq!(
                (
                    error.exit_code(),
                    error.to_string().contains("--capabilities")
                ),
                (2, true),
                "{declaration:?}: {error}"
            );
        }
    }

    #[test]
    fn more_than_twenty_extra_sources_are_a_usage_error_naming_the_option() {
        let accepted = parse(&["search", "In range", "--extra-sources", "20"]);
        let error = parse(&["search", "Out of range", "--extra-sources", "21"])
            .expect_err("the parser refuses the count");

        assert_eq!(
            (
                matches!(
                    accepted,
                    Ok(Command::Search {
                        extra_sources: 20,
                        ..
                    })
                ),
                error.exit_code(),
                error.to_string().contains("--extra-sources"),
            ),
            (true, 2, true),
            "{error}"
        );
    }
}
