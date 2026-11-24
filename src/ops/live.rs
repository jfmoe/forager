//! `doctor --live`: runs the declared Live Checks against the real services, each as a child
//! process of this binary with its own hard time limit.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;
use shared_child::SharedChild;

use crate::catalog::{
    self, LiveArgument, LiveCheck, LiveCommand, LiveContract, LiveInput, LiveTier, MODEL_LIMIT,
    ProviderId, ProviderTransport, RoutePin, StopSignal,
};
use crate::config::{self, RuntimeConfig};
use crate::providers;
use crate::redact::{Secret, redact_credentials};
use crate::types::{Deadline, Platform};

#[path = "live_contract.rs"]
mod contract;

/// The one check outside any provider's registration: a main search with supplemental web
/// search, which proves the pipeline that joins them.
const PIPELINE: LiveCheck = LiveCheck::new(
    "pipeline:search",
    LiveTier::Metered,
    LiveCommand {
        arguments: &[
            LiveArgument::Text("search"),
            LiveArgument::Text("Find current Rust release news and official documentation."),
            LiveArgument::Text("--capabilities"),
            LiveArgument::Text("web_search"),
            LiveArgument::Text("--extra-sources"),
            LiveArgument::Text("2"),
            LiveArgument::Text("--timeout"),
            LiveArgument::Timeout,
        ],
        routes: RoutePin::Configured,
    },
    LiveContract::PipelineSearch,
)
.time_limit(MODEL_LIMIT);

/// How long a killed command gets beyond its own `--timeout` to clean up and print its result.
const KILL_GRACE: Duration = Duration::from_secs(5);

/// A declared check and the provider whose registration declares it; the pipeline check has
/// none.
#[derive(Clone, Copy)]
struct Declared {
    provider: Option<ProviderId>,
    check: LiveCheck,
}

impl Declared {
    fn transport(&self) -> ProviderTransport {
        self.provider.map_or(ProviderTransport::Http, |provider| {
            catalog::registration(provider).transport
        })
    }

    fn stop_signals(&self) -> &'static [StopSignal] {
        match self.transport() {
            ProviderTransport::Browser(environment) => environment.stop_signals,
            ProviderTransport::Http => &[],
        }
    }
}

/// Every declared check, in declaration order: the pipeline check, then each registration's.
fn declared() -> Vec<Declared> {
    std::iter::once(Declared {
        provider: None,
        check: PIPELINE,
    })
    .chain(catalog::registrations().iter().flat_map(|registration| {
        registration.live_checks.iter().map(|check| Declared {
            provider: Some(registration.id),
            check: *check,
        })
    }))
    .collect()
}

/// What a `doctor --live` invocation asks for.
#[derive(Debug, Default)]
pub(crate) struct LiveSelection {
    /// The tiers to run; empty means `free` and `metered`, plus `attended` when a provider is
    /// named.
    pub(crate) tiers: Vec<LiveTier>,
    pub(crate) provider: Option<ProviderId>,
    pub(crate) platform: Option<Platform>,
    /// Set by the unattended option, or by a CI environment.
    pub(crate) unattended: bool,
}

impl LiveSelection {
    fn tiers(&self) -> BTreeSet<LiveTier> {
        if !self.tiers.is_empty() {
            return self.tiers.iter().copied().collect();
        }
        let mut tiers = BTreeSet::from([LiveTier::Free, LiveTier::Metered]);
        // Naming a provider is consent to its attended checks.
        if self.provider.is_some() {
            tiers.insert(LiveTier::Attended);
        }
        tiers
    }
}

/// Why a selection is refused before anything runs.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LiveError {
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Config(#[from] config::ConfigError),
    /// This binary cannot locate itself to start the checks.
    #[error("cannot locate the forager executable: {0}")]
    Executable(std::io::Error),
}

/// Returns, for each declared check, whether the selection runs it. Refuses the attended tier,
/// selected or reached by naming its provider, in an unattended run.
fn select(declared: &[Declared], selection: &LiveSelection) -> Result<Vec<bool>, LiveError> {
    if selection.unattended {
        let named_attended = selection.provider.is_some_and(|provider| {
            declared.iter().any(|entry| {
                entry.provider == Some(provider) && entry.check.tier == LiveTier::Attended
            })
        });
        if selection.tiers.contains(&LiveTier::Attended) || named_attended {
            return Err(LiveError::Refused(
                "an unattended or CI run never runs the attended tier; run it in person without --unattended"
                    .into(),
            ));
        }
    }
    let tiers = selection.tiers();
    let excluded =
        |check: &LiveCheck| selection.unattended && (check.expensive || check.manual_only);
    let mut selected = declared
        .iter()
        .map(|entry| {
            selection
                .provider
                .is_none_or(|provider| entry.provider == Some(provider))
                && selection
                    .platform
                    .is_none_or(|platform| entry.check.platform() == Some(platform))
                && tiers.contains(&entry.check.tier)
                && !excluded(&entry.check)
        })
        .collect::<Vec<_>>();
    // A dependency is declared before its dependents, so one backward pass closes the set.
    for index in (0..declared.len()).rev() {
        if !selected[index] {
            continue;
        }
        if let Some(dependency) = declared[index].check.depends_on
            && let Some(position) = declared
                .iter()
                .position(|entry| entry.check.id == dependency)
            && !excluded(&declared[position].check)
        {
            selected[position] = true;
        }
    }
    Ok(selected)
}

#[derive(Debug, Serialize)]
pub(crate) struct LiveListing {
    mode: &'static str,
    checks: Vec<ListedCheck>,
}

#[derive(Debug, Serialize)]
struct ListedCheck {
    #[serde(flatten)]
    identity: CheckIdentity,
    #[serde(skip_serializing_if = "Option::is_none")]
    depends_on: Option<&'static str>,
    time_limit_seconds: u64,
    expensive: bool,
    manual_only: bool,
    /// Whether this selection runs the check.
    selected: bool,
}

#[derive(Debug, Serialize)]
struct CheckIdentity {
    id: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    provider: Option<&'static str>,
    operation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<Platform>,
    tier: &'static str,
}

impl CheckIdentity {
    fn of(entry: &Declared) -> Self {
        Self {
            id: entry.check.id,
            provider: entry.provider.map(ProviderId::name),
            operation: entry.check.operation(),
            platform: entry.check.platform(),
            tier: entry.check.tier.as_str(),
        }
    }
}

/// Lists every declared check with its tier and flags, and whether `selection` runs it.
pub(crate) fn list(selection: &LiveSelection) -> Result<LiveListing, LiveError> {
    let declared = declared();
    let selected = select(&declared, selection)?;
    Ok(LiveListing {
        mode: "live_list",
        checks: declared
            .iter()
            .zip(selected)
            .map(|(entry, selected)| ListedCheck {
                identity: CheckIdentity::of(entry),
                depends_on: entry.check.depends_on,
                time_limit_seconds: entry.check.time_limit.as_secs(),
                expensive: entry.check.expensive,
                manual_only: entry.check.manual_only,
                selected,
            })
            .collect(),
    })
}

#[derive(Debug, Serialize)]
pub(crate) struct LiveReport {
    mode: &'static str,
    ok: bool,
    unattended: bool,
    tiers: Vec<&'static str>,
    summary: Summary,
    checks: Vec<CheckResult>,
}

#[derive(Debug, Default, PartialEq, Serialize)]
struct Summary {
    passed: usize,
    failed: usize,
    unconfigured: usize,
    not_started: usize,
    unselected: usize,
}

#[derive(Debug, Serialize)]
struct CheckResult {
    #[serde(flatten)]
    identity: CheckIdentity,
    status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<NotStarted>,
    attempts: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Status {
    Passed,
    Failed,
    Unconfigured,
    NotStarted,
}

/// Why a configured, selected check did not start.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum NotStarted {
    /// The run's time limit ended first; this counts as a failure.
    Deadline,
    /// An earlier check on the same site met a stop signal.
    SiteStop,
    /// The check it depends on did not pass or left nothing to read.
    NoInput,
}

/// Runs the selected checks within `timeout_seconds` and reports each one. `selection` already
/// carries whether the run is unattended.
pub(crate) fn run(
    selection: &LiveSelection,
    timeout_seconds: u64,
) -> Result<(LiveReport, u8), LiveError> {
    let declared = declared();
    let selected = select(&declared, selection)?;
    let runtime = config::runtime_config()?;
    let unconfigured = declared
        .iter()
        .map(|entry| unconfigured_reason(entry, &runtime))
        .collect::<Vec<_>>();
    let executable = std::env::current_exe().map_err(LiveError::Executable)?;
    let mut execute = |command: &Prepared, limit: Duration| execute(&executable, command, limit);
    let results = run_checks(
        &declared,
        &selected,
        &unconfigured,
        Deadline::new(Duration::from_secs(timeout_seconds)),
        &runtime.journal.credentials,
        &mut execute,
    );
    let summary = summarize(&results, declared.len());
    let exit_code = exit_code(&results);
    Ok((
        LiveReport {
            mode: "live",
            ok: exit_code == 0,
            unattended: selection.unattended,
            tiers: selection
                .tiers()
                .into_iter()
                .map(LiveTier::as_str)
                .collect(),
            summary,
            checks: results,
        },
        exit_code,
    ))
}

/// Returns why the check cannot run on this machine: a route it reaches lacks its
/// prerequisites, or a keyed check lacks the keys of a provider whose keys are optional.
fn unconfigured_reason(entry: &Declared, runtime: &RuntimeConfig) -> Option<String> {
    let Some(provider) = entry.provider else {
        let configured = runtime.main_search.configured_provider_count() > 0
            && runtime.web_search.configured_provider_count() > 0;
        return (!configured).then(|| {
            "the pipeline check needs a configured main search backend and web search provider"
                .into()
        });
    };
    let routes = match entry.check.command.routes {
        RoutePin::AnonymousWebFetch(_) => return None,
        RoutePin::Configured => std::slice::from_ref(&provider),
        ref pin => pin.routes(),
    };
    routes.iter().find_map(|route| {
        providers::route_prerequisites(*route, runtime)
            .err()
            .or_else(|| {
                catalog::registration(*route)
                    .lacks_optional_keys(runtime.provider_runtime(*route).keys.len())
                    .then(|| {
                        format!(
                            "{} has no configured credentials",
                            config::provider_keys_key(route.name())
                        )
                    })
            })
    })
}

/// A command ready to start: its arguments and the environment it adds.
#[derive(Debug)]
struct Prepared {
    arguments: Vec<String>,
    environment: Vec<(String, String)>,
}

/// How one started command ended.
#[derive(Debug)]
enum Execution {
    Exited {
        code: Option<i32>,
        stdout: Vec<u8>,
    },
    /// Killed at its time limit.
    TimedOut,
    /// It could not be started or observed.
    Broken(&'static str),
}

/// Why one attempt of a check did not pass.
#[derive(Debug)]
struct Failure {
    kind: FailureKind,
    error_kind: Cow<'static, str>,
    message: Cow<'static, str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FailureKind {
    Auth,
    Parameter,
    Runtime,
    Network,
    Timeout,
    /// The command succeeded, but its output breaks the contract.
    Contract,
    Other,
}

impl FailureKind {
    fn of(error_kind: &str) -> Self {
        match error_kind {
            "auth" => Self::Auth,
            "parameter" => Self::Parameter,
            "runtime" => Self::Runtime,
            "network" => Self::Network,
            "timeout" => Self::Timeout,
            _ => Self::Other,
        }
    }

    /// A network error or a timeout may not recur, so it is retried once; any other failure
    /// would get the same answer again.
    fn retryable(self) -> bool {
        matches!(self, Self::Network | Self::Timeout)
    }
}

/// What a run tracks about one browser site between its checks, in memory only.
#[derive(Default)]
struct Site {
    stopped: Option<String>,
    timed_out_last: bool,
}

impl Site {
    fn observe(&mut self, check: &str, failure: Option<&Failure>, signals: &[StopSignal]) {
        let kind = failure.map(|failure| failure.kind);
        let timed_out = kind == Some(FailureKind::Timeout);
        let signal = match kind {
            Some(FailureKind::Auth) => Some(StopSignal::Auth),
            Some(FailureKind::Parameter) => Some(StopSignal::Parameter),
            Some(FailureKind::Runtime) => Some(StopSignal::Runtime),
            Some(FailureKind::Timeout) if self.timed_out_last => {
                Some(StopSignal::TwoTimeoutsInARow)
            }
            _ => None,
        };
        if let Some(signal) = signal.filter(|signal| signals.contains(signal)) {
            self.stopped.get_or_insert_with(|| {
                format!(
                    "{check} met {}, so this run opens no more pages on the site",
                    signal.describe()
                )
            });
        }
        self.timed_out_last = timed_out;
    }
}

/// Runs each selected check in declaration order; `unconfigured` holds each check's reason not
/// to run, and `execute` starts one command with a time limit.
fn run_checks(
    declared: &[Declared],
    selected: &[bool],
    unconfigured: &[Option<String>],
    deadline: Deadline,
    credentials: &[Secret],
    execute: &mut dyn FnMut(&Prepared, Duration) -> Execution,
) -> Vec<CheckResult> {
    let mut outputs = BTreeMap::<&str, Value>::new();
    let mut sites = BTreeMap::<ProviderId, Site>::new();
    let mut results = Vec::new();
    for (index, entry) in declared.iter().enumerate() {
        if !selected[index] {
            continue;
        }
        let check = &entry.check;
        let result = |status, cause, attempts, failure: Option<Failure>, message| CheckResult {
            identity: CheckIdentity::of(entry),
            status,
            cause,
            attempts,
            error_kind: failure
                .as_ref()
                .map(|failure: &Failure| failure.error_kind.to_string()),
            message: failure.map_or(message, |failure| Some(failure.message.into_owned())),
        };
        if let Some(reason) = &unconfigured[index] {
            results.push(result(
                Status::Unconfigured,
                None,
                0,
                None,
                Some(reason.clone()),
            ));
            continue;
        }
        let site = entry
            .provider
            .filter(|_| !entry.stop_signals().is_empty())
            .map(|provider| sites.entry(provider).or_default());
        if let Some(reason) = site.as_ref().and_then(|site| site.stopped.clone()) {
            let message = format!("not started: {reason}");
            results.push(result(
                Status::NotStarted,
                Some(NotStarted::SiteStop),
                0,
                None,
                Some(message),
            ));
            continue;
        }
        let input = match input(check, &outputs) {
            Ok(input) => input,
            Err(message) => {
                results.push(result(
                    Status::NotStarted,
                    Some(NotStarted::NoInput),
                    0,
                    None,
                    Some(message),
                ));
                continue;
            }
        };
        if deadline.remaining().is_none() {
            let message = "not started: the run's time limit ended before this check began";
            results.push(result(
                Status::NotStarted,
                Some(NotStarted::Deadline),
                0,
                None,
                Some(message.into()),
            ));
            continue;
        }
        let retries = usize::from(entry.transport() == ProviderTransport::Http);
        let mut attempts = 0;
        let outcome = loop {
            attempts += 1;
            let outcome = attempt(check, input.as_deref(), deadline, credentials, execute);
            match &outcome {
                Err(failure)
                    if failure.kind.retryable()
                        && attempts <= retries
                        && deadline.remaining().is_some() => {}
                _ => break outcome,
            }
        };
        if let Some(site) = site {
            site.observe(check.id, outcome.as_ref().err(), entry.stop_signals());
        }
        results.push(match outcome {
            Ok(payload) => {
                outputs.insert(check.id, payload);
                result(Status::Passed, None, attempts, None, None)
            }
            Err(failure) => result(Status::Failed, None, attempts, Some(failure), None),
        });
    }
    results
}

/// The value a dependent check reads from its dependency's output, or why it has none.
fn input(check: &LiveCheck, outputs: &BTreeMap<&str, Value>) -> Result<Option<String>, String> {
    let Some(dependency) = check.depends_on else {
        return Ok(None);
    };
    let Some(output) = outputs.get(dependency) else {
        return Err(format!("not started: {dependency} did not pass"));
    };
    let Some(pick) = check
        .command
        .arguments
        .iter()
        .find_map(|argument| match argument {
            LiveArgument::Input(pick) => Some(*pick),
            _ => None,
        })
    else {
        return Ok(None);
    };
    picked(pick, output)
        .map(Some)
        .ok_or_else(|| format!("not started: {dependency} returned nothing this check can read"))
}

fn picked(pick: LiveInput, payload: &Value) -> Option<String> {
    let items = payload["items"].as_array()?;
    let reference = match pick {
        LiveInput::FirstItem(field) => items.first()?[field].as_str(),
        LiveInput::FirstSafeRef => items
            .iter()
            .find(|item| item["nsfw"] == Value::Bool(false))?["ref"]
            .as_str(),
        LiveInput::MostCommentedAccessUrl => most_commented(items)?["access_url"].as_str(),
    };
    reference.map(str::to_owned)
}

/// The item with the most comments, the first on a tie. Xiaohongshu writes counts as text such
/// as `156` or `1.2万`; an empty or missing count is 0.
fn most_commented(items: &[Value]) -> Option<&Value> {
    let count = |item: &Value| {
        let text = item["comments"].as_str().unwrap_or_default().trim();
        let (number, scale) = text
            .strip_suffix('万')
            .map_or((text, 1.0), |number| (number, 10_000.0));
        number.parse::<f64>().map_or(0.0, |number| number * scale)
    };
    items
        .iter()
        .fold(None::<(&Value, f64)>, |best, item| match best {
            Some((_, most)) if count(item) <= most => best,
            _ => Some((item, count(item))),
        })
        .map(|(item, _)| item)
}

/// Runs the check's command once and judges its output.
fn attempt(
    check: &LiveCheck,
    input: Option<&str>,
    deadline: Deadline,
    credentials: &[Secret],
    execute: &mut dyn FnMut(&Prepared, Duration) -> Execution,
) -> Result<Value, Failure> {
    let failure = |kind, message: &'static str| Failure {
        kind,
        error_kind: Cow::Borrowed(if kind == FailureKind::Contract {
            "contract"
        } else {
            "runtime"
        }),
        message: Cow::Borrowed(message),
    };
    let scratch = check
        .command
        .arguments
        .iter()
        .any(|argument| matches!(argument, LiveArgument::ContentDir))
        .then(tempfile::tempdir)
        .transpose()
        .map_err(|_| failure(FailureKind::Other, "cannot create a content directory"))?;
    let prepared = prepare(check, input, scratch.as_ref().map(tempfile::TempDir::path));
    let limit = deadline.remaining().map_or(Duration::ZERO, |remaining| {
        remaining.min(check.time_limit + KILL_GRACE)
    });
    let (code, stdout) = match execute(&prepared, limit) {
        Execution::Exited { code, stdout } => (code, stdout),
        Execution::TimedOut => {
            return Err(Failure {
                kind: FailureKind::Timeout,
                error_kind: Cow::Borrowed("timeout"),
                message: Cow::Borrowed("the command ran past its time limit and was killed"),
            });
        }
        Execution::Broken(message) => return Err(failure(FailureKind::Other, message)),
    };
    let payload = serde_json::from_slice::<Value>(&stdout);
    if code != Some(0) {
        return Err(nonzero_terminal(payload.ok().as_ref(), credentials));
    }
    let payload =
        payload.map_err(|_| failure(FailureKind::Contract, "the command printed invalid JSON"))?;
    match contract::violation(check.contract, &payload) {
        Some(violation) => Err(failure(FailureKind::Contract, violation)),
        None => Ok(payload),
    }
}

/// Says why a command exited nonzero: the error kind and message its stdout reports, with every
/// configured credential masked, or a fixed text when stdout reports neither.
fn nonzero_terminal(payload: Option<&Value>, credentials: &[Secret]) -> Failure {
    let reported = payload.and_then(|payload| {
        Some((
            payload["error_kind"].as_str()?,
            payload["message"].as_str()?,
        ))
    });
    match reported {
        Some((kind, message)) => Failure {
            kind: FailureKind::of(kind),
            error_kind: Cow::Owned(kind.to_owned()),
            message: Cow::Owned(redact_credentials(
                &format!("the command failed with {kind}: {message}"),
                credentials,
            )),
        },
        None => Failure {
            kind: FailureKind::Other,
            error_kind: Cow::Borrowed("runtime"),
            message: Cow::Borrowed("the command exited nonzero without an error kind and message"),
        },
    }
}

/// Builds the check's command line and the environment that holds it to its routes. Every
/// command runs once, without retries of its own, and stays out of the Search Result Journal,
/// because a Live Check is a canary, not use.
fn prepare(check: &LiveCheck, input: Option<&str>, scratch: Option<&Path>) -> Prepared {
    let mut arguments = check
        .command
        .arguments
        .iter()
        .map(|argument| match argument {
            LiveArgument::Text(text) => (*text).to_owned(),
            LiveArgument::Timeout => check.time_limit.as_secs().to_string(),
            LiveArgument::ContentDir => scratch
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            LiveArgument::Input(_) => input.unwrap_or_default().to_owned(),
        })
        .collect::<Vec<_>>();
    // A retrieval command reports its attempts only with `--verbose`, and the contracts read
    // them; the hidden `doctor` entry always reports them.
    if arguments.first().map(String::as_str) != Some("doctor") {
        arguments.push("--verbose".into());
    }
    let order = |routes: &[ProviderId]| {
        serde_json::to_string(&routes.iter().map(|route| route.name()).collect::<Vec<_>>())
            .expect("route names serialize")
    };
    let mut environment = vec![
        ("FORAGER_RETRY__MAX_ATTEMPTS".to_owned(), "1".to_owned()),
        ("FORAGER_JOURNAL__ENABLED".to_owned(), "false".to_owned()),
    ];
    match check.command.routes {
        RoutePin::Platform(platform, routes) => environment.push((
            format!(
                "FORAGER_PLATFORMS__{}__ORDER",
                platform.as_str().to_ascii_uppercase()
            ),
            order(routes),
        )),
        RoutePin::WebFetch(provider) => environment.push((
            "FORAGER_CAPABILITIES__WEB_FETCH__ORDER".into(),
            order(&[provider]),
        )),
        RoutePin::AnonymousWebFetch(provider) => environment.extend([
            (
                "FORAGER_CAPABILITIES__WEB_FETCH__ORDER".into(),
                order(&[provider]),
            ),
            (
                format!(
                    "FORAGER_PROVIDERS__{}__KEYS",
                    provider.name().to_ascii_uppercase()
                ),
                "[]".into(),
            ),
        ]),
        RoutePin::MainSearch(provider) => environment.extend([
            ("FORAGER_SEARCH__BACKENDS".into(), order(&[provider])),
            ("FORAGER_SEARCH__FALLBACK".into(), "off".into()),
        ]),
        RoutePin::Configured => {}
    }
    Prepared {
        arguments,
        environment,
    }
}

/// Starts this binary with `command` and kills it when `limit` passes.
fn execute(executable: &Path, command: &Prepared, limit: Duration) -> Execution {
    let mut process = Command::new(executable);
    process
        .args(&command.arguments)
        .envs(
            command
                .environment
                .iter()
                .map(|(name, value)| (name, value)),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let Ok(child) = SharedChild::spawn(&mut process).map(Arc::new) else {
        return Execution::Broken("cannot start the command");
    };
    let (Some(mut stdout), Some(mut stderr)) = (child.take_stdout(), child.take_stderr()) else {
        return Execution::Broken("cannot capture the command's output");
    };
    let stdout_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    // Drained so a full stderr pipe never stalls the command.
    let stderr_reader = thread::spawn(move || std::io::copy(&mut stderr, &mut std::io::sink()));
    let (status_sender, status_receiver) = mpsc::sync_channel(1);
    let waiting_child = Arc::clone(&child);
    thread::spawn(move || {
        let _ = status_sender.send(waiting_child.wait());
    });
    let status = match status_receiver.recv_timeout(limit) {
        Ok(Ok(status)) => status,
        Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => {
            return Execution::Broken("cannot wait for the command");
        }
        Err(RecvTimeoutError::Timeout) => {
            let _ = child.kill();
            let _ = status_receiver.recv();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Execution::TimedOut;
        }
    };
    let _ = stderr_reader.join();
    match stdout_reader.join() {
        Ok(Ok(stdout)) => Execution::Exited {
            code: status.code(),
            stdout,
        },
        _ => Execution::Broken("cannot collect the command's output"),
    }
}

fn summarize(results: &[CheckResult], declared: usize) -> Summary {
    let mut summary = Summary {
        unselected: declared - results.len(),
        ..Summary::default()
    };
    for result in results {
        *match result.status {
            Status::Passed => &mut summary.passed,
            Status::Failed => &mut summary.failed,
            Status::Unconfigured => &mut summary.unconfigured,
            Status::NotStarted => &mut summary.not_started,
        } += 1;
    }
    summary
}

/// `4` when a check failed or the time limit left one unstarted, `3` when no check ran, `0`
/// otherwise.
fn exit_code(results: &[CheckResult]) -> u8 {
    let failed = results.iter().any(|result| {
        result.status == Status::Failed || result.cause == Some(NotStarted::Deadline)
    });
    let ran = results
        .iter()
        .any(|result| matches!(result.status, Status::Passed | Status::Failed));
    if failed {
        4
    } else if ran {
        0
    } else {
        3
    }
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
