use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::catalog::{self, LiveTier, ProviderId};
use crate::providers::SearchType;
use crate::telemetry::RetrievalCommand;
use crate::types::{CapabilitySet, FallbackPolicy, Platform};

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

    /// What kind of command this is: a retrieval command, which writes Invocation Telemetry,
    /// or an operator command, which does not.
    #[must_use]
    pub fn kind(&self) -> CommandKind {
        match &self.command {
            Command::Search { .. } => CommandKind::Retrieval(RetrievalCommand::Search),
            Command::Fetch { .. } => CommandKind::Retrieval(RetrievalCommand::Fetch),
            Command::Map { .. } => CommandKind::Retrieval(RetrievalCommand::Map),
            Command::Exa {
                command: ExaCommand::Search { .. },
            } => CommandKind::Retrieval(RetrievalCommand::Exa("search")),
            Command::Exa {
                command: ExaCommand::Similar { .. },
            } => CommandKind::Retrieval(RetrievalCommand::Exa("similar")),
            Command::Platform { command } => {
                let (platform, operation) = command.operation();
                CommandKind::Retrieval(RetrievalCommand::Platform(platform, operation))
            }
            Command::Browser { .. } => CommandKind::Browser,
            Command::Config { .. } => CommandKind::Config,
            Command::Setup { .. } => CommandKind::Setup,
            Command::Doctor { .. } => CommandKind::Doctor,
        }
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

/// The kind of a parsed command, which decides at the terminal state what it leaves behind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandKind {
    /// A command that reaches a provider for its result.
    Retrieval(RetrievalCommand),
    /// `config`.
    Config,
    /// `doctor`, in every mode.
    Doctor,
    /// `setup`.
    Setup,
    /// `browser`.
    Browser,
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
        /// The provider to check alone, or, with `--live`, whose Live Checks to run.
        #[arg(long, value_parser = parse_provider)]
        provider: Option<ProviderId>,
        /// Run only the checks that send no request.
        #[arg(long, conflicts_with_all = ["provider", "timeout", "live"])]
        offline: bool,
        /// Run the Live Checks against the real services.
        #[arg(long)]
        live: bool,
        /// The Live Tiers to run; without it, `free` and `metered`.
        #[arg(long, requires = "live", value_delimiter = ',', num_args = 1.., value_parser = parse_tier)]
        tier: Vec<LiveTier>,
        /// Run only the Live Checks of this platform's operations.
        #[arg(long, requires = "live", value_parser = super::browser::parse_platform)]
        platform: Option<Platform>,
        /// List the Live Checks and whether the selection runs them, without running any.
        #[arg(long, requires = "live")]
        list: bool,
        /// Mark the run as unattended: refuse the attended tier and leave out expensive and
        /// manual-only checks.
        #[arg(long, requires = "live")]
        unattended: bool,
        /// Summarise, from local records and without a request, how each provider behaved in
        /// real use and whether the scheduled runs are still running.
        #[arg(long, conflicts_with_all = ["provider", "offline", "live", "timeout"])]
        health: bool,
        /// The days the health summary covers; 7 by default.
        #[arg(long, requires = "health", value_parser = clap::value_parser!(u32).range(1..=i64::from(crate::health::MAX_DAYS)))]
        days: Option<u32>,
        /// Seconds the checks may take; 30 by default, 420 for the whole live run.
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        /// Runs one supplemental web search provider alone, for its Live Check.
        #[arg(long, hide = true, value_parser = parse_provider, requires = "query", conflicts_with_all = ["live", "offline", "provider", "health"])]
        supplemental_web_search: Option<ProviderId>,
        #[arg(long, hide = true, requires = "supplemental_web_search")]
        query: Option<String>,
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

fn parse_tier(value: &str) -> Result<LiveTier, String> {
    LiveTier::parse(value).ok_or_else(|| {
        format!(
            "invalid tier `{value}`; expected one of: {}",
            LiveTier::ALL.map(LiveTier::as_str).join(", ")
        )
    })
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
    use std::time::Duration;

    use super::{Command, ExaCommand, ExaSearchType, parse};
    use crate::app::dispatch::exa_deadline;

    /// The command deadline of the `exa` command `arguments` when Exa's configured timeout is
    /// 42 seconds.
    fn deadline(arguments: &[&str]) -> Duration {
        let Command::Exa {
            command: ExaCommand::Search { timeout, .. } | ExaCommand::Similar { timeout, .. },
        } = parse(arguments).expect("the parser accepts the arguments")
        else {
            unreachable!("the arguments name an exa command")
        };
        exa_deadline(timeout, 42)
    }

    #[test]
    fn an_exa_command_has_the_configured_timeout_unless_its_timeout_names_another_deadline() {
        for (command, target) in [
            ("search", "rust async drop"),
            ("similar", "https://example.test/source"),
        ] {
            assert_eq!(
                (
                    deadline(&["exa", command, target]),
                    deadline(&["exa", command, target, "--timeout", "7"]),
                ),
                (Duration::from_secs(42), Duration::from_secs(7)),
                "{command}"
            );
        }
    }

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

/// The `fetch` command's argument rules, proven by parsing its definition in process.
#[cfg(test)]
mod fetch_tests {
    use std::time::Duration;

    use super::{Command, parse};
    use crate::app::dispatch::fetch_deadline;

    /// The command deadline of the `fetch` command `arguments`.
    fn deadline(arguments: &[&str]) -> Duration {
        match parse(arguments).expect("the parser accepts the arguments") {
            Command::Fetch { timeout, .. } => fetch_deadline(timeout),
            _ => unreachable!("the arguments name fetch"),
        }
    }

    #[test]
    fn a_fetch_has_three_minutes_unless_its_timeout_names_another_deadline() {
        let url = "https://example.test/article";

        assert_eq!(
            (
                deadline(&["fetch", url]),
                deadline(&["fetch", url, "--timeout", "7"]),
            ),
            (Duration::from_mins(3), Duration::from_secs(7))
        );
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

/// The `doctor` command's argument rules, proven by parsing its definition in process.
#[cfg(test)]
mod doctor_tests {
    use clap::CommandFactory;

    use super::{Cli, Command, parse};
    use crate::catalog::LiveTier;

    #[test]
    fn the_offline_mode_refuses_a_provider_and_a_timeout_naming_the_option() {
        for option in [["--provider", "exa"], ["--timeout", "5"]] {
            let error = parse(&["doctor", "--offline", option[0], option[1]])
                .expect_err("the parser refuses the combination");

            assert_eq!(
                (error.exit_code(), error.to_string().contains(option[0])),
                (2, true),
                "{option:?}: {error}"
            );
        }
    }

    #[test]
    fn the_live_selection_options_require_the_live_mode_naming_it() {
        for option in [
            &["--tier", "free"][..],
            &["--platform", "hn"],
            &["--list"],
            &["--unattended"],
        ] {
            let arguments = [&["doctor"][..], option].concat();
            let error = parse(&arguments).expect_err("the parser refuses the option alone");

            assert_eq!(
                (error.exit_code(), error.to_string().contains("--live")),
                (2, true),
                "{option:?}: {error}"
            );
        }
    }

    #[test]
    fn the_live_mode_refuses_the_offline_mode() {
        let error = parse(&["doctor", "--live", "--offline"])
            .expect_err("the parser refuses the combination");

        assert_eq!(
            (error.exit_code(), error.to_string().contains("--offline")),
            (2, true),
            "{error}"
        );
    }

    #[test]
    fn an_unknown_tier_is_refused_naming_the_accepted_tiers() {
        let error = parse(&["doctor", "--live", "--tier", "cheap"])
            .expect_err("the parser refuses the tier");

        assert_eq!(
            (
                error.exit_code(),
                error
                    .to_string()
                    .contains("expected one of: free, metered, attended")
            ),
            (2, true),
            "{error}"
        );
    }

    #[test]
    fn tiers_parse_from_a_comma_separated_list_or_a_repeated_option() {
        let tiers = [
            &["doctor", "--live", "--tier", "free,metered"][..],
            &["doctor", "--live", "--tier", "free", "--tier", "metered"],
        ]
        .map(|arguments| match parse(arguments) {
            Ok(Command::Doctor { tier, .. }) => tier,
            other => panic!("{arguments:?} parse as doctor: {other:?}"),
        });

        assert_eq!(
            tiers,
            [[LiveTier::Free, LiveTier::Metered]; 2].map(Vec::from)
        );
    }

    #[test]
    fn the_health_summary_refuses_the_other_modes_and_takes_one_to_fifteen_days() {
        let refused = [
            &["doctor", "--health", "--live"][..],
            &["doctor", "--health", "--offline"],
            &["doctor", "--health", "--provider", "exa"],
            &["doctor", "--health", "--timeout", "5"],
            &["doctor", "--health", "--days", "0"],
            &["doctor", "--health", "--days", "16"],
            &["doctor", "--days", "3"],
        ]
        .map(|arguments| {
            parse(arguments)
                .map(|_| ())
                .map_err(|error| error.exit_code())
        });
        let days = [
            &["doctor", "--health"][..],
            &["doctor", "--health", "--days", "15"],
        ]
        .map(|arguments| match parse(arguments) {
            Ok(Command::Doctor { health, days, .. }) => (health, days),
            other => panic!("{arguments:?} parse as doctor: {other:?}"),
        });

        assert_eq!(
            (refused, days),
            ([Err(2); 7], [(true, None), (true, Some(15))])
        );
    }

    #[test]
    fn the_supplemental_web_search_entry_stays_out_of_the_doctor_help() {
        let mut cli = Cli::command();
        let help = cli
            .find_subcommand_mut("doctor")
            .expect("doctor command")
            .render_long_help()
            .to_string();

        assert_eq!(
            (help.contains("--live"), help.contains("supplemental")),
            (true, false),
            "{help}"
        );
    }
}
