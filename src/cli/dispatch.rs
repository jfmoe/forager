use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use thiserror::Error;

use super::args::{
    AnysearchCommand, Cli, Command, ConfigCommand, Context7Command, DocsOutputFormat, ExaCommand,
    Language, OutputFormat,
};
use crate::config::{self, ConfigError, ConfigLocation, EditError};
use crate::net::{self, RetryPolicy};
use crate::providers::{
    self, AnysearchDomainsRequest, AnysearchSearchRequest, Context7DocsRequest,
    Context7LibraryRequest, ExaSearchRequest, ExaSimilarRequest, FetchRequest, MapRequest,
};
use crate::types::{
    AnysearchOutcome, CapabilitySet, ClaimRisk, Context7Outcome, Deadline, EvidenceStrength,
    FallbackPolicy, FetchOutcome, JournalOutcome, MapOutcome, PlanCapability, ProviderAttempt,
    RecencyRequirement, ResearchIntentSignals, ResearchPlan, ResearchSubquestion, SearchOutcome,
};

#[doc(hidden)]
pub use crate::attempt_trace::bounded_attempt_summary;
#[doc(hidden)]
pub use crate::net::combine_diagnostics;

pub use crate::providers::ProviderError;
pub use crate::research::{ResearchFailure, ResearchTerminal};
pub use crate::types::ExaOutcome;

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
    /// A Default Search Invocation that failed after journal configuration was available.
    SearchPreflight {
        /// Stable preflight error.
        error: AppError,
        /// Search Result Journal side-channel outcome.
        journal: JournalOutcome,
        /// Requested output format.
        format: DocsOutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
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
        /// Optional terminal projection selected by `log.level`.
        attempt_log: Option<String>,
    },
    /// One completed caller-planned research invocation.
    Research {
        /// Research terminal state, success or failure.
        terminal: ResearchTerminal,
        /// Search Result Journal side-channel outcome.
        journal: JournalOutcome,
        /// Requested output format.
        format: DocsOutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
        /// Whether full provider attempts should be rendered inline.
        verbose: bool,
        /// Optional terminal projection selected by `log.level`.
        attempt_log: Option<String>,
    },
    /// Typed Exa terminal state for binary-side formatting and tee output.
    Exa {
        /// Provider result.
        result: Result<ExaOutcome, ProviderError>,
        /// Requested output format.
        format: OutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
        /// Optional terminal projection selected by `log.level`.
        attempt_log: Option<String>,
    },
    /// Typed Context7 terminal state for binary-side formatting and tee output.
    Context7 {
        /// Provider result.
        result: Result<Context7Outcome, ProviderError>,
        /// Requested output format.
        format: DocsOutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
        /// Optional terminal projection selected by `log.level`.
        attempt_log: Option<String>,
    },
    /// Typed `AnySearch` terminal state for binary-side formatting and tee output.
    Anysearch {
        /// Provider result.
        result: Result<AnysearchOutcome, ProviderError>,
        /// Requested output format.
        format: OutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
        /// Optional terminal projection selected by `log.level`.
        attempt_log: Option<String>,
    },
    /// Typed Web Fetch terminal state for binary-side formatting and tee output.
    Fetch {
        /// Fetch result.
        result: Result<FetchOutcome, ProviderError>,
        /// Requested output format.
        format: DocsOutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
        /// Optional terminal projection selected by `log.level`.
        attempt_log: Option<String>,
    },
    /// Typed Tavily site map terminal state for binary-side formatting and tee output.
    Map {
        /// Site map result.
        result: Result<MapOutcome, ProviderError>,
        /// Requested output format.
        format: OutputFormat,
        /// Optional tee destination.
        output: Option<PathBuf>,
        /// Optional terminal projection selected by `log.level`.
        attempt_log: Option<String>,
    },
}

struct AppContext<P> {
    provider: P,
    runtime: tokio::runtime::Runtime,
    log_level: config::LogLevel,
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
    log_level: config::LogLevel,
}

struct SearchContext {
    config: config::RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    model_breakers: std::sync::Arc<providers::ModelBreakers>,
    journal: config::JournalRuntimeConfig,
    timeout: Duration,
    runtime: tokio::runtime::Runtime,
}

struct ResearchContext {
    config: config::RuntimeConfig,
    client: reqwest::Client,
    retry_policy: RetryPolicy,
    journal: config::JournalRuntimeConfig,
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
            deadline: Deadline::new(Duration::from_secs(timeout.unwrap_or(180))),
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
            model_breakers: std::sync::Arc::new(providers::ModelBreakers::default()),
            journal,
            timeout,
            runtime: dependencies.runtime,
        })
    }

    // Search orchestration is kept together so attempt and journal ordering stay auditable.
    #[expect(clippy::too_many_lines)]
    fn search(
        self,
        mut request: providers::MainSearchRequest,
        capabilities: Option<CapabilitySet>,
        extra_sources: u16,
        fallback_override: Option<FallbackPolicy>,
    ) -> (
        Result<SearchOutcome, ProviderError>,
        JournalOutcome,
        Option<String>,
    ) {
        let fallback = fallback_override.unwrap_or(self.config.main_search.fallback);
        request.allow_model_fallback = fallback.allows_fallback();
        let query = request.query.clone();
        let started = Instant::now();
        let deadline = Deadline::new(self.timeout);
        let (
            capabilities,
            decision_source,
            classifier_degraded,
            classifier_duration,
            classifier_attempts,
            classifier_warning,
        ) = match capabilities {
            Some(capabilities) => (capabilities, "caller", false, None, Vec::new(), None),
            None if !self.config.classifier.configured() => (
                CapabilitySet::default_supplemental_web_search(),
                "default_web_search",
                false,
                None,
                Vec::new(),
                None,
            ),
            None => {
                let classifier = crate::classifier::Classifier::new(
                    self.config.classifier.clone(),
                    self.client.clone(),
                    self.retry_policy,
                );
                match self.runtime.block_on(classifier.classify(&query, deadline)) {
                    Ok(decision) => (
                        decision.capabilities,
                        "classifier",
                        false,
                        Some(decision.duration),
                        decision.attempts,
                        None,
                    ),
                    Err(failure) => (
                        CapabilitySet::default_supplemental_web_search(),
                        "classifier_degraded",
                        true,
                        Some(failure.duration),
                        failure.attempts,
                        Some(format!(
                            "Classifier warning: {}; using default web_search capability",
                            failure.message
                        )),
                    ),
                }
            }
        };
        let journal_capabilities = capabilities.iter().collect::<Vec<_>>();
        let mut result = self.runtime.block_on(crate::engine::search(
            request,
            self.config.main_search.clone(),
            fallback,
            self.client.clone(),
            self.retry_policy,
            deadline,
            self.model_breakers,
        ));
        let (attempts, diagnostic) = match &mut result {
            Ok(outcome) => (&mut outcome.attempts, &mut outcome.diagnostic),
            Err(error) => (&mut error.attempts, &mut error.diagnostic),
        };
        prepend_classifier_context(
            attempts,
            diagnostic,
            classifier_attempts,
            classifier_warning,
        );
        if let Ok(outcome) = &mut result {
            outcome.capabilities = capabilities.iter().collect();
            self.runtime.block_on(crate::engine::execute_capabilities(
                outcome,
                &query,
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
        }
        let journal = crate::journal::record_search(
            &self.journal,
            crate::journal::SearchRecord {
                query: &query,
                budget: self.timeout,
                elapsed: started.elapsed(),
                capabilities: &journal_capabilities,
                decision_source,
                classifier_degraded,
                classifier_duration,
                result: &result,
            },
        );
        let attempt_log =
            provider_attempt_log(self.config.log_level, &result, |outcome| &outcome.attempts);
        (result, journal, attempt_log)
    }
}

impl ResearchContext {
    fn load(timeout_seconds: u64) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let journal = dependencies.config.journal.clone();
        Ok(Self {
            config: dependencies.config,
            client: dependencies.client,
            retry_policy: dependencies.retry_policy,
            journal,
            timeout: Duration::from_secs(timeout_seconds),
            runtime: dependencies.runtime,
        })
    }

    fn research(
        self,
        query: &str,
        caller_plan: Option<ResearchPlan>,
        budget: crate::research::ResearchBudget,
        evidence_dir: PathBuf,
        fallback: FallbackPolicy,
    ) -> (ResearchTerminal, JournalOutcome, Option<String>) {
        let started = Instant::now();
        let deadline = Deadline::new(self.timeout);
        let (
            plan,
            plan_source,
            classifier_degraded,
            classifier_duration,
            classifier_attempts,
            classifier_warning,
        ) = if let Some(plan) = caller_plan {
            (plan, "caller", false, None, Vec::new(), None)
        } else {
            let classifier = crate::classifier::Classifier::new(
                self.config.classifier.clone(),
                self.client.clone(),
                self.retry_policy,
            );
            match self.runtime.block_on(classifier.plan_research(
                query,
                budget.max_subquestions(),
                deadline,
            )) {
                Ok(mut decision) => {
                    let original_len = decision
                        .plan
                        .truncate_decomposition(budget.max_subquestions());
                    let warning = (original_len > budget.max_subquestions()).then(|| {
                        format!(
                            "Classifier warning: classifier returned {original_len} subquestions; truncated to {} limit {}",
                            budget.as_str(),
                            budget.max_subquestions()
                        )
                    });
                    (
                        decision.plan,
                        "classifier",
                        false,
                        Some(decision.duration),
                        decision.attempts,
                        warning,
                    )
                }
                Err(failure) => (
                    minimal_research_fallback_plan(query),
                    "classifier_degraded",
                    true,
                    Some(failure.duration),
                    failure.attempts,
                    Some(format!(
                        "Classifier warning: {}; using fixed minimal web_search research plan",
                        failure.message
                    )),
                ),
            }
        };
        let capabilities = plan.capabilities().iter().collect::<Vec<_>>();
        let log_level = self.config.log_level;
        let terminal = self.runtime.block_on(crate::research::execute(
            crate::research::ResearchRequest {
                query: query.to_owned(),
                plan,
                plan_source,
                budget,
                evidence_dir,
                fallback,
                initial_attempts: classifier_attempts,
                initial_diagnostic: classifier_warning,
            },
            self.config,
            self.client,
            self.retry_policy,
            deadline,
        ));
        let journal = crate::journal::record_research(
            &self.journal,
            crate::journal::ResearchRecord {
                query,
                budget: self.timeout,
                elapsed: started.elapsed(),
                capabilities: &capabilities,
                plan_source,
                classifier_degraded,
                classifier_duration,
                terminal: &terminal,
            },
        );
        let attempt_log = crate::attempt_log::render(log_level, &terminal.attempts);
        (terminal, journal, attempt_log)
    }
}

impl AppContext<providers::Exa> {
    fn for_exa(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        let log_level = config.log_level;
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

impl AppContext<providers::Context7> {
    fn for_context7(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        let log_level = config.log_level;
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
            log_level,
        })
    }

    fn library(
        self,
        request: Context7LibraryRequest,
    ) -> (Result<Context7Outcome, ProviderError>, Option<String>) {
        let result = self.runtime.block_on(self.provider.library(request));
        let attempt_log = provider_attempt_log(self.log_level, &result, |outcome| match outcome {
            Context7Outcome::Library(outcome) => &outcome.attempts,
            Context7Outcome::Docs(outcome) => &outcome.attempts,
        });
        (result, attempt_log)
    }

    fn docs(
        self,
        request: Context7DocsRequest,
    ) -> (Result<Context7Outcome, ProviderError>, Option<String>) {
        let result = self.runtime.block_on(self.provider.docs(request));
        let attempt_log = provider_attempt_log(self.log_level, &result, |outcome| match outcome {
            Context7Outcome::Library(outcome) => &outcome.attempts,
            Context7Outcome::Docs(outcome) => &outcome.attempts,
        });
        (result, attempt_log)
    }
}

impl AppContext<providers::Anysearch> {
    fn for_anysearch(timeout: Option<u64>) -> Result<Self, AppError> {
        let dependencies = NetworkDependencies::load()?;
        let config = dependencies.config;
        let log_level = config.log_level;
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
            log_level,
        })
    }

    fn domains(
        self,
        request: AnysearchDomainsRequest,
    ) -> (Result<AnysearchOutcome, ProviderError>, Option<String>) {
        let result = self.runtime.block_on(self.provider.domains(request));
        let attempt_log = provider_attempt_log(self.log_level, &result, |outcome| match outcome {
            AnysearchOutcome::Domains(outcome) => &outcome.attempts,
            AnysearchOutcome::Search(outcome) => &outcome.attempts,
        });
        (result, attempt_log)
    }

    fn search(
        self,
        request: AnysearchSearchRequest,
    ) -> (Result<AnysearchOutcome, ProviderError>, Option<String>) {
        let result = self.runtime.block_on(self.provider.search(request));
        let attempt_log = provider_attempt_log(self.log_level, &result, |outcome| match outcome {
            AnysearchOutcome::Domains(outcome) => &outcome.attempts,
            AnysearchOutcome::Search(outcome) => &outcome.attempts,
        });
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
                    output,
                });
            }
            let (result, journal, attempt_log) = context.search(
                providers::MainSearchRequest {
                    query,
                    model,
                    allow_model_fallback: true,
                    verbose,
                },
                capabilities,
                extra_sources,
                fallback,
            );
            Ok(CommandOutput::Search {
                result,
                journal,
                format,
                output,
                verbose,
                attempt_log,
            })
        }
        Command::Research {
            query,
            plan,
            budget,
            evidence_dir,
            fallback,
            timeout,
            format,
            output,
            verbose,
        } => {
            let budget = crate::research::ResearchBudget::from(budget);
            let plan = if let Some(plan) = plan {
                let plan_input = if plan == "-" {
                    read_stdin()?
                } else {
                    fs::read_to_string(&plan).map_err(|error| {
                        AppError::Argument(format!("cannot read research plan `{plan}`: {error}"))
                    })?
                };
                let plan = ResearchPlan::parse_json(&plan_input).map_err(AppError::Argument)?;
                if plan.decomposition().len() > budget.max_subquestions() {
                    return Err(AppError::Argument(format!(
                        "caller research plan has {} subquestions; {} budget allows at most {}",
                        plan.decomposition().len(),
                        budget.as_str(),
                        budget.max_subquestions()
                    )));
                }
                Some(plan)
            } else {
                None
            };
            let evidence_dir = evidence_dir.unwrap_or_else(default_evidence_dir);
            let context = ResearchContext::load(timeout)?;
            if plan.is_none() && !context.config.classifier.configured() {
                return Err(AppError::Config(ConfigError::Message(
                    "research without --plan requires a configured classifier plan generator"
                        .into(),
                )));
            }
            let (terminal, journal, attempt_log) =
                context.research(&query, plan, budget, evidence_dir, fallback);
            Ok(CommandOutput::Research {
                terminal,
                journal,
                format,
                output,
                verbose,
                attempt_log,
            })
        }
        Command::Fetch {
            url,
            timeout,
            format,
            output,
            verbose,
        } => {
            let (result, attempt_log) =
                FetchContext::load(timeout)?.fetch(FetchRequest { url, verbose });
            Ok(CommandOutput::Fetch {
                result,
                format,
                output,
                attempt_log,
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
                    verbose,
                });
            Ok(CommandOutput::Map {
                result,
                format,
                output,
                attempt_log,
            })
        }
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
            if domain.contains('.') {
                return Err(AppError::Argument(
                    "DOMAIN must be a parent domain without a dotted sub-domain".into(),
                ));
            }
            let (result, attempt_log) = AppContext::<providers::Anysearch>::for_anysearch(timeout)?
                .domains(AnysearchDomainsRequest { domain, verbose });
            Ok(CommandOutput::Anysearch {
                result,
                format,
                output,
                attempt_log,
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
            let (result, attempt_log) = AppContext::<providers::Anysearch>::for_anysearch(timeout)?
                .search(AnysearchSearchRequest {
                    query,
                    domain,
                    sub_domain,
                    sub_domain_params,
                    max_results,
                    verbose,
                });
            Ok(CommandOutput::Anysearch {
                result,
                format,
                output,
                attempt_log,
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
            let (result, attempt_log) = AppContext::<providers::Context7>::for_context7(timeout)?
                .library(Context7LibraryRequest {
                    name,
                    query,
                    verbose,
                });
            Ok(CommandOutput::Context7 {
                result,
                format: match format {
                    OutputFormat::Json => DocsOutputFormat::Json,
                    OutputFormat::Markdown => DocsOutputFormat::Markdown,
                },
                output,
                attempt_log,
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
            let (result, attempt_log) = AppContext::<providers::Context7>::for_context7(timeout)?
                .docs(Context7DocsRequest {
                    library_id,
                    query,
                    verbose,
                });
            Ok(CommandOutput::Context7 {
                result,
                format,
                output,
                attempt_log,
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
        Command::Doctor {
            provider: None,
            timeout,
            format,
        } => {
            let (report, exit_code) = crate::doctor::shallow(timeout)?;
            let stdout = match format {
                OutputFormat::Json => serde_json::to_string(&report)
                    .map_err(|error| AppError::Runtime(error.to_string()))?,
                OutputFormat::Markdown => render_doctor_markdown(&report)?,
            };
            Ok(CommandOutput::Text {
                stdout,
                stderr: None,
                exit_code,
            })
        }
        Command::Doctor {
            provider: Some(provider),
            timeout,
            format,
        } => {
            let (report, exit_code) = crate::doctor::deep(provider, timeout)?;
            let stdout = match format {
                OutputFormat::Json => serde_json::to_string(&report)
                    .map_err(|error| AppError::Runtime(error.to_string()))?,
                OutputFormat::Markdown => render_deep_doctor_markdown(&report)?,
            };
            Ok(CommandOutput::Text {
                stdout,
                stderr: None,
                exit_code,
            })
        }
        Command::Smoke {
            probe: Some(case_id),
            probe_timeout,
            probe_url,
            ..
        } => run_smoke_probe(&case_id, probe_timeout, probe_url.as_deref()),
        Command::Smoke {
            live: false,
            probe: None,
            ..
        } => {
            let (report, exit_code) = crate::smoke::run_offline()?;
            Ok(CommandOutput::Text {
                stdout: serde_json::to_string(&report)
                    .map_err(|error| AppError::Runtime(error.to_string()))?,
                stderr: None,
                exit_code,
            })
        }
        Command::Smoke {
            live: true,
            list: true,
            probe: None,
            ..
        } => Ok(CommandOutput::Text {
            stdout: serde_json::to_string(&crate::smoke::live_registry())
                .map_err(|error| AppError::Runtime(error.to_string()))?,
            stderr: None,
            exit_code: 0,
        }),
        Command::Smoke {
            live: true,
            list: false,
            timeout,
            outage_evidence,
            probe: None,
            ..
        } => {
            let outage_evidence = crate::smoke::parse_outage_evidence(&outage_evidence)
                .map_err(AppError::Argument)?;
            let (report, exit_code) = crate::smoke::run_live(timeout, &outage_evidence)?;
            Ok(CommandOutput::Text {
                stdout: serde_json::to_string(&report)
                    .map_err(|error| AppError::Runtime(error.to_string()))?,
                stderr: None,
                exit_code,
            })
        }
    }
}

fn run_smoke_probe(
    case_id: &str,
    timeout_seconds: u64,
    url: Option<&str>,
) -> Result<CommandOutput, AppError> {
    match crate::smoke::probe_kind(case_id).map_err(AppError::Argument)? {
        crate::smoke::ProbeKind::Classifier => run_classifier_smoke_probe(timeout_seconds),
        crate::smoke::ProbeKind::Supplemental(provider) => {
            run_supplemental_smoke_probe(provider.name(), timeout_seconds)
        }
        crate::smoke::ProbeKind::Outage => run_outage_smoke_probe(
            url.ok_or_else(|| AppError::Argument("outage probe requires --probe-url".into()))?,
            timeout_seconds,
        ),
    }
}

fn run_outage_smoke_probe(url: &str, timeout_seconds: u64) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let status_page = crate::smoke::is_official_status_url(url);
    let result = dependencies.runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(timeout_seconds), async {
            let response = dependencies
                .client
                .get(url)
                .send()
                .await
                .map_err(net::CappedStreamError::Transport)?;
            let status = response.status();
            let server_error = status.is_server_error();
            let body = if status_page {
                net::read_response_body(
                    response,
                    net::ResponseBodyPolicy::for_status(
                        status,
                        net::ResponseBodyPolicy::CompleteProtocol,
                    ),
                )
                .await?
                .text
            } else {
                String::new()
            };
            Ok::<_, net::CappedStreamError<reqwest::Error>>((server_error, body))
        })
        .await
    });
    let outage = if status_page {
        result
            .ok()
            .and_then(Result::ok)
            .is_some_and(|(_, body)| crate::smoke::status_page_reports_outage(&body))
    } else {
        match result {
            Ok(Ok((server_error, _))) => server_error,
            Ok(Err(_)) | Err(_) => true,
        }
    };
    Ok(CommandOutput::Text {
        stdout: serde_json::json!({"outage": outage}).to_string(),
        stderr: None,
        exit_code: if outage { 4 } else { 0 },
    })
}

fn run_classifier_smoke_probe(timeout_seconds: u64) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    if !dependencies.config.classifier.configured() {
        return Err(AppError::Config(ConfigError::Message(
            "classifier.keys has no configured credentials".into(),
        )));
    }
    let classifier = crate::classifier::Classifier::new(
        dependencies.config.classifier,
        dependencies.client,
        RetryPolicy::new(1, 1.0, Duration::ZERO),
    );
    let timeout = Duration::from_secs(timeout_seconds);
    let capabilities = dependencies
        .runtime
        .block_on(classifier.classify(crate::smoke::PIPELINE_CANARY_QUERY, Deadline::new(timeout)));
    let plan = dependencies.runtime.block_on(classifier.plan_research(
        crate::smoke::RESEARCH_CANARY_QUERY,
        crate::research::ResearchBudget::Standard.max_subquestions(),
        Deadline::new(timeout),
    ));
    let (Ok(capabilities), Ok(plan)) = (capabilities, plan) else {
        return Ok(CommandOutput::Text {
            stdout: serde_json::json!({
                "error_kind": "runtime",
                "message": "classifier live contract probe failed"
            })
            .to_string(),
            stderr: None,
            exit_code: 4,
        });
    };
    let mut attempts = capabilities.attempts;
    attempts.extend(plan.attempts);
    Ok(CommandOutput::Text {
        stdout: serde_json::json!({
            "capabilities": capabilities.capabilities.iter().collect::<Vec<_>>(),
            "research_plan": plan.plan,
            "provider_attempts": attempts,
        })
        .to_string(),
        stderr: None,
        exit_code: 0,
    })
}

fn run_supplemental_smoke_probe(
    provider: &str,
    timeout_seconds: u64,
) -> Result<CommandOutput, AppError> {
    let dependencies = NetworkDependencies::load()?;
    let mut config = dependencies.config.web_search;
    let provider = crate::providers::ProviderId::parse(provider)
        .ok_or_else(|| AppError::Argument(format!("unknown provider `{provider}`")))?;
    config.retain(provider);
    let outcome = dependencies
        .runtime
        .block_on(crate::engine::supplemental_web_search(
            crate::smoke::MAIN_CANARY_QUERY,
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
                "message": "supplemental live contract probe failed"
            })
            .to_string(),
            stderr: None,
            exit_code: 4,
        }),
    }
}

fn render_doctor_markdown(report: &crate::doctor::ShallowDoctorReport) -> Result<String, AppError> {
    let value =
        serde_json::to_value(report).map_err(|error| AppError::Runtime(error.to_string()))?;
    let mut output = format!(
        "# forager doctor\n\nok: {}\n\n",
        value["ok"].as_bool().unwrap_or(false)
    );
    for provider in value["providers"]
        .as_array()
        .expect("doctor providers serialize as an array")
    {
        let _ = writeln!(
            output,
            "- {}: configured={}, key_count={}, source={}, reachable={}",
            provider["provider"].as_str().unwrap_or_default(),
            provider["configured"].as_bool().unwrap_or(false),
            provider["key_count"].as_u64().unwrap_or(0),
            provider["source"].as_str().unwrap_or_default(),
            provider["reachable"].as_bool().unwrap_or(false),
        );
    }
    if let Some(warnings) = value["permission_warnings"].as_array()
        && !warnings.is_empty()
    {
        output.push_str("\n## Permission warnings\n\n");
        for warning in warnings {
            let _ = writeln!(output, "- {}", warning.as_str().unwrap_or_default());
        }
    }
    output.push_str("\n## Effective configuration\n\n```json\n");
    output.push_str(
        &serde_json::to_string_pretty(&value["config"])
            .map_err(|error| AppError::Runtime(error.to_string()))?,
    );
    output.push_str("\n```\n");
    Ok(output)
}

fn render_deep_doctor_markdown(
    report: &crate::doctor::DeepDoctorReport,
) -> Result<String, AppError> {
    let value =
        serde_json::to_value(report).map_err(|error| AppError::Runtime(error.to_string()))?;
    let mut output = format!(
        "# forager doctor: {}\n\nStatus: {}\n\n- configured: {}\n- key_count: {}\n- source: {}\n- deadline_seconds: {}\n",
        value["provider"].as_str().unwrap_or_default(),
        if value["ok"].as_bool().unwrap_or(false) {
            "ok"
        } else {
            "failed"
        },
        value["configured"].as_bool().unwrap_or(false),
        value["key_count"].as_u64().unwrap_or(0),
        value["source"].as_str().unwrap_or_default(),
        value["deadline_seconds"].as_u64().unwrap_or(0),
    );
    for check in value["checks"]
        .as_array()
        .expect("doctor checks serialize as an array")
    {
        let _ = writeln!(
            output,
            "- {} ({}): {}",
            check["name"].as_str().unwrap_or_default(),
            check["transport"].as_str().unwrap_or_default(),
            if check["ok"].as_bool().unwrap_or(false) {
                "ok"
            } else {
                "failed"
            },
        );
    }
    if let Some(message) = value["message"].as_str() {
        let _ = write!(
            output,
            "\n{}: {message}\n",
            value["error_kind"].as_str().unwrap_or("runtime")
        );
    }
    Ok(output)
}

fn minimal_research_fallback_plan(query: &str) -> ResearchPlan {
    ResearchPlan::new(
        1,
        ResearchIntentSignals {
            recency_requirement: RecencyRequirement::None,
            docs_api_intent: false,
            source_authority_need: EvidenceStrength::Normal,
            claim_risk: ClaimRisk::Medium,
            cross_validation_need: EvidenceStrength::Normal,
        },
        vec![ResearchSubquestion {
            id: "sq1".into(),
            question: query.into(),
            reason: "Gather minimum available web evidence".into(),
            required_capabilities: vec![PlanCapability::WebSearch],
        }],
    )
    .expect("the fixed fallback research plan is valid")
}

fn default_evidence_dir() -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir()
        .join("forager-evidence")
        .join(format!("{}-{timestamp}", std::process::id()))
}

fn prepend_classifier_context(
    attempts: &mut Vec<ProviderAttempt>,
    diagnostic: &mut Option<String>,
    classifier_attempts: Vec<ProviderAttempt>,
    classifier_warning: Option<String>,
) {
    attempts.splice(0..0, classifier_attempts);
    *diagnostic = combine_diagnostics(
        [classifier_warning, diagnostic.take()]
            .into_iter()
            .flatten(),
    );
}

fn provider_attempt_log<'a, T>(
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
    output: Option<PathBuf>,
) -> Result<CommandOutput, AppError> {
    let (result, attempt_log) = AppContext::<providers::Exa>::for_exa(timeout)?.exa_search(request);
    Ok(CommandOutput::Exa {
        result,
        format,
        output,
        attempt_log,
    })
}

fn run_exa_similar(
    request: ExaSimilarRequest,
    timeout: Option<u64>,
    format: OutputFormat,
    output: Option<PathBuf>,
) -> Result<CommandOutput, AppError> {
    let (result, attempt_log) =
        AppContext::<providers::Exa>::for_exa(timeout)?.exa_similar(request);
    Ok(CommandOutput::Exa {
        result,
        format,
        output,
        attempt_log,
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
            2 => "parameter",
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

    use super::{AppError, ConfigError, read_secret};

    #[test]
    fn app_errors_have_stable_json_preflight_fields() {
        let cases = [
            (
                AppError::Argument("invalid plan".into()),
                "parameter",
                2,
                "invalid plan",
            ),
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
