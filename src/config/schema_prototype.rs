//! PROTOTYPE (#45) — throwaway；勿并入 main。
//!
//! 验证：单一 SCHEMA 表能否派生 config.rs 的五张手工映射
//! （`LEAVES` / `path_kind` / `apply_env_value` / `template_comment` /
//! `validate*`），并锁定 `Leaf` 的字段形态。等价性由本文件的测试与
//! 旧实现逐 leaf 比对证明。`cargo test --lib config::schema_prototype` 运行。
//!
//! 目标 mod 树（拆分落地时的去向，本原型只迁 apply 这一张表）：
//!
//! ```text
//! src/config/
//!   mod.rs       门面 re-export + ConfigError / EditError
//!   schema.rs    Config 结构体族 + Default + SCHEMA 表（本文件类型转正）
//!   load.rs      load_effective_config + apply_environment(表驱动) + normalize
//!   validate.rs  validate 执行器（表驱动 Rule）+ value_error + source_position
//!   runtime.rs   *RuntimeConfig 族 + runtime_config() + resolve_journal_dir
//!   view.rs      effective_view + build_view（表驱动，按 path 构嵌套 JSON）
//!   edit.rs      set/unset + SetupDocument + 模板 + toml_edit helpers
//!   location.rs  ConfigLocation + verify_default_directory
//! src/secure_fs.rs  atomic_write/create + 锁 + private file/ACL（credentials 与
//!                   journal 直接消费，非 config 专属，提为顶层）
//! src/redact.rs     CREDENTIAL_MASK + redact_url/redact_urls/redact_credentials
//!                   （14 模块消费；#46 的 Secret newtype 同住此模块）
//! ```

#![allow(dead_code)]

use super::{Config, ValueKind};

/// 目标形态：一个 leaf 一行声明，五张表全部由此派生。
struct Leaf {
    path: &'static str,
    /// 与 `get`/`get_mut` 的 variant 一致；由 `leaf!` 宏从同一 ident 展开。
    kind: ValueKind,
    get: fn(&Config) -> FieldRef<'_>,
    get_mut: fn(&mut Config) -> FieldMut<'_>,
    rule: Rule,
    view: View,
    /// `template_comment` 的 purpose 文案。
    comment: &'static str,
}

enum FieldRef<'a> {
    String(&'a str),
    Bool(bool),
    I64(i64),
    F64(f64),
    Strings(&'a [String]),
    // #46 落地后新增：Secrets(&'a [Secret])——keys 行把 Strings 换成 Secrets，
    // apply 执行器多一个 parse_secret_array 分支，其余表行不动。
}

enum FieldMut<'a> {
    String(&'a mut String),
    Bool(&'a mut bool),
    I64(&'a mut i64),
    F64(&'a mut f64),
    Strings(&'a mut Vec<String>),
}

/// validate / validate_edit_value 的按 leaf 规则（执行器在落地票实现）。
enum Rule {
    Any,
    OneOf(&'static [&'static str]),
    Subset {
        allowed: &'static [&'static str],
        unique: bool,
        allow_empty: bool,
    },
    /// integer > 0（i64→u64 收紧后语义变为 != 0，NonNegative 整类删除）。
    Positive,
    /// integer >= 0（u64 收紧后由类型保证，规则退化为 Any）。
    NonNegative,
    /// f64：finite 且 > 0。
    PositiveFinite,
    /// `providers::supports(capability, entry)` 逐项成立。
    CapabilityOrder {
        capability: &'static str,
        allow_empty: bool,
    },
}

/// build_view 的投影方式：Keys 可由 kind+路径派生，但显式标注更可审计。
enum View {
    Plain,
    Url,
    Keys,
}

macro_rules! kind_of {
    (String) => {
        ValueKind::String
    };
    (Bool) => {
        ValueKind::Boolean
    };
    (I64) => {
        ValueKind::Integer
    };
    (F64) => {
        ValueKind::Float
    };
    (Strings) => {
        ValueKind::Array
    };
}

macro_rules! leaf {
    ($path:literal, $($field:ident).+ : $variant:ident, $rule:expr, $view:expr, $comment:literal) => {{
        fn get(config: &Config) -> FieldRef<'_> {
            FieldRef::from(&config.$($field).+)
        }
        fn get_mut(config: &mut Config) -> FieldMut<'_> {
            FieldMut::$variant(&mut config.$($field).+)
        }
        Leaf {
            path: $path,
            kind: kind_of!($variant),
            get,
            get_mut,
            rule: $rule,
            view: $view,
            comment: $comment,
        }
    }};
}

const BACKENDS: &[&str] = &["xai", "openai_compatible"];
const XAI_TOOLS: &[&str] = &["web_search", "x_search"];
const VALIDATION: &[&str] = &["fast", "balanced", "strict"];
const FALLBACK: &[&str] = &["auto", "off"];
const LOG_LEVELS: &[&str] = &["error", "warn", "info", "debug", "trace"];

fn schema() -> Vec<Leaf> {
    use Rule::{Any, CapabilityOrder, NonNegative, OneOf, Positive, PositiveFinite, Subset};
    use View::{Keys, Plain, Url};
    vec![
        leaf!("search.backends", search.backends: Strings, Subset { allowed: BACKENDS, unique: true, allow_empty: false }, Plain, "ordered main-model backends"),
        leaf!("search.validation", search.validation: String, OneOf(VALIDATION), Plain, "result validation level: fast, balanced, or strict"),
        leaf!("search.fallback", search.fallback: String, OneOf(FALLBACK), Plain, "fallback policy: auto or off"),
        leaf!("classifier.url", classifier.url: String, Any, Url, "service endpoint URL"),
        leaf!("classifier.keys", classifier.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("classifier.model", classifier.model: String, Any, Plain, "model identifier"),
        leaf!("classifier.fallback_models", classifier.fallback_models: Strings, Any, Plain, "ordered fallback model identifiers"),
        leaf!("classifier.timeout", classifier.timeout: I64, Positive, Plain, "shared timeout in seconds; must be greater than zero"),
        leaf!("providers.xai.url", providers.xai.url: String, Any, Url, "service endpoint URL"),
        leaf!("providers.xai.keys", providers.xai.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("providers.xai.model", providers.xai.model: String, Any, Plain, "model identifier"),
        leaf!("providers.xai.tools", providers.xai.tools: Strings, Subset { allowed: XAI_TOOLS, unique: false, allow_empty: true }, Plain, "xAI tools enabled for the main model"),
        leaf!("providers.openai_compatible.url", providers.openai_compatible.url: String, Any, Url, "service endpoint URL"),
        leaf!("providers.openai_compatible.keys", providers.openai_compatible.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("providers.openai_compatible.model", providers.openai_compatible.model: String, Any, Plain, "model identifier"),
        leaf!("providers.openai_compatible.fallback_models", providers.openai_compatible.fallback_models: Strings, Any, Plain, "ordered fallback model identifiers"),
        leaf!("providers.openai_compatible.stream", providers.openai_compatible.stream: Bool, Any, Plain, "enable streaming transport"),
        leaf!("providers.exa.url", providers.exa.url: String, Any, Url, "service endpoint URL"),
        leaf!("providers.exa.keys", providers.exa.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("providers.exa.timeout", providers.exa.timeout: I64, Positive, Plain, "shared timeout in seconds; must be greater than zero"),
        leaf!("providers.context7.url", providers.context7.url: String, Any, Url, "service endpoint URL"),
        leaf!("providers.context7.keys", providers.context7.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("providers.context7.timeout", providers.context7.timeout: I64, Positive, Plain, "shared timeout in seconds; must be greater than zero"),
        leaf!("providers.jina.url", providers.jina.url: String, Any, Url, "service endpoint URL"),
        leaf!("providers.jina.keys", providers.jina.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("providers.jina.respond_with", providers.jina.respond_with: String, Any, Plain, "optional X-Respond-With header value"),
        leaf!("providers.jina.timeout", providers.jina.timeout: I64, Positive, Plain, "shared timeout in seconds; must be greater than zero"),
        leaf!("providers.tavily.url", providers.tavily.url: String, Any, Url, "service endpoint URL"),
        leaf!("providers.tavily.keys", providers.tavily.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("providers.tavily.timeout", providers.tavily.timeout: I64, Positive, Plain, "shared timeout in seconds; must be greater than zero"),
        leaf!("providers.firecrawl.url", providers.firecrawl.url: String, Any, Url, "service endpoint URL"),
        leaf!("providers.firecrawl.keys", providers.firecrawl.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("providers.firecrawl.timeout", providers.firecrawl.timeout: I64, Positive, Plain, "shared timeout in seconds; must be greater than zero"),
        leaf!("providers.anysearch.url", providers.anysearch.url: String, Any, Url, "service endpoint URL"),
        leaf!("providers.anysearch.keys", providers.anysearch.keys: Strings, Any, Keys, "credential pool; keep empty until credentials are available"),
        leaf!("providers.anysearch.timeout", providers.anysearch.timeout: I64, Positive, Plain, "shared timeout in seconds; must be greater than zero"),
        leaf!("capabilities.web_search.order", capabilities.web_search.order: Strings, CapabilityOrder { capability: "web_search", allow_empty: true }, Plain, "authoritative provider order for this capability"),
        leaf!("capabilities.web_fetch.order", capabilities.web_fetch.order: Strings, CapabilityOrder { capability: "web_fetch", allow_empty: false }, Plain, "authoritative provider order for this capability"),
        leaf!("capabilities.docs_search.order", capabilities.docs_search.order: Strings, CapabilityOrder { capability: "docs_search", allow_empty: true }, Plain, "authoritative provider order for this capability"),
        leaf!("capabilities.vertical_search.order", capabilities.vertical_search.order: Strings, CapabilityOrder { capability: "vertical_search", allow_empty: true }, Plain, "authoritative provider order for this capability"),
        leaf!("log.level", log.level: String, OneOf(LOG_LEVELS), Plain, "stderr log level"),
        leaf!("journal.enabled", journal.enabled: Bool, Any, Plain, "record search result journals"),
        leaf!("journal.dir", journal.dir: String, Any, Plain, "journal storage directory"),
        leaf!("journal.retention_days", journal.retention_days: I64, NonNegative, Plain, "journal retention in days; zero keeps entries indefinitely"),
        leaf!("retry.max_attempts", retry.max_attempts: I64, Positive, Plain, "maximum attempts per request"),
        leaf!("retry.multiplier", retry.multiplier: F64, PositiveFinite, Plain, "retry backoff multiplier"),
        leaf!("retry.max_wait", retry.max_wait: I64, NonNegative, Plain, "maximum retry wait in seconds"),
        leaf!("http.ssl_verify", http.ssl_verify: Bool, Any, Plain, "verify TLS certificates"),
    ]
}

/// `apply_env_value` 的表驱动替身：match 五个 variant 取代 48 臂 match。
/// `retry.multiplier` 的 `integer!`-赋-f64 命名陷阱在此结构下不可再现。
fn apply(leaf: &Leaf, config: &mut Config, raw: &str) -> Result<(), ()> {
    match (leaf.get_mut)(config) {
        FieldMut::String(slot) => *slot = raw.to_owned(),
        FieldMut::Bool(slot) => *slot = raw.parse().map_err(|_| ())?,
        FieldMut::I64(slot) => *slot = raw.parse().map_err(|_| ())?,
        FieldMut::F64(slot) => *slot = raw.parse().map_err(|_| ())?,
        FieldMut::Strings(slot) => *slot = super::parse_string_array(raw)?,
    }
    Ok(())
}

impl<'a> From<&'a String> for FieldRef<'a> {
    fn from(value: &'a String) -> Self {
        Self::String(value)
    }
}

impl<'a> From<&'a Vec<String>> for FieldRef<'a> {
    fn from(value: &'a Vec<String>) -> Self {
        Self::Strings(value)
    }
}

impl From<&bool> for FieldRef<'_> {
    fn from(value: &bool) -> Self {
        Self::Bool(*value)
    }
}

impl From<&i64> for FieldRef<'_> {
    fn from(value: &i64) -> Self {
        Self::I64(*value)
    }
}

impl From<&f64> for FieldRef<'_> {
    fn from(value: &f64) -> Self {
        Self::F64(*value)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde::Deserialize;

    use super::super::{LEAVES, apply_env_value, path_kind, template_comment};
    use super::{Config, FieldMut, FieldRef, Leaf, ValueKind, apply, schema};

    fn kind_tag(kind: ValueKind) -> &'static str {
        match kind {
            ValueKind::String => "string",
            ValueKind::Boolean => "boolean",
            ValueKind::Integer => "integer",
            ValueKind::Float => "float",
            ValueKind::Array => "array",
        }
    }

    fn accessor_tags(leaf: &Leaf) -> (&'static str, &'static str) {
        let mut config = Config::default();
        let get = match (leaf.get)(&config) {
            FieldRef::String(_) => "string",
            FieldRef::Bool(_) => "boolean",
            FieldRef::I64(_) => "integer",
            FieldRef::F64(_) => "float",
            FieldRef::Strings(_) => "array",
        };
        let get_mut = match (leaf.get_mut)(&mut config) {
            FieldMut::String(_) => "string",
            FieldMut::Bool(_) => "boolean",
            FieldMut::I64(_) => "integer",
            FieldMut::F64(_) => "float",
            FieldMut::Strings(_) => "array",
        };
        (get, get_mut)
    }

    #[test]
    fn schema_paths_match_legacy_leaves_exactly() {
        let table: Vec<_> = schema().iter().map(|leaf| leaf.path).collect();
        let unique: BTreeSet<_> = table.iter().copied().collect();
        assert_eq!(table.len(), unique.len(), "duplicate schema paths");
        assert_eq!(unique, LEAVES.iter().copied().collect::<BTreeSet<_>>());
    }

    #[test]
    fn schema_kind_matches_legacy_path_kind_and_accessors() {
        for leaf in schema() {
            let legacy = path_kind(leaf.path).expect("legacy kind");
            assert_eq!(kind_tag(leaf.kind), kind_tag(legacy), "path={}", leaf.path);
            let (get, get_mut) = accessor_tags(&leaf);
            assert_eq!(kind_tag(leaf.kind), get, "path={}", leaf.path);
            assert_eq!(kind_tag(leaf.kind), get_mut, "path={}", leaf.path);
        }
    }

    #[test]
    fn schema_comment_matches_legacy_template_comment() {
        for leaf in schema() {
            assert_eq!(
                format!("{}: {}", leaf.path, leaf.comment),
                template_comment(leaf.path),
                "path={}",
                leaf.path
            );
        }
    }

    #[test]
    fn schema_apply_matches_legacy_apply_env_value() {
        let samples: &[(&str, &[&str])] = &[
            ("string", &["hello"]),
            ("boolean", &["true", "false", "yes", "1", ""]),
            ("integer", &["7", "-3", "abc", "1.5", ""]),
            ("float", &["1.5", "-0.5", "abc", ""]),
            ("array", &[r#"["a", "b"]"#, r#"[]"#, r#"[1]"#, "abc", ""]),
        ];
        for leaf in schema() {
            let raws = samples
                .iter()
                .find(|(tag, _)| *tag == kind_tag(leaf.kind))
                .map(|(_, raws)| *raws)
                .expect("sample values");
            for raw in raws {
                let mut legacy = Config::default();
                let mut driven = Config::default();
                let legacy_result = apply_env_value(&mut legacy, leaf.path, raw);
                let driven_result = apply(&leaf, &mut driven, raw);
                assert_eq!(
                    legacy_result.is_ok(),
                    driven_result.is_ok(),
                    "path={} raw={raw:?}",
                    leaf.path
                );
                assert_eq!(
                    toml::to_string(&legacy).expect("serialize"),
                    toml::to_string(&driven).expect("serialize"),
                    "path={} raw={raw:?}",
                    leaf.path
                );
            }
        }
    }

    // 决策 4 的前提验证：i64→u64 收紧后，serde 拒负值的报错带完整 key
    // path，可无损替换 validate 中的手写 `>= 0` 检查。
    #[test]
    fn u64_field_rejects_negative_value_with_key_path() {
        #[derive(Debug, Deserialize)]
        struct Probe {
            #[allow(dead_code)]
            retry: ProbeRetry,
        }
        #[derive(Debug, Deserialize)]
        struct ProbeRetry {
            #[allow(dead_code)]
            max_wait: u64,
        }
        let deserializer = toml::Deserializer::parse("[retry]\nmax_wait = -1").expect("parse");
        let error = serde_path_to_error::deserialize::<_, Probe>(deserializer)
            .expect_err("negative must fail");
        assert_eq!(error.path().to_string(), "retry.max_wait");
        assert!(
            error.inner().to_string().contains("-1"),
            "diagnostic should cite the offending value: {}",
            error.inner()
        );
    }
}
