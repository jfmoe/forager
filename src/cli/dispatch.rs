use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use thiserror::Error;

use super::args::{
    Cli, Command, ConfigCommand, ContentOutputFormat, ExaCommand, Language, OutputFormat,
    OutputTarget,
};
use super::doctor;
use crate::config::{self, ConfigError, ConfigLocation, EditError};
use crate::net::{self, RetryPolicy};
use crate::providers::{
    self, ExaSearchRequest, ExaSimilarRequest, FetchRequest, MapRequest, ProviderId,
};
use crate::types::{
    AttemptErrorKind, CapabilitySet, Deadline, FallbackPolicy, FetchOutcome, HnStoriesResult,
    JournalOutcome, MapOutcome, Platform, PlatformAskResult, PlatformCommentsPage,
    PlatformFetchResult, PlatformSearchPage, PlatformTranscribeResult, PolymarketHistoryResult,
    ProviderAttempt, SearchOutcome,
};

#[doc(hidden)]
pub use crate::attempt_trace::bounded_attempt_summary;
#[doc(hidden)]
pub use crate::net::combine_diagnostics;

pub use crate::search_fanout::SearchFailure;
pub use crate::types::ExaOutcome;
pub use crate::types::ProviderError;

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
    /// A command that failed after its preflight, without attempts or a journal: the binary
    /// renders the thin failure payload `{error_kind, message}` with the exit code of the
    /// error's family.
    Failure {
        /// What kind of failure ended the command.
        kind: AttemptErrorKind,
        /// The failure message.
        message: String,
    },
    /// A Default Search Invocation that failed after journal configuration was available.
    SearchPreflight {
        /// Stable preflight error.
        error: AppError,
        /// Search Result Journal side-channel outcome.
        journal: JournalOutcome,
        /// Requested output format.
        format: ContentOutputFormat,
        /// Optional output file destination.
        output: Option<OutputTarget>,
    },
    /// One completed Default Search Invocation.
    Search {
        /// Main-search terminal result.
        result: Result<SearchOutcome, SearchFailure>,
        /// Search Result Journal side-channel outcome.
        journal: JournalOutcome,
        /// How the result is presented.
        presentation: Presentation<ContentOutputFormat>,
    },
    /// Typed Exa terminal state for binary-side formatting and tee output.
    Exa {
        /// Provider result.
        result: Result<ExaOutcome, ProviderError>,
        /// How the result is presented.
        presentation: Presentation<OutputFormat>,
    },
    /// Typed Web Fetch terminal state for binary-side formatting and tee output.
    Fetch {
        /// Fetch result.
        result: Result<FetchOutcome, ProviderError>,
        /// The host of the requested URL, without its userinfo or port.
        host: Option<String>,
        /// How the result is presented.
        presentation: Presentation<ContentOutputFormat>,
    },
    /// Typed platform search terminal state for binary-side formatting and tee output.
    PlatformSearch {
        /// Platform search result.
        result: Result<PlatformSearchPage, ProviderError>,
        /// How the result is presented.
        presentation: Presentation<OutputFormat>,
    },
    /// Typed platform fetch terminal state for binary-side formatting and tee output.
    PlatformFetch {
        /// Platform fetch result; a full-text body is already written unless the format is
        /// `content`.
        result: Box<Result<PlatformFetchResult, ProviderError>>,
        /// How the result is presented.
        presentation: Presentation<ContentOutputFormat>,
    },
    /// Typed platform comments terminal state for binary-side formatting and tee output.
    PlatformComments {
        /// Comments result, in the platform's own shape.
        result: Result<PlatformCommentsPage, ProviderError>,
        /// How the result is presented.
        presentation: Presentation<OutputFormat>,
    },
    /// Typed platform ask terminal state for binary-side formatting and tee output.
    PlatformAsk {
        /// The platform asked.
        platform: Platform,
        /// Ask result, in the platform's own shape.
        result: Result<PlatformAskResult, ProviderError>,
        /// How the result is presented.
        presentation: Presentation<OutputFormat>,
    },
    /// Typed transcribe terminal state for binary-side formatting and tee output.
    PlatformTranscribe {
        /// Transcribe result; the transcript is already written unless the format is
        /// `content`.
        result: Box<Result<PlatformTranscribeResult, ProviderError>>,
        /// How the result is presented.
        presentation: Presentation<ContentOutputFormat>,
    },
    /// Typed Hacker News stories terminal state for binary-side formatting and tee output.
    HnStories {
        /// Stories result.
        result: Result<HnStoriesResult, ProviderError>,
        /// How the result is presented.
        presentation: Presentation<OutputFormat>,
    },
    /// Typed Polymarket price history terminal state for binary-side formatting and tee output.
    PolymarketHistory {
        /// History result.
        result: Result<PolymarketHistoryResult, ProviderError>,
        /// How the result is presented.
        presentation: Presentation<OutputFormat>,
    },
    /// Typed Tavily site map terminal state for binary-side formatting and tee output.
    Map {
        /// Site map result.
        result: Result<MapOutcome, ProviderError>,
        /// How the result is presented.
        presentation: Presentation<OutputFormat>,
    },
}

/// How a retrieval command presents its result: the format, the `--output` copy, whether the
/// result lists every provider attempt, as `--verbose` asks, and the attempt log that
/// `log.level` selects.
#[derive(Debug)]
pub struct Presentation<F> {
    /// Requested output format.
    pub format: F,
    /// Optional output file destination.
    pub output: Option<OutputTarget>,
    /// Whether the result lists every provider attempt.
    pub verbose: bool,
    /// Optional terminal projection selected by `log.level`.
    pub attempt_log: Option<String>,
}

struct AppContext<P> {
    provider: P,
    runtime: tokio::runtime::Runtime,
    log_level: config::LogLevel,
}

pub(super) struct NetworkDependencies {
    pub(super) config: config::RuntimeConfig,
    pub(super) client: reqwest::Client,
    pub(super) retry_policy: RetryPolicy,
    pub(super) runtime: tokio::runtime::Runtime,
}

struct FetchContext {
    config: config::WebFetchRuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    deadline: Deadline,
    runtime: tokio::runtime::Runtime,
    log_level: config::LogLevel,
}

struct SearchContext {
    config: config::RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    journal: config::JournalRuntimeConfig,
    timeout: Duration,
    runtime: tokio::runtime::Runtime,
}

impl NetworkDependencies {
    pub(super) fn load() -> Result<Self, AppError> {
        let config = config::runtime_config()?;
        let retry_policy = RetryPolicy::new(
            config.retry.max_attempts,
            config.retry.multiplier,
            Duration::from_secs(config.retry.max_wait_seconds),
        );
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|error| AppError::Runtime(format!("cannot start network runtime: {error}")))?;
        let client = runtime
            .block_on(async { net::build_client(config.ssl_verify) })
            .map_err(|error| AppError::Runtime(format!("cannot build HTTP client: {error}")))?;
        Ok(Self {
            config,
            client,
            retry_policy,
            runtime,
        })
    }

    /// The configuration of `platform`, once every browser route in its order passed the
    /// preflight check; a platform command reads it before it plans its route chain.
    pub(super) fn platform(
        &self,
        platform: crate::types::Platform,
    ) -> Result<&config::PlatformRuntimeConfig, AppError> {
        let config = self.config.platforms.get(platform);
        crate::platform_chain::browser_preflight(config, &self.config.browser)
            .map_err(|message| AppError::Config(ConfigError::Message(message)))?;
        Ok(config)
    }
}

/// The command deadline of a `fetch` asked to finish within `timeout` seconds, three minutes
/// when it names none.
pub(super) fn fetch_deadline(timeout: Option<u64>) -> Duration {
    Duration::from_secs(timeout.unwrap_or(180))
}

/// The command deadline of an `exa` command asked to finish within `timeout` seconds, the
/// configured Exa timeout of `configured_seconds` when it names none.
pub(super) fn exa_deadline(timeout: Option<u64>, configured_seconds: u64) -> Duration {
    Duration::from_secs(timeout.unwrap_or(configured_seconds))
}

impl FetchContext {
    fn load(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let log_level = dependencies.config.log_level;
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
            deadline: Deadline::new(fetch_deadline(timeout)),
            runtime: dependencies.runtime,
            log_level,
        })
    }

    fn fetch(self, request: FetchRequest) -> (Result<FetchOutcome, ProviderError>, Option<String>) {
        let result = self.runtime.block_on(crate::engine::fetch(
            request,
            self.config,
            self.client,
            self.retry_policy,
            self.deadline,
        ));
        let attempt_log =
            provider_attempt_log(self.log_level, &result, |outcome| &outcome.attempts);
        (result, attempt_log)
    }
}

impl SearchContext {
    fn load(timeout_seconds: u64) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        let journal = config.journal.clone();
        let timeout = Duration::from_secs(timeout_seconds);
        Ok(Self {
            config,
            client: dependencies.client,
            retry_policy: dependencies.retry_policy,
            journal,
            timeout,
            runtime: dependencies.runtime,
        })
    }

    fn search(
        self,
        request: providers::MainSearchRequest,
        capabilities: Option<CapabilitySet>,
        extra_sources: u16,
        fallback_override: Option<FallbackPolicy>,
    ) -> (
        Result<SearchOutcome, SearchFailure>,
        JournalOutcome,
        Option<String>,
    ) {
        let fallback = fallback_override.unwrap_or(self.config.main_search.fallback);
        let query = request.query.clone();
        let started = Instant::now();
        let deadline = Deadline::new(self.timeout);
        let (capabilities, plan_source) = search_plan(capabilities);
        let journal_capabilities = capabilities.iter().collect::<Vec<_>>();
        let result = self.runtime.block_on(crate::search_fanout::search(
            request,
            &capabilities,
            extra_sources,
            &self.config,
            crate::engine::CapabilityExecution::new(
                fallback,
                self.client.clone(),
                self.retry_policy,
                deadline,
            ),
        ));
        let journal = crate::journal::record_search(
            &self.journal,
            crate::journal::SearchRecord {
                query: &query,
                budget: self.timeout,
                elapsed: started.elapsed(),
                capabilities: &journal_capabilities,
                plan_source,
                result: &result,
            },
        );
        let attempts = match &result {
            Ok(outcome) => &outcome.attempts,
            Err(failure) => &failure.error.attempts,
        };
        let attempt_log = crate::attempt_log::render(self.config.log_level, attempts);
        (result, journal, attempt_log)
    }
}

/// The capabilities a search runs and who chose them: a search that declares none runs main
/// search only.
fn search_plan(declared: Option<CapabilitySet>) -> (CapabilitySet, crate::journal::PlanSource) {
    match declared {
        Some(capabilities) => (capabilities, crate::journal::PlanSource::Caller),
        None => (
            CapabilitySet::default(),
            crate::journal::PlanSource::DefaultMainSearchOnly,
        ),
    }
}

impl AppContext<providers::Exa> {
    fn for_exa(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        let log_level = config.log_level;
        if !config.provider_configured(ProviderId::Exa) {
            return Err(AppError::Config(ConfigError::Message(
                "providers.exa.keys has no configured credentials".into(),
            )));
        }
        let deadline = exa_deadline(timeout, config.exa.timeout_seconds);
        let provider = providers::build_exa(
            config.exa,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(deadline),
        );
        Ok(Self {
            provider,
            runtime: dependencies.runtime,
            log_level,
        })
    }

    fn exa_search(
        self,
        request: ExaSearchRequest,
    ) -> (Result<ExaOutcome, ProviderError>, Option<String>) {
        let result = self.runtime.block_on(self.provider.search(request));
        let attempt_log =
            provider_attempt_log(self.log_level, &result, |outcome| &outcome.attempts);
        (result, attempt_log)
    }

    fn exa_similar(
        self,
        request: ExaSimilarRequest,
    ) -> (Result<ExaOutcome, ProviderError>, Option<String>) {
        let result = self.runtime.block_on(self.provider.similar(request));
        let attempt_log =
            provider_attempt_log(self.log_level, &result, |outcome| &outcome.attempts);
        (result, attempt_log)
    }
}

impl AppContext<providers::TavilyMap> {
    fn for_tavily_map(timeout: u64) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let log_level = dependencies.config.log_level;
        if !dependencies.config.provider_configured(ProviderId::Tavily) {
            return Err(AppError::Config(ConfigError::Message(
                "providers.tavily.keys has no configured credentials".into(),
            )));
        }
        let provider = providers::build_tavily_map(
            dependencies.config.tavily,
            dependencies.client,
            dependencies.retry_policy,
            Deadline::new(Duration::from_secs(timeout)),
        );
        Ok(Self {
            provider,
            runtime: dependencies.runtime,
            log_level,
        })
    }

    fn map(self, request: MapRequest) -> (Result<MapOutcome, ProviderError>, Option<String>) {
        let result = self.runtime.block_on(self.provider.map(request));
        let attempt_log =
            provider_attempt_log(self.log_level, &result, |outcome| &outcome.attempts);
        (result, attempt_log)
    }
}

/// Executes a parsed command.
///
/// # Errors
///
/// Returns [`AppError`] when arguments, configuration, standard input, or
/// persistence are invalid.
// Command dispatch stays explicit so each CLI variant remains visible in one exhaustive match.
#[expect(clippy::too_many_lines)]
pub fn run(cli: Cli) -> Result<CommandOutput, AppError> {
    match cli.command {
        Command::Search {
            query,
            capabilities,
            model,
            extra_sources,
            fallback,
            timeout,
            format,
            output,
            verbose,
        } => {
            let context = SearchContext::load(timeout)?;
            if context.config.main_search.configured_provider_count() == 0 {
                let error = AppError::Config(ConfigError::Message(
                    "search.backends has no configured credentials".into(),
                ));
                let message = error.to_string();
                let journal = crate::journal::record_search_preflight(
                    &context.journal,
                    crate::journal::SearchPreflightRecord {
                        query: &query,
                        budget: Duration::from_secs(timeout),
                        error_kind: "config",
                        message: &message,
                    },
                );
                return Ok(CommandOutput::SearchPreflight {
                    error,
                    journal,
                    format,
                    output: output.target(),
                });
            }
            let (result, journal, attempt_log) = context.search(
                providers::MainSearchRequest { query, model },
                capabilities,
                extra_sources,
                fallback,
            );
            Ok(CommandOutput::Search {
                result,
                journal,
                presentation: Presentation {
                    format,
                    output: output.target(),
                    verbose,
                    attempt_log,
                },
            })
        }
        Command::Fetch {
            url,
            timeout,
            format,
            output,
            verbose,
        } => {
            let host = reqwest::Url::parse(&url)
                .ok()
                .and_then(|url| url.host_str().map(str::to_owned));
            let (result, attempt_log) = FetchContext::load(timeout)?.fetch(FetchRequest { url });
            Ok(CommandOutput::Fetch {
                result,
                host,
                presentation: Presentation {
                    format,
                    output: output.target(),
                    verbose,
                    attempt_log,
                },
            })
        }
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
        } => {
            let (result, attempt_log) =
                AppContext::<providers::TavilyMap>::for_tavily_map(timeout)?.map(MapRequest {
                    url,
                    instructions,
                    max_depth,
                    max_breadth,
                    limit,
                    timeout_seconds: timeout,
                });
            Ok(CommandOutput::Map {
                result,
                presentation: Presentation {
                    format,
                    output: output.target(),
                    verbose,
                    attempt_log,
                },
            })
        }
        Command::Exa {
            command:
                ExaCommand::Search {
                    query,
                    num_results,
                    search_type,
                    include_text,
                    text_max_characters,
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
                text_max_characters: include_text.then_some(text_max_characters),
                include_highlights,
                start_published_date,
                include_domains,
                exclude_domains,
                category,
            },
            timeout,
            format,
            output.target(),
            verbose,
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
            ExaSimilarRequest { url, num_results },
            timeout,
            format,
            output.target(),
            verbose,
        ),
        Command::Platform { command } => super::platform::run(command),
        Command::Browser { command } => super::browser::run(command),
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
        Command::Doctor {
            provider,
            offline,
            live,
            tier,
            platform,
            list,
            unattended,
            health,
            days,
            timeout,
            format,
            supplemental_web_search,
            query,
        } => {
            if let Some(provider) = supplemental_web_search {
                return run_supplemental_web_search(
                    provider,
                    &query.unwrap_or_default(),
                    timeout.unwrap_or(doctor::DOCTOR_TIMEOUT_SECONDS),
                );
            }
            if live {
                let selection = crate::live::LiveSelection {
                    tiers: tier,
                    provider,
                    platform,
                    unattended: unattended || doctor::ci_environment(std::env::var_os("CI")),
                };
                return doctor::run_live(
                    &selection,
                    list,
                    timeout.unwrap_or(doctor::LIVE_TIMEOUT_SECONDS),
                    format,
                );
            }
            if health {
                return doctor::run_health(days.unwrap_or(crate::health::DEFAULT_DAYS), format);
            }
            doctor::run_report(
                provider,
                offline,
                timeout.unwrap_or(doctor::DOCTOR_TIMEOUT_SECONDS),
                format,
            )
        }
    }
}

/// Runs one supplemental web search provider alone: the hidden entry behind its Live Check.
fn run_supplemental_web_search(
    provider: ProviderId,
    query: &str,
    timeout_seconds: u64,
) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let mut config = dependencies.config.web_search;
    config.retain(provider);
    let outcome = dependencies
        .runtime
        .block_on(crate::engine::supplemental_web_search(
            query,
            3,
            config,
            crate::engine::CapabilityExecution::new(
                FallbackPolicy::Off,
                dependencies.client,
                RetryPolicy::new(1, 1.0, Duration::ZERO),
                Deadline::new(Duration::from_secs(timeout_seconds)),
            ),
        ));
    match outcome {
        Ok(outcome) => Ok(CommandOutput::Text {
            stdout: serde_json::json!({
                "results": outcome.sources,
                "provider_attempts": outcome.attempts,
            })
            .to_string(),
            stderr: None,
            exit_code: 0,
        }),
        Err(_) => Ok(CommandOutput::Text {
            stdout: serde_json::json!({
                "error_kind": "runtime",
                "message": "the supplemental web search failed"
            })
            .to_string(),
            stderr: None,
            exit_code: 4,
        }),
    }
}

pub(super) fn invocation_temp_dir(name: &str) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir()
        .join(name)
        .join(format!("{}-{timestamp}", std::process::id()))
}

pub(super) fn provider_attempt_log<'a, T>(
    level: config::LogLevel,
    result: &'a Result<T, ProviderError>,
    success_attempts: impl FnOnce(&'a T) -> &'a [ProviderAttempt],
) -> Option<String> {
    let attempts = match result {
        Ok(outcome) => success_attempts(outcome),
        Err(error) => &error.attempts,
    };
    crate::attempt_log::render(level, attempts)
}

fn run_exa_search(
    request: ExaSearchRequest,
    timeout: Option<u64>,
    format: OutputFormat,
    output: Option<OutputTarget>,
    verbose: bool,
) -> Result<CommandOutput, AppError> {
    let (result, attempt_log) = AppContext::<providers::Exa>::for_exa(timeout)?.exa_search(request);
    Ok(CommandOutput::Exa {
        result,
        presentation: Presentation {
            format,
            output,
            verbose,
            attempt_log,
        },
    })
}

fn run_exa_similar(
    request: ExaSimilarRequest,
    timeout: Option<u64>,
    format: OutputFormat,
    output: Option<OutputTarget>,
    verbose: bool,
) -> Result<CommandOutput, AppError> {
    let (result, attempt_log) =
        AppContext::<providers::Exa>::for_exa(timeout)?.exa_similar(request);
    Ok(CommandOutput::Exa {
        result,
        presentation: Presentation {
            format,
            output,
            verbose,
            attempt_log,
        },
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

    for (index, step) in SetupStep::ORDER.into_iter().enumerate() {
        step.announce(language, SetupStep::FIRST_NUMBER + index);
        match step {
            SetupStep::MainModel => {
                configure_model(&mut input, language, &mut document, "xai")?;
            }
            SetupStep::SupplementalProviders => configure_optional_keys(
                &mut input,
                language,
                &mut document,
                &["exa", "jina", "tavily", "firecrawl"],
            )?,
            SetupStep::PlatformRoutes => configure_optional_keys(
                &mut input,
                language,
                &mut document,
                &["serpapi", "youtube_api"],
            )?,
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

/// A step of interactive setup after the language choice, which is step 1.
#[derive(Clone, Copy)]
enum SetupStep {
    MainModel,
    SupplementalProviders,
    PlatformRoutes,
}

impl SetupStep {
    /// The number of the first step here; the language choice is step 1.
    const FIRST_NUMBER: usize = 2;

    /// The steps in the order setup runs them; each one's number follows from its place here.
    const ORDER: [Self; 3] = [
        Self::MainModel,
        Self::SupplementalProviders,
        Self::PlatformRoutes,
    ];

    fn announce(self, language: Language, number: usize) {
        match language {
            Language::Zh => eprintln!("第 {number} 步：{}", self.title(language)),
            Language::En => eprintln!("Step {number}: {}", self.title(language)),
        }
    }

    fn title(self, language: Language) -> &'static str {
        match (self, language) {
            (Self::MainModel, Language::Zh) => "主模型",
            (Self::MainModel, Language::En) => "main model",
            (Self::SupplementalProviders, Language::Zh) => "补强 provider",
            (Self::SupplementalProviders, Language::En) => "supplemental providers",
            (Self::PlatformRoutes, Language::Zh) => "平台 route（Scholar 与 YouTube 需要自备 key）",
            (Self::PlatformRoutes, Language::En) => {
                "platform routes (Scholar and YouTube need your own key)"
            }
        }
    }
}

fn configure_optional_keys(
    input: &mut impl BufRead,
    language: Language,
    document: &mut config::SetupDocument,
    providers: &[&str],
) -> Result<(), AppError> {
    for provider in providers {
        let prompt = match language {
            Language::Zh => format!("{provider} keys（逗号分隔；回车跳过并保留现值）: "),
            Language::En => {
                format!("{provider} keys (comma-separated; Enter skips and preserves): ")
            }
        };
        if let Some(keys) = read_keys(input, &prompt)? {
            document.set_strings(&config::provider_keys_key(provider), &keys)?;
        }
    }
    Ok(())
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
    #[must_use]
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Argument(_) | Self::Edit(EditError::Argument(_)) => 2,
            Self::Config(_) | Self::Edit(EditError::Config(_)) | Self::Stdin(_) => 3,
            Self::Runtime(_) => 4,
        }
    }

    /// Returns the stable human-readable error category.
    #[must_use]
    pub fn category(&self) -> &'static str {
        match self.exit_code() {
            2 => "argument_error",
            3 => "config_error",
            _ => "runtime_error",
        }
    }

    /// Returns the stable machine-readable preflight failure object.
    #[must_use]
    pub fn json_preflight_payload(&self) -> Value {
        let message = self
            .to_string()
            .lines()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(500)
            .collect::<String>();
        let error_kind = match self.exit_code() {
            3 => "config",
            _ => "runtime",
        };
        serde_json::json!({
            "error_kind": error_kind,
            "message": message,
            "journal_ref": Value::Null,
            "journal_status": "unavailable",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io::Cursor;

    use serde_json::{Value, json};

    use super::{AppError, ConfigError, read_secret, search_plan};
    use crate::journal::PlanSource;

    #[test]
    fn a_search_runs_the_declared_capabilities_or_else_main_search_alone() {
        let plans = [None, Some("none"), Some("web_search")].map(|declared| {
            let (capabilities, source) = search_plan(
                declared.map(|declaration| declaration.parse().expect("capability set")),
            );
            (
                capabilities
                    .iter()
                    .map(crate::types::Capability::as_str)
                    .collect::<Vec<_>>(),
                source,
            )
        });

        assert_eq!(
            plans,
            [
                (Vec::new(), PlanSource::DefaultMainSearchOnly),
                (Vec::new(), PlanSource::Caller),
                (vec!["web_search"], PlanSource::Caller),
            ]
        );
    }

    #[test]
    fn app_errors_have_stable_json_preflight_fields() {
        let cases = [
            (
                AppError::Config(ConfigError::Message("invalid config".into())),
                "config",
                3,
                "invalid config",
            ),
            (
                AppError::Runtime("runtime failed".into()),
                "runtime",
                4,
                "runtime failed",
            ),
        ];

        for (error, error_kind, exit_code, message) in cases {
            assert_eq!(
                (error.json_preflight_payload(), error.exit_code()),
                (
                    json!({
                        "error_kind": error_kind,
                        "message": message,
                        "journal_ref": Value::Null,
                        "journal_status": "unavailable",
                    }),
                    exit_code,
                )
            );
        }
    }

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
