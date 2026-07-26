//! CLI parsing and application dispatch.

use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

use clap::{Parser, Subcommand, ValueEnum};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::config::{self, ConfigError, ConfigLocation, EditError};
use crate::net::{self, RetryPolicy};
use crate::providers::{
    self, AnysearchDomainsRequest, AnysearchSearchRequest, Context7DocsRequest,
    Context7LibraryRequest, ExaSearchRequest, ExaSimilarRequest, FetchRequest, MapRequest,
    SearchType,
};
use crate::types::{
    AnysearchOutcome, CapabilitySet, Context7Outcome, Deadline, FetchOutcome, MapOutcome,
};
use crate::types::{JournalOutcome, SearchOutcome};

pub use crate::providers::ProviderError;
pub use crate::types::ExaOutcome;

/// Parsed `forager` command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "forager", infer_subcommands = false)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(visible_alias = "s")]
    Search {
        query: String,
        #[arg(long)]
        capabilities: Option<CapabilitySet>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = 0)]
        extra_sources: u16,
        #[arg(long, default_value = "balanced", value_parser = ["fast", "balanced", "strict"])]
        validation: String,
        #[arg(long, value_parser = ["auto", "off"])]
        fallback: Option<String>,
        #[arg(long, default_value_t = 180, value_parser = clap::value_parser!(u64).range(1..))]
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
        #[arg(long, default_value_t = 1, value_parser = clap::value_parser!(u16).range(1..))]
        max_depth: u16,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u16).range(1..))]
        max_breadth: u16,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u16).range(1..))]
        limit: u16,
        #[arg(long, default_value_t = 150, value_parser = clap::value_parser!(u64).range(1..))]
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
}

#[derive(Debug, Subcommand)]
enum AnysearchCommand {
    Domains {
        domain: Option<String>,
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
enum Context7Command {
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
enum ExaCommand {
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
enum ConfigCommand {
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
enum Language {
    Zh,
    En,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum ExaSearchType {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum DocsOutputFormat {
    Json,
    Markdown,
    Content,
}

/// A command result ready for binary-side rendering.
#[derive(Debug)]
pub enum CommandOutput {
    /// Text already produced by a non-network command.
    Text {
        /// Standard output without an automatically appended newline.
        stdout: String,
        /// Optional diagnostic emitted on standard error.
        stderr: Option<String>,
        /// Process exit status.
        exit_code: u8,
    },
    /// One completed Default Search Invocation.
    Search {
        /// Main-search terminal result.
        result: Result<SearchOutcome, ProviderError>,
        /// Search Result Journal side-channel outcome.
        journal: JournalOutcome,
        /// Requested output format.
        format: DocsOutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
        /// Whether full provider attempts should be rendered inline.
        verbose: bool,
    },
    /// Typed Exa terminal state for binary-side formatting and tee output.
    Exa {
        /// Provider result.
        result: Result<ExaOutcome, ProviderError>,
        /// Requested output format.
        format: OutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
    },
    /// Typed Context7 terminal state for binary-side formatting and tee output.
    Context7 {
        /// Provider result.
        result: Result<Context7Outcome, ProviderError>,
        /// Requested output format.
        format: DocsOutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
    },
    /// Typed AnySearch terminal state for binary-side formatting and tee output.
    Anysearch {
        /// Provider result.
        result: Result<AnysearchOutcome, ProviderError>,
        /// Requested output format.
        format: OutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
    },
    /// Typed Web Fetch terminal state for binary-side formatting and tee output.
    Fetch {
        /// Fetch result.
        result: Result<FetchOutcome, ProviderError>,
        /// Requested output format.
        format: DocsOutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
    },
    /// Typed Tavily site map terminal state for binary-side formatting and tee output.
    Map {
        /// Site map result.
        result: Result<MapOutcome, ProviderError>,
        /// Requested output format.
        format: OutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
    },
}

struct AppContext<P> {
    provider: P,
    runtime: tokio::runtime::Runtime,
}

struct NetworkDependencies {
    config: config::RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    runtime: tokio::runtime::Runtime,
}

struct FetchContext {
    config: config::WebFetchRuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    runtime: tokio::runtime::Runtime,
}

struct SearchContext {
    config: config::RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    model_breakers: std::sync::Arc<providers::ModelBreakers>,
    journal: config::JournalRuntimeConfig,
    default_model: String,
    endpoint_host: String,
    timeout: Duration,
    runtime: tokio::runtime::Runtime,
}

impl NetworkDependencies {
    fn load() -> Result<Self, AppError> {
        let config = config::runtime_config()?;
        let retry_policy = RetryPolicy::new(
            config.retry.max_attempts,
            config.retry.multiplier,
            Duration::from_secs(config.retry.max_wait_seconds),
        );
        let client = net::build_client(config.ssl_verify)
            .map_err(|error| AppError::Runtime(format!("cannot build HTTP client: {error}")))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| AppError::Runtime(format!("cannot start network runtime: {error}")))?;
        Ok(Self {
            config,
            client,
            retry_policy,
            runtime,
        })
    }
}

impl FetchContext {
    fn load(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config.web_fetch;
        if config.configured_provider_count() == 0 {
            return Err(AppError::Config(ConfigError::Message(
                "capabilities.web_fetch.order has no configured provider".into(),
            )));
        }
        Ok(Self {
            config,
            client: dependencies.client,
            retry_policy: dependencies.retry_policy,
            deadline: Deadline::new(Duration::from_secs(timeout.unwrap_or(180))),
            runtime: dependencies.runtime,
        })
    }

    fn fetch(self, request: FetchRequest) -> Result<FetchOutcome, ProviderError> {
        self.runtime.block_on(crate::engine::fetch(
            request,
            self.config,
            self.client,
            self.retry_policy,
            self.deadline,
        ))
    }
}

impl SearchContext {
    fn load(timeout_seconds: u64) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        let journal = config.journal.clone();
        if config.main_search.configured_provider_count() == 0 {
            return Err(AppError::Config(ConfigError::Message(
                "search.backends has no configured credentials".into(),
            )));
        }
        let timeout = Duration::from_secs(timeout_seconds);
        let default_model = config.main_search.default_model().to_owned();
        let endpoint_host = config.main_search.default_endpoint_host();
        Ok(Self {
            config,
            client: dependencies.client,
            retry_policy: dependencies.retry_policy,
            model_breakers: std::sync::Arc::new(providers::ModelBreakers::default()),
            journal,
            default_model,
            endpoint_host,
            timeout,
            runtime: dependencies.runtime,
        })
    }

    fn search(
        self,
        mut request: providers::SearchRequest,
        capabilities: Option<&CapabilitySet>,
        extra_sources: u16,
        fallback_override: Option<&str>,
    ) -> (Result<SearchOutcome, ProviderError>, JournalOutcome) {
        let fallback = fallback_override
            .unwrap_or(&self.config.main_search.fallback)
            .to_owned();
        request.allow_model_fallback = fallback != "off";
        let query = request.query.clone();
        let journal_capabilities =
            capabilities.map(|capabilities| capabilities.iter().collect::<Vec<_>>());
        let model = request
            .model
            .clone()
            .unwrap_or_else(|| self.default_model.clone());
        let started = Instant::now();
        let deadline = Deadline::new(self.timeout);
        let mut result = self.runtime.block_on(crate::engine::search(
            request,
            self.config.main_search.clone(),
            &fallback,
            self.client.clone(),
            self.retry_policy,
            deadline,
            self.model_breakers,
        ));
        if let (Ok(outcome), Some(capabilities)) = (&mut result, capabilities) {
            outcome.capabilities = capabilities.iter().collect();
            self.runtime.block_on(crate::engine::execute_capabilities(
                outcome,
                &query,
                capabilities,
                extra_sources.max(1),
                &self.config,
                crate::engine::CapabilityExecution::new(
                    &fallback,
                    self.client.clone(),
                    self.retry_policy,
                    deadline,
                ),
            ));
        }
        let journal = crate::journal::record_search(
            &self.journal,
            crate::journal::SearchRecord {
                query: &query,
                budget: self.timeout,
                elapsed: started.elapsed(),
                model: &model,
                endpoint_host: &self.endpoint_host,
                capabilities: journal_capabilities.as_deref(),
                result: &result,
            },
        );
        (result, journal)
    }
}

impl AppContext<providers::Exa> {
    fn for_exa(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        if config.exa.keys.is_empty() {
            return Err(AppError::Config(ConfigError::Message(
                "providers.exa.keys has no configured credentials".into(),
            )));
        }
        let command_timeout = timeout.unwrap_or(config.exa.timeout_seconds);
        let provider = providers::build_exa(
            config.exa,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(command_timeout)),
        );
        Ok(Self {
            provider,
            runtime: dependencies.runtime,
        })
    }

    fn exa_search(self, request: ExaSearchRequest) -> Result<ExaOutcome, ProviderError> {
        self.runtime.block_on(self.provider.search(request))
    }

    fn exa_similar(self, request: ExaSimilarRequest) -> Result<ExaOutcome, ProviderError> {
        self.runtime.block_on(self.provider.similar(request))
    }
}

impl AppContext<providers::TavilyMap> {
    fn for_tavily_map(timeout: u64) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config.tavily;
        if config.keys.is_empty() {
            return Err(AppError::Config(ConfigError::Message(
                "providers.tavily.keys has no configured credentials".into(),
            )));
        }
        let provider = providers::build_tavily_map(
            config,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(timeout)),
        );
        Ok(Self {
            provider,
            runtime: dependencies.runtime,
        })
    }

    fn map(self, request: MapRequest) -> Result<MapOutcome, ProviderError> {
        self.runtime.block_on(self.provider.map(request))
    }
}

impl AppContext<providers::Context7> {
    fn for_context7(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        if config.context7.keys.is_empty() {
            return Err(AppError::Config(ConfigError::Message(
                "providers.context7.keys has no configured credentials".into(),
            )));
        }
        let command_timeout = timeout.unwrap_or(config.context7.timeout_seconds);
        let provider = providers::build_context7(
            config.context7,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(command_timeout)),
        );
        Ok(Self {
            provider,
            runtime: dependencies.runtime,
        })
    }

    fn library(self, request: Context7LibraryRequest) -> Result<Context7Outcome, ProviderError> {
        self.runtime.block_on(self.provider.library(request))
    }

    fn docs(self, request: Context7DocsRequest) -> Result<Context7Outcome, ProviderError> {
        self.runtime.block_on(self.provider.docs(request))
    }
}

impl AppContext<providers::Anysearch> {
    fn for_anysearch(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        if config.anysearch.keys.is_empty() {
            return Err(AppError::Config(ConfigError::Message(
                "providers.anysearch.keys has no configured credentials".into(),
            )));
        }
        let command_timeout = timeout.unwrap_or(config.anysearch.timeout_seconds);
        let provider = providers::build_anysearch(
            config.anysearch,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(command_timeout)),
        );
        Ok(Self {
            provider,
            runtime: dependencies.runtime,
        })
    }

    fn domains(self, request: AnysearchDomainsRequest) -> Result<AnysearchOutcome, ProviderError> {
        self.runtime.block_on(self.provider.domains(request))
    }

    fn search(self, request: AnysearchSearchRequest) -> Result<AnysearchOutcome, ProviderError> {
        self.runtime.block_on(self.provider.search(request))
    }
}

/// Executes a parsed command.
///
/// # Errors
///
/// Returns [`AppError`] when arguments, configuration, standard input, or
/// persistence are invalid.
pub fn run(cli: Cli) -> Result<CommandOutput, AppError> {
    match cli.command {
        Command::Search {
            query,
            capabilities,
            model,
            extra_sources,
            validation: _,
            fallback,
            timeout,
            format,
            output,
            verbose,
        } => {
            let (result, journal) = SearchContext::load(timeout)?.search(
                providers::SearchRequest {
                    query,
                    model,
                    allow_model_fallback: true,
                    verbose,
                },
                capabilities.as_ref(),
                extra_sources,
                fallback.as_deref(),
            );
            Ok(CommandOutput::Search {
                result,
                journal,
                format,
                output,
                verbose,
            })
        }
        Command::Fetch {
            url,
            timeout,
            format,
            output,
            verbose,
        } => Ok(CommandOutput::Fetch {
            result: FetchContext::load(timeout)?.fetch(FetchRequest { url, verbose }),
            format,
            output,
        }),
        Command::Map {
            url,
            instructions,
            max_depth,
            max_breadth,
            limit,
            timeout,
            format,
            output,
            verbose,
        } => Ok(CommandOutput::Map {
            result: AppContext::<providers::TavilyMap>::for_tavily_map(timeout)?.map(MapRequest {
                url,
                instructions,
                max_depth,
                max_breadth,
                limit,
                timeout_seconds: timeout,
                verbose,
            }),
            format,
            output,
        }),
        Command::Anysearch {
            command:
                AnysearchCommand::Domains {
                    domain,
                    timeout,
                    format,
                    output,
                    verbose,
                },
        } => {
            let domain = domain.ok_or_else(|| {
                AppError::Argument(
                    "anysearch domains requires a parent DOMAIN before contacting AnySearch".into(),
                )
            })?;
            if domain.contains('.') {
                return Err(AppError::Argument(
                    "DOMAIN must be a parent domain without a dotted sub-domain".into(),
                ));
            }
            let result = AppContext::<providers::Anysearch>::for_anysearch(timeout)?
                .domains(AnysearchDomainsRequest { domain, verbose });
            Ok(CommandOutput::Anysearch {
                result,
                format,
                output,
            })
        }
        Command::Anysearch {
            command:
                AnysearchCommand::Search {
                    query,
                    domain,
                    sub_domain,
                    sub_domain_params,
                    max_results,
                    timeout,
                    format,
                    output,
                    verbose,
                },
        } => {
            let sub_domain_params = sub_domain_params.unwrap_or_default();
            if domain.as_deref().is_some_and(|value| value.contains('.'))
                || sub_domain
                    .as_deref()
                    .is_some_and(|value| value.contains('.'))
            {
                return Err(AppError::Argument(
                    "dotted domain shorthand is unsupported; pass separate --domain and --sub-domain values"
                        .into(),
                ));
            }
            if domain.as_deref() == Some("security") && sub_domain.as_deref() == Some("cve") {
                return Err(AppError::Argument(
                    "legacy sub-domain aliases are unsupported; migrate to --domain security --sub-domain vuln"
                        .into(),
                ));
            }
            if sub_domain_params.keys().any(|name| {
                ["query", "domain", "sub_domain", "max_results"].contains(&name.as_str())
            }) {
                return Err(AppError::Argument(
                    "--sub-domain-params cannot override reserved fields".into(),
                ));
            }
            if !sub_domain_params.is_empty() && domain.is_none() {
                return Err(AppError::Argument(
                    "--sub-domain-params requires both --domain and --sub-domain".into(),
                ));
            }
            let result = AppContext::<providers::Anysearch>::for_anysearch(timeout)?.search(
                AnysearchSearchRequest {
                    query,
                    domain,
                    sub_domain,
                    sub_domain_params,
                    max_results,
                    verbose,
                },
            );
            Ok(CommandOutput::Anysearch {
                result,
                format,
                output,
            })
        }
        Command::Context7 {
            command:
                Context7Command::Library {
                    name,
                    query,
                    timeout,
                    format,
                    output,
                    verbose,
                },
        } => {
            let result = AppContext::<providers::Context7>::for_context7(timeout)?.library(
                Context7LibraryRequest {
                    name,
                    query,
                    verbose,
                },
            );
            Ok(CommandOutput::Context7 {
                result,
                format: match format {
                    OutputFormat::Json => DocsOutputFormat::Json,
                    OutputFormat::Markdown => DocsOutputFormat::Markdown,
                },
                output,
            })
        }
        Command::Context7 {
            command:
                Context7Command::Docs {
                    library_id,
                    query,
                    timeout,
                    format,
                    output,
                    verbose,
                },
        } => {
            let result = AppContext::<providers::Context7>::for_context7(timeout)?.docs(
                Context7DocsRequest {
                    library_id,
                    query,
                    verbose,
                },
            );
            Ok(CommandOutput::Context7 {
                result,
                format,
                output,
            })
        }
        Command::Exa {
            command:
                ExaCommand::Search {
                    query,
                    num_results,
                    search_type,
                    include_text,
                    include_highlights,
                    start_published_date,
                    include_domains,
                    exclude_domains,
                    category,
                    timeout,
                    format,
                    output,
                    verbose,
                },
        } => run_exa_search(
            ExaSearchRequest {
                query,
                num_results,
                search_type: search_type.into(),
                include_text,
                include_highlights,
                start_published_date,
                include_domains,
                exclude_domains,
                category,
                verbose,
            },
            timeout,
            format,
            output,
        ),
        Command::Exa {
            command:
                ExaCommand::Similar {
                    url,
                    num_results,
                    timeout,
                    format,
                    output,
                    verbose,
                },
        } => run_exa_similar(
            ExaSimilarRequest {
                url,
                num_results,
                verbose,
            },
            timeout,
            format,
            output,
        ),
        Command::Config {
            command: ConfigCommand::Path,
        } => Ok(CommandOutput::Text {
            stdout: ConfigLocation::discover()?
                .config_file()
                .display()
                .to_string(),
            stderr: None,
            exit_code: 0,
        }),
        Command::Config {
            command: ConfigCommand::List,
        } => Ok(CommandOutput::Text {
            stdout: config::effective_view_json()?,
            stderr: None,
            exit_code: 0,
        }),
        Command::Config {
            command: ConfigCommand::Set { key, value },
        } => {
            let value = if value == "-" { read_stdin()? } else { value };
            config::set_file_value(&key, &value)?;
            Ok(CommandOutput::Text {
                stdout: String::new(),
                stderr: None,
                exit_code: 0,
            })
        }
        Command::Config {
            command: ConfigCommand::Unset { key },
        } => {
            let overridden = config::unset_file_value(&key)?;
            Ok(CommandOutput::Text {
                stdout: String::new(),
                stderr: overridden
                    .then(|| format!("environment override for `{key}` is still effective")),
                exit_code: 0,
            })
        }
        Command::Setup {
            non_interactive: true,
            ..
        } => {
            let path = config::create_setup_template()?;
            Ok(CommandOutput::Text {
                stdout: format!(
                    "Created {}. Run `forager doctor` to check the configuration.",
                    path.display()
                ),
                stderr: None,
                exit_code: 0,
            })
        }
        Command::Setup {
            non_interactive: false,
            lang,
        } => run_interactive_setup(lang),
    }
}

fn parse_json_object(value: &str) -> Result<Map<String, Value>, String> {
    serde_json::from_str::<Value>(value)
        .map_err(|_| "--sub-domain-params must be a valid JSON object".to_owned())?
        .as_object()
        .cloned()
        .ok_or_else(|| "--sub-domain-params must be a single JSON object".to_owned())
}

fn run_exa_search(
    request: ExaSearchRequest,
    timeout: Option<u64>,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<CommandOutput, AppError> {
    let result = AppContext::<providers::Exa>::for_exa(timeout)?.exa_search(request);
    Ok(CommandOutput::Exa {
        result,
        format,
        output,
    })
}

fn run_exa_similar(
    request: ExaSimilarRequest,
    timeout: Option<u64>,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<CommandOutput, AppError> {
    let result = AppContext::<providers::Exa>::for_exa(timeout)?.exa_similar(request);
    Ok(CommandOutput::Exa {
        result,
        format,
        output,
    })
}

fn run_interactive_setup(language: Option<Language>) -> Result<CommandOutput, AppError> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let language = match language {
        Some(language) => language,
        None => choose_language(&mut input)?,
    };
    let mut document = config::SetupDocument::load()?;

    prompt_stage(language, 2, false);
    let current_backend = document.primary_backend().to_owned();
    let backend = choose_backend(&mut input, language, &current_backend)?;
    configure_model(&mut input, language, &mut document, &backend)?;

    let classifier_configured = document.classifier_is_configured();
    prompt_stage(language, 3, classifier_configured);
    configure_classifier(&mut input, language, &mut document, classifier_configured)?;

    prompt_stage(language, 4, false);
    for provider in [
        "exa",
        "context7",
        "jina",
        "tavily",
        "firecrawl",
        "anysearch",
    ] {
        let prompt = match language {
            Language::Zh => format!("{provider} keys（逗号分隔；回车跳过并保留现值）: "),
            Language::En => {
                format!("{provider} keys (comma-separated; Enter skips and preserves): ")
            }
        };
        if let Some(keys) = read_keys(&mut input, &prompt)? {
            document.set_strings(&format!("providers.{provider}.keys"), &keys)?;
        }
    }

    document.save()?;
    let stdout = match language {
        Language::Zh => "配置已保存。请运行 `forager doctor` 检查配置。".to_owned(),
        Language::En => "Configuration saved. Run `forager doctor` to check it.".to_owned(),
    };
    Ok(CommandOutput::Text {
        stdout,
        stderr: None,
        exit_code: 0,
    })
}

fn choose_language(input: &mut impl BufRead) -> Result<Language, AppError> {
    loop {
        let value = prompt_line(input, "语言 / Language [zh/en]（默认 zh）: ")?;
        match value.trim().to_ascii_lowercase().as_str() {
            "" | "zh" | "1" => return Ok(Language::Zh),
            "en" | "2" => return Ok(Language::En),
            _ => eprintln!("请输入 zh 或 en / Enter zh or en."),
        }
    }
}

fn prompt_stage(language: Language, stage: u8, classifier_configured: bool) {
    let label = match (language, stage, classifier_configured) {
        (Language::Zh, 2, _) => "第 2 步：主模型",
        (Language::Zh, 3, true) => "第 3 步：分类器（跳过会保留现值）",
        (Language::Zh, 3, false) => "第 3 步：分类器（跳过后无法自动路由或生成 research 计划）",
        (Language::Zh, 4, _) => "第 4 步：补强 provider",
        (Language::En, 2, _) => "Step 2: main model",
        (Language::En, 3, true) => "Step 3: classifier (skip preserves current values)",
        (Language::En, 3, false) => {
            "Step 3: classifier (skipping disables automatic routing and research plans)"
        }
        (Language::En, 4, _) => "Step 4: supplemental providers",
        _ => return,
    };
    eprintln!("{label}");
}

fn choose_backend(
    input: &mut impl BufRead,
    language: Language,
    current: &str,
) -> Result<String, AppError> {
    loop {
        let prompt = match language {
            Language::Zh => {
                format!("主模型 backend [xai/openai_compatible]（默认 {current}）: ")
            }
            Language::En => {
                format!("Main backend [xai/openai_compatible] (default {current}): ")
            }
        };
        let value = prompt_line(input, &prompt)?;
        match value.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(current.to_owned()),
            "xai" | "1" => return Ok("xai".to_owned()),
            "openai_compatible" | "2" => return Ok("openai_compatible".to_owned()),
            _ => match language {
                Language::Zh => eprintln!("请输入 xai 或 openai_compatible。"),
                Language::En => eprintln!("Enter xai or openai_compatible."),
            },
        }
    }
}

fn configure_model(
    input: &mut impl BufRead,
    language: Language,
    document: &mut config::SetupDocument,
    backend: &str,
) -> Result<(), AppError> {
    let prefix = format!("providers.{backend}");
    let url_path = format!("{prefix}.url");
    let model_path = format!("{prefix}.model");
    let keys_path = format!("{prefix}.keys");
    let url = read_preserving(
        input,
        language,
        &format!("{backend} URL"),
        document.string(&url_path),
    )?;
    let keys_prompt = match language {
        Language::Zh => format!("{backend} keys（逗号分隔；回车保留现值）: "),
        Language::En => format!("{backend} keys (comma-separated; Enter preserves): "),
    };
    let keys = read_keys(input, &keys_prompt)?;
    let model = read_preserving(
        input,
        language,
        &format!("{backend} model"),
        document.string(&model_path),
    )?;

    document.set_string(&url_path, &url)?;
    if let Some(keys) = keys {
        document.set_strings(&keys_path, &keys)?;
    }
    document.set_string(&model_path, &model)?;
    Ok(())
}

fn configure_classifier(
    input: &mut impl BufRead,
    language: Language,
    document: &mut config::SetupDocument,
    configured: bool,
) -> Result<(), AppError> {
    let prompt = match (language, configured) {
        (Language::Zh, true) => "配置分类器？[Y/n]（跳过会保留现值）: ",
        (Language::Zh, false) => "配置分类器？[y/N]（跳过后无法自动路由或生成 research 计划）: ",
        (Language::En, true) => "Configure classifier? [Y/n] (skip preserves current values): ",
        (Language::En, false) => {
            "Configure classifier? [y/N] (skip disables automatic routing and research plans): "
        }
    };
    if !read_confirmation(input, prompt, configured, language)? {
        return Ok(());
    }

    let url = read_preserving(
        input,
        language,
        "classifier URL",
        document.string("classifier.url"),
    )?;
    let keys_prompt = match language {
        Language::Zh => "classifier keys（逗号分隔；回车保留现值）: ",
        Language::En => "classifier keys (comma-separated; Enter preserves): ",
    };
    let keys = read_keys(input, keys_prompt)?;
    let model = read_preserving(
        input,
        language,
        "classifier model",
        document.string("classifier.model"),
    )?;
    document.set_string("classifier.url", &url)?;
    if let Some(keys) = keys {
        document.set_strings("classifier.keys", &keys)?;
    }
    document.set_string("classifier.model", &model)?;
    Ok(())
}

fn read_confirmation(
    input: &mut impl BufRead,
    prompt: &str,
    default: bool,
    language: Language,
) -> Result<bool, AppError> {
    loop {
        let value = prompt_line(input, prompt)?;
        match value.trim().to_ascii_lowercase().as_str() {
            "" => return Ok(default),
            "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => match language {
                Language::Zh => eprintln!("请输入 y 或 n。"),
                Language::En => eprintln!("Enter y or n."),
            },
        }
    }
}

fn read_preserving(
    input: &mut impl BufRead,
    language: Language,
    label: &str,
    current: &str,
) -> Result<String, AppError> {
    let prompt = match language {
        Language::Zh => format!("{label}（回车使用当前值）: "),
        Language::En => format!("{label} (Enter keeps the current value): "),
    };
    let value = prompt_line(input, &prompt)?;
    if value.is_empty() {
        Ok(current.to_owned())
    } else {
        Ok(value)
    }
}

fn read_keys(input: &mut impl BufRead, prompt: &str) -> Result<Option<Vec<String>>, AppError> {
    let value = read_secret(
        input,
        prompt,
        io::stdin().is_terminal(),
        rpassword::read_password,
    )?;
    if value.trim().is_empty() {
        return Ok(None);
    }
    let keys = value
        .split(',')
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    Ok(Some(keys))
}

fn read_secret(
    input: &mut impl BufRead,
    prompt: &str,
    terminal: bool,
    read_hidden: impl FnOnce() -> io::Result<String>,
) -> Result<String, AppError> {
    if terminal {
        eprint!("{prompt}");
        io::stderr().flush().map_err(AppError::Stdin)?;
        read_hidden().map_err(AppError::Stdin)
    } else {
        prompt_line(input, prompt)
    }
}

fn prompt_line(input: &mut impl BufRead, prompt: &str) -> Result<String, AppError> {
    eprint!("{prompt}");
    io::stderr().flush().map_err(AppError::Stdin)?;
    let mut value = String::new();
    if input.read_line(&mut value).map_err(AppError::Stdin)? == 0 {
        return Err(AppError::Stdin(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "interactive setup input ended",
        )));
    }
    if value.ends_with("\r\n") {
        value.truncate(value.len() - 2);
    } else if value.ends_with('\n') {
        value.pop();
    }
    Ok(value)
}

fn read_stdin() -> Result<String, AppError> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(AppError::Stdin)?;
    if input.ends_with("\r\n") {
        input.truncate(input.len() - 2);
    } else if input.ends_with('\n') {
        input.pop();
    }
    Ok(input)
}

/// Application errors with stable exit categories.
#[derive(Debug, Error)]
pub enum AppError {
    /// Command arguments violate a cross-field contract.
    #[error("{0}")]
    Argument(String),
    /// A configuration could not be loaded or persisted.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A set or unset operation failed.
    #[error(transparent)]
    Edit(#[from] EditError),
    /// Standard input could not be read.
    #[error("cannot read standard input: {0}")]
    Stdin(io::Error),
    /// The application runtime could not execute or serialize a command.
    #[error("{0}")]
    Runtime(String),
}

impl AppError {
    /// Returns the CLI exit status for this error.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Argument(_) => 2,
            Self::Edit(EditError::Argument(_)) => 2,
            Self::Config(_) | Self::Edit(EditError::Config(_)) | Self::Stdin(_) => 3,
            Self::Runtime(_) => 4,
        }
    }

    /// Returns the stable human-readable error category.
    pub fn category(&self) -> &'static str {
        match self.exit_code() {
            2 => "argument_error",
            3 => "config_error",
            _ => "runtime_error",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Cursor;

    use super::read_secret;

    #[test]
    fn read_secret_uses_hidden_reader_for_a_terminal() {
        let hidden_reader_called = Cell::new(false);
        let mut piped = Cursor::new("piped-secret\n");

        let value = read_secret(&mut piped, "secret: ", true, || {
            hidden_reader_called.set(true);
            Ok("terminal-secret".to_owned())
        })
        .expect("read terminal secret");

        assert_eq!(
            (value, hidden_reader_called.get(), piped.position()),
            ("terminal-secret".to_owned(), true, 0)
        );
    }

    #[test]
    fn read_secret_uses_bufread_for_a_pipe() {
        let hidden_reader_called = Cell::new(false);
        let mut piped = Cursor::new("piped-secret\n");

        let value = read_secret(&mut piped, "secret: ", false, || {
            hidden_reader_called.set(true);
            Ok("terminal-secret".to_owned())
        })
        .expect("read piped secret");

        assert_eq!(
            (value, hidden_reader_called.get()),
            ("piped-secret".to_owned(), false)
        );
    }
}
