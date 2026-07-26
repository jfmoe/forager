# 现有 Python 架构痛点梳理（Rust 重写输入）

> 调查日期：2026-07-23；源码基线：`main @ 11ac647`（smart-search 0.7.1）。
> 对应 GitHub issue：[#55 现有架构痛点梳理](https://github.com/jfmoe/smartsearch/issues/55)（Part of #52 Rust 重写决策）。
>
> 性质：面向 Rust 新架构设计的「反面清单」。每条痛点附具体文件/行号证据，并给出对新架构的设计启示。
> 「设计启示」是建议，不构成对现有实现的价值终审；现状描述均可回溯到源码。

## 执行摘要

通读 `src/smart_search/`（25 个模块、约 14.3K 行），核心结论如下。

1. **两个 God 模块承载了绝大部分复杂度。** `service.py` 4972 行、约 100 个顶层函数，混合了编排、路由、每个 provider 的调用包装、断路器、research 计划、ML 路由标定、doctor/smoke/config 透传；`config.py` 1074 行单例，手工维护 ~90 个配置键。这两个文件是长期腐化的主震中（`src/smart_search/service.py`、`src/smart_search/config.py`）。
2. **Provider 抽象是一层「谎言」。** `BaseSearchProvider.search()` 声明返回 `List[SearchResult]`，但没有任何 provider 遵守：Exa/xAI 返回 JSON `str`，Zhipu 返回 `str`，Jina/Tavily/Firecrawl 根本不继承基类。真正的契约是「一个未类型化的 `dict`/JSON 字符串形状」，靠约定俗成维系（`src/smart_search/providers/base.py:30-41`）。
3. **HTTP/重试/错误分类样板在 6+ 个 provider 中被复制，且已发生语义漂移。** `_error_payload`、`_elapsed_ms`、`_is_retryable_exception`、`RETRYABLE_STATUS_CODES`、`_mask_secret`、`_parse_sse` 各写各的；`RETRYABLE_STATUS_CODES` 在 openai_compatible 含 429、其余不含（`src/smart_search/providers/openai_compatible.py:40` vs `src/smart_search/providers/exa.py:14`）。
4. **error_type 分类法没有单一真相源，散落在至少 4 处且互相不一致。** exa 把 `{400,422}` 归 `parameter_error`，jina 只认 422，service.py 又有一份 `_http_status_error_type`（`src/smart_search/service.py:429-436`）。下游用字符串集合硬编码判断 `status=error` 还是 `empty`（如 `src/smart_search/service.py:2017`、`:2151`）。
5. **两套并行的凭据体系。** Provider Credential Pool 只覆盖 6 个 allowlist provider；xAI/openai-compatible/zhipu/zhipu-mcp 走 config 上的裸属性，凭据解析路径完全不同（`src/smart_search/credential_pool.py:21-28`、`src/smart_search/service.py:643-668`）。
6. **搜索结果是一个巨大的隐式 dict 契约，在多处手工重复拼装。** `_empty_search_result`、`search()` 末尾、`research()` 末尾、各 `_primary_search_error_result` 各自组装同一形状但字段并不完全一致（`src/smart_search/service.py:327-361`、`:2845-2873`、`:1496-1527`）。
7. **可测试性靠海量 monkeypatch 维持。** 测试 11.8K 行、`test_service.py` 单文件 446 处 mock/patch；全局可变状态（config 单例、模块级断路器字典）必须手工 `reset_*` 才能隔离（`tests/test_service.py`、`src/smart_search/service.py:309`、`:443-444`）。
8. **模块耦合是双向且深入私有面的。** `service.py` 从 `intent_router` 导入 6 个下划线私有符号；provider 之间横向 import 私有实现（`xai_responses` 复用 `openai_compatible._WaitWithRetryAfter` 等）。
9. **新 provider 接入成本高且发散。** 接一个 provider 要改：`providers/` 新文件（自带全套 HTTP 样板）、`config.py`（键+属性+`get_config_info` 三处）、`credential_pool` allowlist、`service.py`（`PROVIDER_PROFILES`/`RESEARCH_PROFILE_ORDER`/`_provider_configured`/fallback 链/inline `call_*`）、`cli.py`（subparser）、doctor/smoke。且没有统一 registry。

以下逐条展开，每条给出证据与 Rust 设计启示。

---

## 反面清单

### 1. `service.py` 是 5000 行的 God 模块

**证据**：`src/smart_search/service.py` 共 4972 行、约 100 个顶层 `def/async def`（见 `grep -nE "^(async def|def) "`）。它同时承担：

- 主搜索编排 `search()`（`:2542-2873`，单函数约 330 行）；
- research 两阶段执行 `research()`（`:1267-1529`）与离线计划 `build_deep_research_plan()`（`:991-1264`，约 270 行、大量关键词分支）；
- 每个 provider 的薄包装 `exa_search`/`zhipu_search`/`context7_library`/`context7_docs`/`jina_fetch`/`zhipu_mcp_*`/`anysearch_*`（`:3462-3800`）；
- Tavily/Firecrawl 的**内联** HTTP 实现 `call_tavily_*`/`call_firecrawl_*`（`:2282-2540`）——这两个 provider 甚至没有 `providers/` 下的类；
- openai-compatible 的 model 断路器（模块级全局 dict，`:307-474`）；
- ML 路由标定：macro-F1、混淆矩阵、阈值/margin 网格搜索（`:3035-3386`）；
- doctor/smoke/diagnose/config 透传（`:3794-4519`）。

**设计启示（Rust）**：按「能力（capability）」而非「一个巨型 service」切分 crate/模块：`orchestrator`（编排）、`providers`（各 provider 独立）、`routing`、`research`、`diagnostics`。标定、doctor、smoke 是独立工具面，不应与运行时编排同居一个编译单元。内联的 Tavily/Firecrawl 必须与其他 provider 一样成为一等 provider 实现。

### 2. Provider 抽象基类形同虚设（隐式契约）

**证据**：`BaseSearchProvider` 声明 `async def search(self, query, max_results=5) -> List[SearchResult]`（`src/smart_search/providers/base.py:30-41`）。实际：

- `ExaSearchProvider.search(...) -> str`（返回 `json.dumps`，签名带 8 个关键字参数，`src/smart_search/providers/exa.py:74-145`）；
- `XAIResponsesSearchProvider.search(...) -> str`（`src/smart_search/providers/xai_responses.py:58-61`）；
- `OpenAICompatibleSearchProvider.search(...) -> List[SearchResult]` 类型注解，实际返回 `str`（`src/smart_search/providers/openai_compatible.py:141-164`）；
- `JinaReaderProvider`、内联 Tavily/Firecrawl 根本不继承 `BaseSearchProvider`。

`SearchResult` 类（`base.py:5-27`）在生产路径里几乎不被使用；真正流通的是裸 `dict`。编排层靠 `getattr(search_provider, "last_transport_attempts", [])` 这种 duck-typing 探测能力（`src/smart_search/service.py:529`）。

**设计启示（Rust）**：用 `trait` 定义 provider 契约，并让返回类型是**强类型枚举/结构体**（如 `ProviderResponse { answer, sources, attempts, .. }`），而非 JSON 字符串。能力差异（是否有 transport fallback、是否流式）用 trait 方法或关联类型显式建模，而不是 `getattr` 探测。编译器强制所有 provider 遵守同一契约，消灭「基类说一套、实现做一套」。

### 3. HTTP/重试/错误样板在 6+ provider 中复制并已漂移

**证据**：以下辅助函数在多个 provider 文件中各写一份：

- `_error_payload`：exa（`exa.py:45`）、jina（`jina.py:20`）、zhipu（`zhipu.py:37`）、zhipu_mcp（`zhipu_mcp.py:13`）——四份互不相同的实现。
- `_elapsed_ms`：jina、zhipu、zhipu_mcp、openai_compatible 各一份。
- `_is_retryable_exception` + `RETRYABLE_STATUS_CODES`：exa、zhipu、openai_compatible、context7 各一份。**且已漂移**：`openai_compatible.py:40` 的集合含 `429`，而 `exa.py:14`/`zhipu.py:13`/`context7.py:15` 不含 429（因为 429 走凭据轮换）。这种「同名常量不同取值」正是长期腐化的典型。
- `_mask_secret`：jina（`jina.py:40`）、zhipu_mcp（`zhipu_mcp.py:31`）重复。
- `_parse_sse` / SSE 解析：context7（`context7.py:65`）、zhipu_mcp（`zhipu_mcp.py:48`）、openai_compatible（`:323-360`）、xai_responses（`:86-116`）各写一套 SSE/JSON 兼容解析。
- SSL 校验 warning 的模块级全局 `_ssl_warning_emitted` 在 `openai_compatible.py:16` 与 `xai_responses.py:16` 各有一份。

**设计启示（Rust）**：抽出单一 `http` 层：一个共享 client 构造器（超时、redirect、SSL 策略）、一个 `RetryPolicy`、一个 `classify_error(status) -> ErrorKind` 的**唯一**实现、一个共享 SSE/JSON-RPC 解析器。provider 只声明 endpoint、payload、如何把响应映射到强类型结果。retryable 状态码集合必须是**一处定义**，429 是否 retry 作为策略参数传入而非各文件硬编码。

### 4. 两套独立的 MCP transport 实现

**证据**：Remote MCP（JSON-RPC over HTTP + SSE、session initialize）被独立实现了至少两遍：

- `Context7Provider`：`_start_session`（initialize + `notifications/initialized` + `Mcp-Session-Id` 头）、`_post_with_retry`、`_request`、`_parse_sse`、`_call_tool`（`src/smart_search/providers/context7.py:197-337`）。
- `ZhipuMCPProvider`：`call_tool` 自建 jsonrpc envelope、`_parse_sse_or_json`、`_normalize_response`（`src/smart_search/providers/zhipu_mcp.py:114-227`）。
- AnySearch 是第三套 MCP 风格调用（`src/smart_search/providers/anysearch.py`，含自己的 `_safe_error`/`_argument_secrets`/schema 校验）。

三者对 session、SSE、错误、密钥脱敏各有一套，互不复用。

**设计启示（Rust）**：把 MCP 当作**一个 transport 类型**（`McpClient`：session 生命周期、`tools/call`、SSE 帧解析、错误映射），provider 只提供 tool 名与参数 schema。新增任何 MCP provider 应该是「声明 tools」而不是「再抄一遍协议」。

### 5. error_type 分类法无单一真相，下游用字符串集合硬判

**证据**：`error_type` 的产生点至少 4 处且规则不一致（见痛点 3）。消费点则把这些字符串**硬编码成集合**来决定 attempt 状态：

- `src/smart_search/service.py:2017`：判断 jina 失败是 `error` 还是 `empty`，硬列 `{"auth_error","config_error","parameter_error","quality_error","rate_limited","timeout","network_error","runtime_error"}`。
- `:2151`、`:2188`、`:2242`：docs/vertical fallback 各列一份**不同**的「算作 error 的 error_type 集合」。
- `credential_pool.py:30`：`ROTATABLE_ERROR_TYPES = {"rate_limited","quota_exhausted"}` 又是一份独立枚举。

任何新增 error_type 都需要人肉同步这些散落集合，极易漏改。

**设计启示（Rust）**：`enum ErrorKind`（`Auth`、`RateLimited`、`QuotaExhausted`、`Parameter`、`Timeout`、`Network`、`Quality`、`Runtime`…）作为唯一真相。「是否可重试」「是否触发凭据轮换」「算 error 还是 empty」都是 `ErrorKind` 上的方法或 `match`，编译器在新增变体时强制处理所有分支（穷尽性检查）。

### 6. 搜索结果 dict 契约在多处手工重复拼装

**证据**：结果对象是一个约 25 字段的裸 `dict`，在这些地方分别手工组装、字段集合并不完全一致：

- `_empty_search_result`（`src/smart_search/service.py:327-361`）；
- `search()` 成功返回（`:2845-2873`）；
- `research()` 返回（`:1496-1527`，字段名不同：`final_answer`/`citations`/`evidence_items`…）；
- `_primary_search_error_result` / `_primary_search_exception_result`（`:3388-3461`）。

`provider_attempts` 里每个 attempt 也是手工 dict（`_attempt`，`:364-385`），且 openai transport attempt 走另一条组装路径（`_append_openai_transport_attempts`，`:524-555`）。字段是否存在依赖调用点，消费者只能防御式 `.get()`。

**设计启示（Rust）**：`SearchOutcome`、`ProviderAttempt`、`Source` 用 `struct` + `serde` 定义一次，序列化形状由类型保证一致。空结果/错误结果是同一 `struct` 的构造函数，不可能字段错配。research 与 search 若形状不同，应是**不同的具名类型**而非「同一 dict 的方言」。

### 7. 配置是 1074 行手工维护的单例 God 对象

**证据**：`config.py`（`src/smart_search/config.py`）：

- `_CONFIG_KEYS` 手工列出约 90 个键（`:29-99`）；
- 每个 provider 有 4-6 个近乎重复的 property：`*_api_key`/`*_api_url`/`*_timeout`/`*_has_credentials`（如 exa `:751-763`、zhipu `:791-804`、jina `:836-898`）；
- `get_config_info()` 单函数约 170 行，手工把每个键映射进输出 dict（`:900-1067`）——新增一个键要同时改 `_CONFIG_KEYS`、property、`get_config_info`、`set_config_value`/`unset_config_value` 里的 `_cached_model` 失效名单（`:313-328`、`:340-354`）四处以上。
- 单例通过 `__new__` 实现（`:102-108`），带可变缓存 `_cached_model`/`_config_file`，测试必须手工重置（见痛点 9）。

**设计启示（Rust）**：配置用**声明式 struct + derive**（如 `serde` + 一个 config-derive 宏）从 env/文件加载，键名、默认值、掩码、来源追踪由 per-field 属性生成，而非四处手工同步。provider 配置应是 `HashMap<ProviderId, ProviderConfig>` 或每 provider 一个 `struct`，通过 registry 迭代产出 doctor/config 输出，消灭 170 行的手工映射。避免全局可变单例，用显式传入的 `&Config`（代码里 `IntentRouter(cfg)` 已经是好例子，但 `service.py` 到处直接引用模块级 `config` 单例）。

### 8. 两套并行的凭据管理体系

**证据**：

- Provider Credential Pool 只服务 6 个 allowlist provider：`PROVIDER_CREDENTIAL_KEYS = {exa, tavily, jina, firecrawl, context7, anysearch}`（`src/smart_search/credential_pool.py:21-28`），支持 `*_API_KEYS` JSON 数组 + round-robin + 限流轮换。
- 主 provider（xAI、openai-compatible）与 zhipu/zhipu-mcp **不在池内**，凭据从 config 裸属性取（`config.xai_api_key`、`config.zhipu_mcp_api_key`），`_provider_configured` 用一长串 `if provider == ...` 分别判断（`src/smart_search/service.py:643-668`）。
- 结果：`config.provider_has_credentials()` 对池内 provider 走池、对池外 provider 走别的分支（`config.py:849-863`），「一个 provider 是否已配置」这一简单问题有多条实现路径。
- `call_jina_reader` 里还有一条特例：空池时保留匿名 Reader（`service.py:2454-2487`），与 `_run_with_credential_pool` 的通用路径不同。

**设计启示（Rust）**：所有 provider 统一走同一 `CredentialSource`（单 key 是 pool size=1 的特例）。「是否已配置」「取下一个凭据」「限流轮换」是 provider registry 上的统一操作，不存在「池内/池外」二分。匿名可用性（如 Jina 无 key）作为 provider 元数据 `requires_key: bool` 声明，而非编排层特判。

### 9. 可测试性依赖大量 monkeypatch 与全局状态重置

**证据**：

- 测试总量 11818 行，接近源码规模；`test_service.py` 单文件出现 446 处 `mock/patch/monkeypatch`（`grep -rc` 结果）。这通常意味着被测单元与 IO/全局状态耦合过紧，难以纯函数化测试。
- 全局可变状态需要专门的 reset 钩子：模块级断路器字典 `_OPENAI_COMPATIBLE_MODEL_BREAKERS`（`service.py:309`）与 `_STREAM_BREAKERS`（`openai_compatible.py:17`）靠 `reset_runtime_breakers()`（`service.py:443-444`）、`reset_openai_compatible_breakers()`（`openai_compatible.py:75`）在测试间清理（`tests/test_service.py:1039` 等，`tests/test_openai_compatible_provider.py:64`）。
- config 单例缓存要手工失效：`conftest.py:17` 与 `tests/test_service.py:14` 都 `monkeypatch.setattr(config, "_cached_model", None)`；`tests/test_skill_preferences.py:295` 直接改 `config._config_file`。
- 模块级 `_AVAILABLE_MODELS_CACHE`（`service.py:52`）同样是进程内可变缓存。

**设计启示（Rust）**：断路器、模型缓存、config 应是**显式持有的状态**（传入 `&mut Breakers` / `Arc<...>`），而非模块级 `static mut`。这样测试构造隔离实例即可，无需全局 reset。纯逻辑（路由规则、来源解析、error 分类）应从 IO 中剥离成纯函数，直接单测，减少对 mock transport 的依赖。

### 10. 模块耦合双向且穿透私有面

**证据**：

- `service.py` 从 `intent_router` 导入 6 个下划线私有符号：`_classifier_can_add_capability`、`_cosine_similarity`、`_ordered_capabilities`、`_semantic_summary` 等（`src/smart_search/service.py:29-33`）。私有实现细节成了跨模块 API。
- provider 横向耦合：`xai_responses.py:9` 从 `openai_compatible` import `_WaitWithRetryAfter, _is_retryable_exception, get_local_time_info`（私有 + 拼写为「兄弟 provider 依赖另一个 provider 的私有件」）。
- `service.py` 顶部 import 了 8 个 provider 模块 + intent_router + intent_catalog + sources + utils，是所有东西的汇聚点（`:14-49`）。
- `sources.py` 反向 import `config`（`sources.py:10`）用于 `output_cleanup_enabled`，使一个「纯文本解析」模块也带上了全局配置依赖（`sources.py:144`）。

**设计启示（Rust）**：模块只暴露 `pub` 契约，私有 helper 不跨模块复用（要复用就上移到共享 crate）。避免「一个 provider 依赖另一个 provider 的内部」——共享件（retry wait、时间上下文、error 分类）属于 `http`/`common` 层。`sources` 这类纯解析逻辑不应依赖全局 config，清洗开关作为参数传入。

### 11. 新 provider 接入的样板成本高且无统一 registry

**证据**：以现状接入一个新 web_search provider，需要触碰（不完全列举）：

1. `providers/<new>.py`：自带 `_error_payload`/`_elapsed_ms`/retry/httpx client 全套样板（见痛点 3）。
2. `config.py`：`_CONFIG_KEYS` 加键、加 `*_api_key`/`*_api_url`/`*_timeout` property、`get_config_info` 加映射（三处）。
3. `credential_pool.py`：`PROVIDER_CREDENTIAL_KEYS` allowlist（若要多 key）。
4. `service.py`：`PROVIDER_PROFILES`（`:173-301`，每 provider 一段元数据）、`RESEARCH_PROFILE_ORDER`（`:164-172`）、`_provider_configured`（`:643-668` 加一个 `if`）、`get_capability_status`（`:1532-1594` 加一行）、对应能力的 fallback 函数（`_run_web_search_fallback` 等，`:2053-2113`）、可能的 inline `call_*`。
5. `cli.py`：`add_parser` + `set_defaults(command=...)`（现有 39 个 subparser，`:2906-3346`）。
6. doctor/smoke：`_test_<provider>_connection`（`:4173-4335`）、smoke mock/live 分支。

这些位置没有编译期关联，漏改任何一处都不会立刻报错。

**设计启示（Rust）**：建立**单一 provider registry**：每个 provider 实现 `trait Provider`（声明 id、capability、所需凭据键、配置 schema、连通性自检）。config 输出、capability status、fallback 链、doctor、CLI 帮助全部由 registry 迭代生成。新增 provider = 新增一个实现 + 注册一行，编译器保证所有派生面被覆盖。`PROVIDER_PROFILES`（现在是一大坨手写 dict 元数据）应成为 trait 的返回值/关联常量。

### 12. 路由逻辑分裂在 router 与 service 两地，规则用大堆关键词集合

**证据**：

- 意图路由的「权威」实现在 `intent_router.py`（rules + embeddings + classifier 三引擎融合，`:340-487`），但 `service.py` 又保留了一层 `_is_docs_intent`/`_is_zh_current_intent`/`_is_web_current_intent`/`_is_fetch_intent` 包装（`:903-916`），以及 research 专用的 `_research_route_signals`/`_research_capability_routes`（`:726-811`）。同一「意图」概念有多个入口。
- `build_deep_research_plan` 内嵌大量硬编码关键词集合做启发式：`DEEP_HIGH_COMPLEXITY_KEYWORDS`、`DEEP_RECENT_KEYWORDS`、`DEEP_CHINA_KEYWORDS`、`DEEP_EXA_DISCOVERY_KEYWORDS` 等（`service.py:80-163`），并在 `:1000-1012` 用中英混合关键词命中决定 recency/claim_risk/authority。规则以 `_contains_any` 子串匹配为主（`:919-921`），脆弱且难以标定。
- 语义引擎有一整套离线标定工具（F1/混淆矩阵/阈值搜索，`:3010-3386`）与预设推荐（`embedding_presets.py`），复杂度不低。

**设计启示（Rust）**：路由是一个**独立子系统**，只有一个入口 `route(query, ctx) -> Route`。rules 关键词表应为数据（可外置、可版本化、可标定），而非散落在 service 的模块级常量。research 与 search 共享同一 router 输出，差异体现在「如何消费 Route」，不是「各自再判一遍意图」。语义/分类器作为可选 stage，降级路径清晰（现有 `degraded_reason` 设计可保留）。

### 13. 编排层充斥 provider 专属特判（openai-compatible 尤甚）

**证据**：`search()` 主循环里对 `openai-compatible` 的特殊处理贯穿全程：model 候选展开（`_openai_model_candidates`，`:477-500`）、model 断路器 open 检查与跳过（`:2683-2697`）、transport attempts 回填（`_append_openai_transport_attempts`，`:2708-2712`、`:2747-2751`）、成功/失败时记录 breaker（`:2727-2731`、`:2752-2753`）、`_fallback_used` 里对 `provider == "OpenAI-compatible"` 的 identity 特判（`:613`）。xAI 与 openai-compatible 走两条不同的构造与记录路径（`_main_search_providers`，`:1882-1904`）。

**设计启示（Rust）**：把「model 级 fallback + 断路器 + transport fallback」封进 openai-compatible provider 自身（对编排层只暴露「给我一个答案或一个错误」），而不是让编排层知道每个 provider 的内部重试拓扑。`ProviderAttempt` 由 provider 自报，编排层只聚合。这样新 provider 的重试策略不会再泄漏进 `search()` 主循环。

---

## 交叉主题（贯穿多条）

- **未类型化 dict 是最深层的病根**：痛点 2/5/6/11 都源于「用 `dict[str, Any]` + `json.dumps` 当契约」。Rust 的类型系统本身就是对这一整类腐化的解药——这也正是 #52 选择重写的核心杠杆点。
- **「单一真相源」缺失**：error 分类、retryable 集合、provider 元数据、配置键各有 2-4 份副本。新架构应让每类事实**只定义一次**并派生其余用途。
- **全局可变状态**：单例 config、模块级断路器/缓存字典，既是耦合源也是测试负担。新架构用显式状态所有权替代。

## 对 Rust 新架构的优先级建议

1. **先定契约类型**：`Provider` trait、`ProviderResponse`/`SearchOutcome`/`ProviderAttempt`/`Source`/`ErrorKind` 强类型（对应痛点 2/5/6）。
2. **共享 transport 层**：http client + retry policy + MCP client + SSE 解析统一（痛点 3/4）。
3. **provider registry 驱动一切派生面**：config 输出、capability status、fallback、doctor、CLI（痛点 7/8/11）。
4. **编排层瘦身、provider 自包含重试**：把 openai-compatible 特判收回 provider 内部（痛点 1/13）。
5. **路由单入口 + 规则数据化**（痛点 12）。
6. **显式状态、纯函数化**：断路器/缓存/config 显式持有，纯逻辑可无 mock 单测（痛点 9/10）。

## 来源索引

### 核心模块（行号证据）

- God 模块与编排：`src/smart_search/service.py:1-49`（import 汇聚）、`:307-500`（断路器/model 候选）、`:2542-2873`（`search()`）、`:991-1264`（deep plan）、`:1267-1529`（`research()`）、`:2282-2540`（内联 Tavily/Firecrawl）、`:3035-3386`（路由标定）。
- Provider 抽象：`src/smart_search/providers/base.py:5-41`。
- 样板复制/漂移：`src/smart_search/providers/exa.py:14-63`、`src/smart_search/providers/jina.py:16-51`、`src/smart_search/providers/zhipu.py:13-52`、`src/smart_search/providers/zhipu_mcp.py:9-64`、`src/smart_search/providers/openai_compatible.py:16-113`、`src/smart_search/providers/xai_responses.py:9-42`、`src/smart_search/providers/context7.py:15-337`。
- error_type 散落：`src/smart_search/service.py:429-436`、`:2017`、`:2151`、`:2188`、`:2242`、`src/smart_search/credential_pool.py:30`。
- 结果 dict 契约：`src/smart_search/service.py:327-385`、`:2845-2873`、`:1496-1527`、`:3388-3461`。
- 配置 God 对象：`src/smart_search/config.py:29-108`、`:751-898`、`:900-1067`。
- 双套凭据：`src/smart_search/credential_pool.py:21-28`、`src/smart_search/service.py:643-668`、`:2454-2487`、`src/smart_search/config.py:843-874`。
- 耦合/私有面导入：`src/smart_search/service.py:14-49`、`src/smart_search/providers/xai_responses.py:9`、`src/smart_search/sources.py:10,144`。
- provider 元数据登记面：`src/smart_search/service.py:164-301`、`:643-679`、`:1532-1594`；`src/smart_search/cli.py:2906-3346`；`src/smart_search/service.py:4173-4335`（doctor 自检）。
- 路由分裂：`src/smart_search/intent_router.py:340-487`、`src/smart_search/service.py:80-163`、`:726-811`、`:903-921`。

### 测试可测性

- `tests/test_service.py`（446 mock/patch）、`tests/conftest.py:17`、`tests/test_service.py:14,1039`、`tests/test_openai_compatible_provider.py:64`、`tests/test_skill_preferences.py:295`。

### 相关一手记录

- [Issue #55：现有架构痛点梳理](https://github.com/jfmoe/smartsearch/issues/55)（本清单对应工单）。
- Issue #52：Rust 重写决策（本清单为其设计输入；本调查未修改 #52）。
- 现有 ADR：`docs/adr/0005-provider-credential-pool.md`（凭据池边界）、`docs/adr/0003`~`0004`（caller capability 契约）。

## 局限性

- 本调查以静态通读为主，未运行 profiling，也未逐一执行测试用例；「难测试」的判断基于 mock 密度与全局状态而非覆盖率数据。
- 行号基于 `main @ 11ac647`，后续提交可能漂移。
- 「设计启示」是面向 Rust 的建议方向，非最终架构决策；具体 crate 边界、trait 形状仍需在 #52 的架构设计中定稿。
- 未穷尽 `cli.py`（3387 行）、`skill_installer.py`/`skill_sync.py`、`result_journal.py`、`file_lock.py` 的细节；这些模块的样板问题与上述同类，未单列以控制篇幅。
