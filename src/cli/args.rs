use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{Map, Value};

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
                format: DocsOutputFormat::Json,
                ..
            } | Command::Research {
                format: DocsOutputFormat::Json,
                ..
            } | Command::Fetch {
                format: DocsOutputFormat::Json,
                ..
            }
        )
    }

    /// Returns the requested tee destination for a JSON preflight failure.
    #[must_use]
    pub fn json_preflight_output(&self) -> Option<PathBuf> {
        match &self.command {
            Command::Search {
                format: DocsOutputFormat::Json,
                output,
                ..
            }
            | Command::Research {
                format: DocsOutputFormat::Json,
                output,
                ..
            }
            | Command::Fetch {
                format: DocsOutputFormat::Json,
                output,
                ..
            } => output.clone(),
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
        #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
        format: DocsOutputFormat,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
    #[command(visible_alias = "rs")]
    Research {
        query: String,
        #[arg(long)]
        plan: Option<String>,
        #[arg(long, value_enum, default_value_t = ResearchBudgetArg::Standard)]
        budget: ResearchBudgetArg,
        #[arg(long)]
        evidence_dir: Option<PathBuf>,
        #[arg(long, default_value_t = FallbackPolicy::Auto)]
        fallback: FallbackPolicy,
        #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
        format: DocsOutputFormat,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
    #[command(visible_alias = "f")]
    Fetch {
        url: String,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
        format: DocsOutputFormat,
        #[arg(long)]
        output: Option<PathBuf>,
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
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
    #[command(visible_alias = "as")]
    Anysearch {
        #[command(subcommand)]
        command: AnysearchCommand,
    },
    #[command(visible_alias = "c7")]
    Context7 {
        #[command(subcommand)]
        command: Context7Command,
    },
    Exa {
        #[command(subcommand)]
        command: ExaCommand,
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
        #[arg(long, requires = "live", default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: u64,
        #[arg(long, requires = "live", value_name = "CASE_ID=EVIDENCE_URL")]
        outage_evidence: Vec<String>,
        #[arg(long, hide = true, conflicts_with = "live", value_parser = ["C04", "C05", "C06", "OUTAGE"])]
        probe: Option<String>,
        #[arg(long, hide = true, requires = "probe", default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..))]
        probe_timeout: u64,
        #[arg(long, hide = true, requires = "probe")]
        probe_url: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum AnysearchCommand {
    Domains {
        domain: String,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
    Search {
        query: String,
        #[arg(long, requires = "sub_domain")]
        domain: Option<String>,
        #[arg(long, requires = "domain")]
        sub_domain: Option<String>,
        #[arg(long, value_parser = parse_json_object)]
        sub_domain_params: Option<Map<String, Value>>,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..=100))]
        max_results: u16,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
}

#[derive(Debug, Subcommand)]
pub(super) enum Context7Command {
    Library {
        name: String,
        #[arg(default_value = "")]
        query: String,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = OutputFormat::Json)]
        format: OutputFormat,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
    },
    Docs {
        library_id: String,
        query: String,
        #[arg(long, value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
        #[arg(long, value_enum, default_value_t = DocsOutputFormat::Json)]
        format: DocsOutputFormat,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        verbose: bool,
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
        #[arg(long)]
        output: Option<PathBuf>,
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
        #[arg(long)]
        output: Option<PathBuf>,
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
            help = "TOML value, or - for standard input; argument values may be saved in shell history"
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

#[derive(Clone, Copy, Debug, ValueEnum)]
pub(super) enum ResearchBudgetArg {
    Quick,
    Standard,
    Deep,
}

impl From<ResearchBudgetArg> for crate::research::ResearchBudget {
    fn from(value: ResearchBudgetArg) -> Self {
        match value {
            ResearchBudgetArg::Quick => Self::Quick,
            ResearchBudgetArg::Standard => Self::Standard,
            ResearchBudgetArg::Deep => Self::Deep,
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum DocsOutputFormat {
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

fn parse_json_object(value: &str) -> Result<Map<String, Value>, String> {
    serde_json::from_str::<Value>(value)
        .map_err(|_| "--sub-domain-params must be a valid JSON object".to_owned())?
        .as_object()
        .cloned()
        .ok_or_else(|| "--sub-domain-params must be a single JSON object".to_owned())
}
