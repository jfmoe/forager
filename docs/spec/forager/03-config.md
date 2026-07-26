# 3. 配置

权威来源：[#57 Resolution](https://github.com/jfmoe/smartsearch/issues/57) 及其[补充决议](https://github.com/jfmoe/smartsearch/issues/57#issuecomment-5078205945)。

## 格式与位置

- **TOML**，`~/.config/forager/config.toml`（尊重 `XDG_CONFIG_HOME`），全平台统一。
- `FORAGER_CONFIG_DIR` 为唯一目录覆盖（引导变量，从 Env provider **显式豁免**，先于配置装载读取）。砍：Windows LOCALAPPDATA 特例、旧目录迁移探测、cwd fallback（默认目录不可写＝报错并提示设 `FORAGER_CONFIG_DIR`）。
- 权限契约：config 目录 `0700`，`config.toml` 与临时文件 `0600`（原子替换后重申）。

## 分层与 env 映射

- **env > file > default** 维持。
- 映射：`FORAGER_` 前缀 + `__` 层级分隔，如 `providers.openai_compatible.stream` → `FORAGER_PROVIDERS__OPENAI_COMPATIBLE__STREAM`。
- **裸 provider 原生 env 一律不认**（`EXA_API_KEY` 等不再读取）。
- **未知 `FORAGER_*` env ＝ config_error 退 3**（与文件层严格 schema 对称）。
- env 值按 schema 目标类型解析（字符串字段不猜 TOML 类型）；数组值用 TOML 数组字面量：`FORAGER_PROVIDERS__EXA__KEYS='["sk-a","sk-b"]'`。

## 全键面定稿

```toml
# 除 keys 外全部可省略——省略即内置默认
[search]
backends = ["xai", "openai_compatible"]
validation = "balanced"      # fast|balanced|strict
fallback = "auto"            # auto|off

[classifier]
url = ""
keys = []
model = ""
fallback_models = []         # 同端点模型链（同 url/keys 换模型）
timeout = 30                 # 整个分类阶段的共享预算

[providers.xai]
url = "https://api.x.ai/v1"
keys = []
model = "grok-4-fast"
tools = ["web_search", "x_search"]

[providers.openai_compatible]
url = ""
keys = []
model = "grok-4-fast"        # openrouter 自动 :online 后缀＝引擎规则
fallback_models = []
stream = false               # 幸存的唯一传输持久开关

[providers.exa]
url = "https://api.exa.ai"
keys = []
timeout = 30

[providers.context7]
url = "https://mcp.context7.com/mcp"
keys = []
timeout = 30

[providers.jina]
url = "https://r.jina.ai"
keys = []
respond_with = ""            # X-Respond-With（如 readerlm-v2）
timeout = 30

[providers.tavily]
url = "https://api.tavily.com"
keys = []
timeout = 30

[providers.firecrawl]
url = "https://api.firecrawl.dev/v2"
keys = []
timeout = 30                 # 补齐——旧版缺失该键

[providers.anysearch]
url = "https://api.anysearch.com/mcp"
keys = []
timeout = 30

[capabilities.web_search]
order = ["tavily", "firecrawl"]
[capabilities.web_fetch]
order = ["jina", "tavily", "firecrawl"]   # 置空＝config_error（引擎不变量）
[capabilities.docs_search]
order = ["context7", "exa"]
[capabilities.vertical_search]
order = ["anysearch"]

[log]
level = "info"               # 仅 stderr 控制台流；debug＝旧 DEBUG 的 verbose

[journal]
enabled = true               # 默认翻转为开
dir = "~/.local/state/forager/journal"   # $XDG_STATE_HOME/forager/journal
retention_days = 30          # 0＝无限期

[retry]
max_attempts = 3
multiplier = 1
max_wait = 10

[http]
ssl_verify = true
```

命名规则：小写 snake；节承担命名空间；endpoint 字段统一 `url`；`timeout` 裸秒数。`[research]` 节不存在。

### 凭据形状

唯一形状：每 provider 节一个 `keys` **真数组**（单凭据＝单元素数组）。`*_API_KEY`/`*_API_KEYS` 双形态与「KEYS 覆盖 KEY」优先级消灭。`classifier.keys` 沿用凭据池全套语义（去空去重、轮询、配额/限流失败同请求内换用）。

### 链序权威

`[capabilities.<seam>].order`＝**完全权威**：列表即该 seam 全部可用面与顺序，禁用＝从表删除，无合并规则；未配置凭据者运行时自然跳过。`RESEARCH_PREFERRED/DISABLED_PROVIDERS`、`TAVILY_ENABLED` 消灭。条件变序废除，JS-heavy 场景由薄正文质量门控继任（第 4 章）。变序若回归必须走「具名 request class + `<class>_order` 键」机制（v1 具名类为空；写 `order` 未写变体键→继承）。校验：order 引用不支持该 seam 的 provider＝config_error（支持矩阵＝registry 编译期事实）。

### 值域与交叉约束（进 schema 与验收）

`search.backends` 非空、去重、限 `{xai, openai_compatible}`；全部 backend 无凭据＝退 3；`providers.xai.tools` 限 `{web_search, x_search}`；所有 `timeout > 0`；`retry.max_attempts >= 1`、`multiplier > 0`、`max_wait >= 0`；`journal.retention_days >= 0` 且 0＝无限期。文件层严格 schema：未知键＝退 3、报错点名坏键。

## `config` 命令

1. 点路径寻址，**schema 即键清单**：`forager config set providers.exa.timeout 45`；非法路径＝退 2；set 时校验类型/enum；数组值＝TOML 字面量。
2. `config set KEY -`＝stdin 读值（敏感值通道）：读完整 stdin、去恰一个末尾 LF/CRLF、空输入为合法空字符串、按目标类型解析。argv 通道保留，文档警示 shell history。
3. 写入＝toml_edit 精确编辑 + 原子替换（保留未修改项注释/空白/相对顺序；不承诺 dotted-key 原始顺序）；不做跨进程锁；并发丢更新＝已接受风险。
4. `config list`＝**生效视图**：每键 `{值（凭据掩码、数组逐元素掩码）, 来源 env/file/default}`；doctor 内嵌同一结构块（同一序列化器）。
5. `config unset` 只删文件层；被 env 覆盖时提示「env 仍在生效」。
6. **坏配置修复通道**：`path` 永不加载 schema；`set`/`unset` 走 toml_edit 文档层编辑、仅校验目标键值；`list` 在提取失败时仍报文件路径、坏键与行列。

## `setup` 向导

交互四步：①语言 → ②主模型（选 backend，回车取默认）→ ③分类器（可跳过，明示后果）→ ④补强 provider ×6 逐个贴 keys 或跳过。写盘后提示跑 `forager doctor`（setup 不做连通性探测）。只问凭据与模型级键；行为键出口是 `config set` 与手编。二跑＝增量更新。`--non-interactive`＝生成全键注释模板（table 形式，凭据留空），目标已存在拒绝覆盖。

## 旧键→新键映射表（迁移的唯一交付物）

零迁移代码（无 `config import`、setup 不探测旧文件）；手抄凭据一次完成；forager 对 `~/.config/smart-search/` 零感知。覆盖 `config.py` `_CONFIG_KEYS` 全集 + 两个体系外键：

| 旧键 | 新去向 |
|---|---|
| `XAI_API_URL` | `providers.xai.url` |
| `XAI_API_KEY` | `providers.xai.keys`（单元素数组） |
| `XAI_MODEL` | `providers.xai.model` |
| `XAI_TOOLS` | `providers.xai.tools` |
| `OPENAI_COMPATIBLE_API_URL` | `providers.openai_compatible.url` |
| `OPENAI_COMPATIBLE_API_KEY` | `providers.openai_compatible.keys`（单元素数组） |
| `OPENAI_COMPATIBLE_MODEL` | `providers.openai_compatible.model` |
| `OPENAI_COMPATIBLE_FALLBACK_MODELS` | `providers.openai_compatible.fallback_models` |
| `OPENAI_COMPATIBLE_STREAM` | `providers.openai_compatible.stream` |
| `SMART_SEARCH_VALIDATION_LEVEL` | `search.validation` |
| `SMART_SEARCH_FALLBACK_MODE` | `search.fallback` |
| `SMART_SEARCH_MINIMUM_PROFILE` | **已删除**（capability 缺口自报继任，见第 1 章修订留痕） |
| `SMART_SEARCH_RESEARCH_PREFERRED_PROVIDERS` | **已删除**（`capabilities.*.order` 完全权威） |
| `SMART_SEARCH_RESEARCH_DISABLED_PROVIDERS` | **已删除**（同上） |
| `SMART_SEARCH_INTENT_ROUTER` | **已删除**（三层路由砍） |
| `INTENT_EMBEDDING_API_URL` | **已删除**（embeddings 层砍） |
| `INTENT_EMBEDDING_API_KEY` | **已删除** |
| `INTENT_EMBEDDING_MODEL` | **已删除** |
| `INTENT_EMBEDDING_THRESHOLD` | **已删除** |
| `INTENT_EMBEDDING_MARGIN` | **已删除** |
| `INTENT_CLASSIFIER_API_URL` | `classifier.url` |
| `INTENT_CLASSIFIER_API_KEY` | `classifier.keys`（单元素数组） |
| `INTENT_CLASSIFIER_MODEL` | `classifier.model` |
| `INTENT_ROUTER_TIMEOUT_SECONDS` | **已删除**（`classifier.timeout` 为新语义键：阶段共享预算，不做值迁移） |
| `EXA_API_KEY` / `EXA_API_KEYS` | `providers.exa.keys` |
| `EXA_BASE_URL` | `providers.exa.url` |
| `EXA_TIMEOUT_SECONDS` | `providers.exa.timeout` |
| `CONTEXT7_API_KEY` / `CONTEXT7_API_KEYS` | `providers.context7.keys` |
| `CONTEXT7_MCP_API_URL` | `providers.context7.url` |
| `CONTEXT7_TIMEOUT_SECONDS` | `providers.context7.timeout` |
| `CONTEXT7_BASE_URL`（体系外遗留） | **已删除**（旧迁移探测键，不再识别） |
| `ZHIPU_API_KEY` | **已删除**（Zhipu 全家砍） |
| `ZHIPU_API_URL` | **已删除** |
| `ZHIPU_SEARCH_ENGINE` | **已删除** |
| `ZHIPU_TIMEOUT_SECONDS` | **已删除** |
| `ZHIPU_MCP_API_KEY` | **已删除** |
| `ZHIPU_MCP_SEARCH_API_URL` | **已删除** |
| `ZHIPU_MCP_READER_API_URL` | **已删除** |
| `ZHIPU_MCP_ZREAD_API_URL` | **已删除** |
| `ZHIPU_MCP_TIMEOUT_SECONDS` | **已删除** |
| `JINA_API_KEY` / `JINA_API_KEYS` | `providers.jina.keys` |
| `JINA_READER_API_URL` | `providers.jina.url` |
| `JINA_RESPOND_WITH` | `providers.jina.respond_with` |
| `JINA_TIMEOUT_SECONDS` | `providers.jina.timeout` |
| `TAVILY_API_KEY` / `TAVILY_API_KEYS` | `providers.tavily.keys` |
| `TAVILY_API_URL` | `providers.tavily.url` |
| `TAVILY_ENABLED` | **已删除**（禁用＝从 `order` 删除） |
| `TAVILY_TIMEOUT_SECONDS` | `providers.tavily.timeout` |
| `FIRECRAWL_API_KEY` / `FIRECRAWL_API_KEYS` | `providers.firecrawl.keys` |
| `FIRECRAWL_API_URL` | `providers.firecrawl.url` |
| `ANYSEARCH_API_KEY` / `ANYSEARCH_API_KEYS` | `providers.anysearch.keys` |
| `ANYSEARCH_API_URL` | `providers.anysearch.url` |
| `ANYSEARCH_TIMEOUT_SECONDS` | `providers.anysearch.timeout` |
| `SMART_SEARCH_DEBUG` | **已删除**（`log.level = "debug"` 继任） |
| `SMART_SEARCH_LOG_LEVEL` | `log.level` |
| `SMART_SEARCH_LOG_DIR` | **已删除**（file logging 砍，持久观测归 journal） |
| `SMART_SEARCH_LOG_TO_FILE` | **已删除**（同上） |
| `SMART_SEARCH_RETRY_MAX_ATTEMPTS` | `retry.max_attempts` |
| `SMART_SEARCH_RETRY_MULTIPLIER` | `retry.multiplier` |
| `SMART_SEARCH_RETRY_MAX_WAIT` | `retry.max_wait` |
| `SMART_SEARCH_OUTPUT_CLEANUP` | **已删除**（正文消毒无条件执行） |
| `SMART_SEARCH_RESULT_JOURNAL_ENABLED` | `journal.enabled`（默认由关翻转为**开**） |
| `SMART_SEARCH_RESULT_JOURNAL_RETENTION_DAYS` | `journal.retention_days` |
| `SSL_VERIFY` | `http.ssl_verify` |
| `SMART_SEARCH_CONFIG_DIR`（体系外引导变量） | `FORAGER_CONFIG_DIR` |
