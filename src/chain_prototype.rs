//! PROTOTYPE — issue #44「定稿 provider 执行链抽象与注册模型」的签名骨架。
//!
//! 用途是给决策讨论一个可编译的具体形态，不是生产代码；票关闭后整个模块
//! 随分支丢弃，validated 的签名再按 ADR/决议正式落地。
//!
//! 覆盖票面五个待定项：
//! 1. `run_provider_chain`：五个 engine seam 的 fallback 链收敛为一个泛型函数，
//!    预算策略（ADR 0007 主路径优先 vs 辅助 seam 均分切片）作为参数；
//!    fetch 专属的薄内容检测不进泛型——由 fetch 的 `run` 闭包把「薄成功」
//!    映射成 `Err(Quality)`，链自然继续。
//! 2. `execute_v2`：四份 claim→尝试→记 attempt→轮换→退避循环统一。闭包签名
//!    增加 attempt `Deadline`（MCP provider 用它构造 `McpClient`，HTTP provider
//!    忽略），`AttemptFailureV2` 携带 `redirected_library_id`（context7 需要，
//!    `ProviderError` 已有同名字段）。同时内嵌 ADR 0007 的名额分离：重试门
//!    以 `retry_count` 计，不再被轮换挤占。
//! 3. `SeamEntries<C>`：seam 配置以 `(ProviderId, C)` 有序对存储，取代
//!    `order: Vec<String>` + `providers: BTreeMap<String, C>` 双结构；id 与
//!    config 同行携带，engine 侧 17 处 `expect` 失去存在条件。registry 的
//!    `ProviderConstructor` 与 `DoctorProbe` 两枚举与 `ProviderId` 一一对应，
//!    删除后由穷尽 match `ProviderId` 承担（编译器强制新 provider 补齐）。
//! 4. `Redactor`：借用式值类型替代 `redacted_message(msg, url, credentials)`
//!    的散装三参调用与 web_fetch 的私有副本。query 参数 `String` → `&str`
//!    见 `documentation_search_chained` 内注释；#46 落地后 `execute_v2` 闭包
//!    的 `String` 凭据同步换 `Secret`。
//! 5. 验收目标见 issue resolution comment：新增一个 provider 的“非新增代码”
//!    改动收敛到 registry 行 + 构造 match 臂 + config 解析 + doctor 臂 + fixture，
//!    其中 match 臂由穷尽性保证漏改即编译错误。
#![allow(dead_code)]

use std::collections::HashSet;
use std::future::Future;
use std::time::Instant;

use crate::config::{DocsSearchProviderConfig, DocsSearchRuntimeConfig};
use crate::credentials::CredentialPool;
use crate::engine::CapabilityExecution;
use crate::net::{
    McpClient, McpToolResult, combine_diagnostics, duration_millis, slice_budget, truncate_message,
};
use crate::providers::execution::{ExecutionOutcome, ExecutionSettings};
use crate::providers::{self, ProviderError, ProviderId};
use crate::types::{AttemptErrorKind, Deadline, ProviderAttempt, SupplementalSearchOutcome};

// ---------------------------------------------------------------------------
// 决策 1：run_provider_chain —— 五链收敛的泛型骨架
// ---------------------------------------------------------------------------

/// 每一环拿到的预算策略。
pub(crate) enum BudgetPolicy {
    /// ADR 0007：主路径用满全部剩余预算，fallback 只吃残余（main_search）。
    PrimaryFirst,
    /// 现状语义：`slice_budget` 均分，切片不足记 skipped attempt（辅助 seam）。
    SlicedEven,
}

pub(crate) struct ChainSettings {
    pub(crate) seam: &'static str,
    pub(crate) budget: BudgetPolicy,
    /// 链上没有任何 attempt 可归因时的终态消息。
    pub(crate) exhausted_message: &'static str,
    pub(crate) verbose: bool,
}

/// 链上的一环：id 与 config 同行携带，不再回查 BTreeMap（决策 3 的消费端）。
pub(crate) struct ChainEntry<C> {
    pub(crate) id: ProviderId,
    pub(crate) config: C,
    pub(crate) configured: bool,
}

/// 一环成功后的统一产物；各 seam 的 outcome 类型由适配闭包双向映射。
pub(crate) struct ChainStep<T> {
    pub(crate) value: T,
    pub(crate) attempts: Vec<ProviderAttempt>,
    pub(crate) diagnostic: Option<String>,
}

/// 组装可执行链：fallback off 时取首环（保留“未配置也占位、进链后记
/// Auth attempt 并停链”的现状语义），否则过滤未配置项。
pub(crate) fn executable_entries<C>(
    order: impl IntoIterator<Item = (ProviderId, C)>,
    fallback_off: bool,
    configured: impl Fn(&C) -> bool,
) -> Vec<ChainEntry<C>> {
    let entries = order.into_iter().map(|(id, config)| {
        let configured = configured(&config);
        ChainEntry {
            id,
            config,
            configured,
        }
    });
    if fallback_off {
        entries.take(1).collect()
    } else {
        entries.filter(|entry| entry.configured).collect()
    }
}

/// 五个 seam 共享的 fallback 骨架：预算→逐环尝试→首个 Ok 返回→terminal 归因。
pub(crate) async fn run_provider_chain<C, T, Run, Fut>(
    entries: Vec<ChainEntry<C>>,
    settings: ChainSettings,
    deadline: Deadline,
    mut run: Run,
) -> Result<ChainStep<T>, ProviderError>
where
    Run: FnMut(ProviderId, C, Deadline) -> Fut,
    Fut: Future<Output = Result<ChainStep<T>, ProviderError>>,
{
    let mut attempts = Vec::new();
    let mut diagnostics = Vec::new();
    let total = entries.len();
    for (index, entry) in entries.into_iter().enumerate() {
        if !entry.configured {
            attempts.push(unconfigured_attempt(entry.id, settings.seam));
            break;
        }
        let Some(remaining) = deadline.remaining() else {
            break;
        };
        let budget = match settings.budget {
            BudgetPolicy::PrimaryFirst => remaining,
            BudgetPolicy::SlicedEven => match slice_budget(remaining, total - index) {
                Some(budget) => budget,
                None => {
                    attempts.push(skipped_attempt(entry.id, settings.seam));
                    continue;
                }
            },
        };
        match run(entry.id, entry.config, Deadline::new(budget)).await {
            Ok(mut step) => {
                attempts.append(&mut step.attempts);
                if let Some(diagnostic) = step.diagnostic.take() {
                    diagnostics.push(diagnostic);
                }
                return Ok(ChainStep {
                    value: step.value,
                    attempts,
                    diagnostic: combine_diagnostics(diagnostics),
                });
            }
            Err(error) => {
                attempts.extend(error.attempts);
                if let Some(diagnostic) = error.diagnostic {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }
    let terminal = terminal_attempt(&attempts);
    Err(ProviderError {
        kind: terminal
            .and_then(|attempt| attempt.error_kind)
            .unwrap_or(AttemptErrorKind::Timeout),
        message: terminal.map_or_else(
            || settings.exhausted_message.into(),
            |attempt| attempt.message.clone(),
        ),
        attempts,
        verbose: settings.verbose,
        diagnostic: combine_diagnostics(diagnostics),
        redirected_library_id: None,
    })
}

// ---------------------------------------------------------------------------
// 试迁移：documentation_search 走链（对照 engine.rs:667-749）
// ---------------------------------------------------------------------------

/// `engine::documentation_search` 的链化重写：行为等价，54 行 → 组装 + 适配闭包。
/// 正式迁移时 config 解析直接产出 `SeamEntries`，此处的 `filter_map` 组装消失。
pub(crate) async fn documentation_search_chained(
    query: &str,
    limit: u16,
    config: &DocsSearchRuntimeConfig,
    execution: &CapabilityExecution,
) -> Result<SupplementalSearchOutcome, ProviderError> {
    let order = config.order.iter().filter_map(|name| {
        let id = ProviderId::parse(name)?;
        Some((id, config.provider(name)?.clone()))
    });
    let entries = executable_entries(
        order,
        execution.fallback == "off",
        DocsSearchProviderConfig::configured,
    );
    let step = run_provider_chain(
        entries,
        ChainSettings {
            seam: "docs_search",
            budget: BudgetPolicy::SlicedEven,
            exhausted_message: "documentation search has no executable provider",
            verbose: false,
        },
        execution.deadline,
        |id, provider_config, budget| {
            let client = execution.client.clone();
            let retry_policy = execution.retry_policy;
            async move {
                let outcome = providers::build_docs_search(
                    id.name(),
                    provider_config,
                    client,
                    retry_policy,
                    budget,
                )
                // 决策 4：trait 收 &str 后此处 to_owned 消失，每环不再复制 query。
                .search(query.to_owned(), limit)
                .await?;
                Ok(ChainStep {
                    value: outcome.sources,
                    attempts: outcome.attempts,
                    diagnostic: outcome.diagnostic,
                })
            }
        },
    )
    .await?;
    Ok(SupplementalSearchOutcome {
        sources: step.value,
        attempts: step.attempts,
        diagnostic: step.diagnostic,
    })
}

// ---------------------------------------------------------------------------
// 决策 2：execute_v2 —— 四份执行循环统一后的形态
// ---------------------------------------------------------------------------

/// `execution::AttemptFailure` 的收敛版：增加 context7 的 redirect 通道，
/// 使 MCP 分支可并入共享循环（`ProviderError` 已有同名字段，此处只是打通）。
pub(crate) struct AttemptFailureV2 {
    pub(crate) kind: AttemptErrorKind,
    pub(crate) status: Option<u16>,
    pub(crate) message: String,
    pub(crate) redirected_library_id: Option<String>,
}

/// 统一执行循环。与现行 `execution::execute` 的差异：
/// - 闭包签名 `FnMut(String, Deadline)`：attempt 级 deadline 传入闭包，MCP
///   provider 用它构造 `McpClient`，HTTP provider 忽略。#46 落地后凭据参数
///   由 `String` 换 owned `Secret`。
/// - 重试门以 `retry_count` 计（ADR 0007 名额分离）：轮换名额 = 凭据池大小，
///   重试名额 = `max_attempts`，互不挤占。
/// - exa 手写循环中「超时不退避、不查 deadline」的行为漂移随统一消失。
pub(crate) async fn execute_v2<T, F, Fut>(
    credentials: &CredentialPool,
    settings: ExecutionSettings,
    mut send_once: F,
) -> Result<ExecutionOutcome<T>, ProviderError>
where
    F: FnMut(String, Deadline) -> Fut,
    Fut: Future<Output = Result<(u16, T), AttemptFailureV2>>,
{
    let selection = credentials.claim();
    let mut attempts = Vec::new();
    let mut credential_index = selection.index;
    let mut retry_count = 0;
    let mut rotation_count = 0;
    let mut redirect = None;

    loop {
        let Some(remaining) = settings.deadline.remaining() else {
            return Err(terminal_error(
                &settings,
                AttemptErrorKind::Timeout,
                attempts,
                selection.diagnostic.clone(),
                redirect,
            ));
        };
        let attempt_limit = remaining.min(settings.attempt_timeout);
        let started = Instant::now();
        let response = tokio::time::timeout(
            attempt_limit,
            send_once(
                credentials.key(credential_index).to_owned(),
                Deadline::new(attempt_limit),
            ),
        )
        .await;
        let failure = match response {
            Ok(Ok((status, value))) => {
                attempts.push(attempt_record(
                    &settings,
                    None,
                    Some(status),
                    started,
                    credential_index,
                    retry_count,
                    rotation_count,
                    String::new(),
                ));
                return Ok(ExecutionOutcome {
                    value,
                    attempts,
                    diagnostic: selection.diagnostic,
                });
            }
            Ok(Err(failure)) => failure,
            Err(_) => AttemptFailureV2 {
                kind: AttemptErrorKind::Timeout,
                status: None,
                message: settings.timeout_message.into(),
                redirected_library_id: None,
            },
        };
        let kind = failure.kind;
        if failure.redirected_library_id.is_some() {
            redirect = failure.redirected_library_id;
        }
        attempts.push(attempt_record(
            &settings,
            Some(kind),
            failure.status,
            started,
            credential_index,
            retry_count,
            rotation_count,
            failure.message,
        ));

        if kind.rotates_credential() && rotation_count + 1 < credentials.len() {
            rotation_count += 1;
            credential_index = credentials.rotated_index(selection.index, rotation_count);
            continue;
        }
        if kind.is_retryable() && retry_count + 1 < settings.retry_policy.max_attempts() {
            retry_count += 1;
            let wait = settings.retry_policy.wait(retry_count);
            if settings
                .deadline
                .remaining()
                .is_none_or(|remaining| wait >= remaining)
            {
                return Err(terminal_error(
                    &settings,
                    AttemptErrorKind::Timeout,
                    attempts,
                    selection.diagnostic.clone(),
                    redirect,
                ));
            }
            tokio::time::sleep(wait).await;
            continue;
        }
        return Err(terminal_error(
            &settings,
            kind,
            attempts,
            selection.diagnostic.clone(),
            redirect,
        ));
    }
}

/// MCP provider 挂进 `execute_v2` 的 send_once 形态示例：错误分类与结果解码
/// 仍留在各 provider（context7 的 `Context7Failure::from`、anysearch 的参数值
/// 脱敏都在返回 `AttemptFailureV2` 前完成），共享循环只见统一失败类型。
pub(crate) async fn mcp_send_once_demo(
    client: &reqwest::Client,
    url: &str,
    tool: &'static str,
    arguments: serde_json::Value,
    credential: &str,
    attempt_deadline: Deadline,
) -> Result<(u16, McpToolResult), AttemptFailureV2> {
    McpClient::new(client, url, attempt_deadline)
        .call_tool(credential, tool, arguments)
        .await
        .map(|result| (200, result))
        .map_err(|error| AttemptFailureV2 {
            kind: error.kind,
            status: error.status,
            message: error.message,
            redirected_library_id: None,
        })
}

// ---------------------------------------------------------------------------
// 决策 3：注册模型 —— SeamEntries 取代 order + BTreeMap 双结构
// ---------------------------------------------------------------------------

/// seam 配置的目标形态：解析期把 `order: Vec<String>` 与
/// `providers: BTreeMap<String, C>` 合并成有序对，名字→id→config 的解析
/// 与校验一次完成，engine 侧按对迭代，`expect("validated …")` 全部消失。
/// `ProviderConstructor` / `DoctorProbe` 两枚举删除，构造与探针改为穷尽
/// match `ProviderId`（新增 provider 漏改即编译错误）。
pub(crate) type SeamEntries<C> = Vec<(ProviderId, C)>;

// ---------------------------------------------------------------------------
// 决策 4：Redactor 值类型 + query 借用
// ---------------------------------------------------------------------------

/// 借用式脱敏器：替代 `redacted_message(message, endpoint, credentials)` 散装
/// 三参调用（exa 6 处、context7/anysearch/web_fetch 各自的私有变体）。
/// provider 持有 credentials 与 config，随取随用，零分配零所有权转移。
pub(crate) struct Redactor<'a> {
    pub(crate) credentials: &'a CredentialPool,
    pub(crate) endpoint: &'a str,
}

impl Redactor<'_> {
    pub(crate) fn text(&self, raw: &str) -> String {
        self.credentials
            .redact(raw)
            .replace(self.endpoint, &crate::config::redact_url(self.endpoint))
    }

    pub(crate) fn message(&self, raw: &str) -> String {
        truncate_message(&self.text(raw))
    }
}

// ---------------------------------------------------------------------------
// 支撑件（正式迁移时与 engine.rs 现有私有函数合并，此处为编译自足的副本）
// ---------------------------------------------------------------------------

fn unconfigured_attempt(id: ProviderId, seam: &'static str) -> ProviderAttempt {
    synthetic_attempt(
        id,
        seam,
        AttemptErrorKind::Auth,
        format!("{} has no configured credentials", id.name()),
    )
}

fn skipped_attempt(id: ProviderId, seam: &'static str) -> ProviderAttempt {
    synthetic_attempt(
        id,
        seam,
        AttemptErrorKind::Timeout,
        "skipped to preserve fallback deadline budget".into(),
    )
}

fn synthetic_attempt(
    id: ProviderId,
    seam: &'static str,
    kind: AttemptErrorKind,
    message: String,
) -> ProviderAttempt {
    ProviderAttempt {
        provider: id.name(),
        seam,
        error_kind: Some(kind),
        http_status: None,
        duration_ms: 0,
        credential_index: 0,
        retry_count: 0,
        rotation_count: 0,
        message,
        model: None,
        transport: None,
        endpoint_host: None,
        breaker_event: None,
    }
}

// 原型内平铺参数以便对照现行 execute 的字段；正式迁移时并入循环局部结构。
#[expect(clippy::too_many_arguments)]
fn attempt_record(
    settings: &ExecutionSettings,
    error_kind: Option<AttemptErrorKind>,
    http_status: Option<u16>,
    started: Instant,
    credential_index: usize,
    retry_count: usize,
    rotation_count: usize,
    message: String,
) -> ProviderAttempt {
    ProviderAttempt {
        provider: settings.provider,
        seam: settings.seam,
        error_kind,
        http_status,
        duration_ms: duration_millis(started.elapsed()),
        credential_index,
        retry_count,
        rotation_count,
        message,
        model: settings.model.clone(),
        transport: settings.transport,
        endpoint_host: settings.endpoint_host.clone(),
        breaker_event: settings.breaker_event,
    }
}

fn terminal_error(
    settings: &ExecutionSettings,
    kind: AttemptErrorKind,
    attempts: Vec<ProviderAttempt>,
    diagnostic: Option<String>,
    redirected_library_id: Option<String>,
) -> ProviderError {
    let message = attempts.last().map_or_else(
        || format!("{} request failed", settings.provider),
        |attempt| attempt.message.clone(),
    );
    ProviderError {
        kind,
        message,
        attempts,
        verbose: settings.verbose,
        diagnostic,
        redirected_library_id,
    }
}

fn terminal_attempt(attempts: &[ProviderAttempt]) -> Option<&ProviderAttempt> {
    let mut final_providers = HashSet::new();
    attempts
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, attempt)| final_providers.insert(attempt.provider))
        .filter(|(_, attempt)| attempt.error_kind.is_some())
        .max_by_key(|(index, attempt)| {
            (
                error_priority(attempt.error_kind.expect("filtered error kind")),
                *index,
            )
        })
        .map(|(_, attempt)| attempt)
}

fn error_priority(kind: AttemptErrorKind) -> u8 {
    match kind {
        AttemptErrorKind::Network => 0,
        AttemptErrorKind::Timeout => 1,
        AttemptErrorKind::RateLimited => 2,
        AttemptErrorKind::QuotaExhausted => 3,
        AttemptErrorKind::Auth => 4,
        AttemptErrorKind::Parameter => 5,
        AttemptErrorKind::Runtime => 6,
        AttemptErrorKind::Quality => 7,
        AttemptErrorKind::Evidence => 8,
    }
}
