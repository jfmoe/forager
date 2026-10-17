# X 平台经 xAI x_search 接入，条目是转述内容

forager 的第五个平台 `x`（X，原 Twitter）只有一条 route `xai`。它复用主搜索的 xAI provider（`providers.xai` 的 `url`、`keys` 与 `model`），以 Responses API 的 `x_search` 工具读取 X，提供 search、fetch 与 Platform Operation Ask。

## 背景

`x_search` 包含关键词检索、语义检索、用户检索与 thread 读取四个服务端工具，由模型自行决定调用哪个、调用几次。xAI API 只返回工具调用的参数，不返回工具读到的帖子（xAI 文档 Tool Usage Details，2026-10-17）；forager 能拿到的只有模型写出的文本与 `url_citation` 注释。现有平台的条目都来自 forager 直接读到的平台数据，X 是第一个条目由模型转述的平台。

## 决定

- **接受 Relayed Content 作为平台条目，但身份与时间不依赖模型。**
  - search 用 json_schema 结构化输出取得帖子列表。结构化输出时，annotations 列出本次读到的全部来源；帖子 id 不在其中的条目被丢弃。2026-10-17 经 SuperGrok 订阅提供 Responses API 的网关实测：模型返回的帖子都在 annotations 中。
  - 发布时间由帖子 id（snowflake）推算，不取模型给出的时间。同日 3 条样本的推算值与模型给出值精确到秒一致。
  - fetch 读取 thread，根帖须与请求一致并出现在 annotations 中。实测回复的 id 不出现在 annotations 中，无法核对，平台规范写明这一点。
  - 正文与作者 handle 不保证逐字，search 条目深度为 `snippet`。
- **只暴露服务端强制执行的参数**：账号白名单或黑名单、日期区间。排序由模型自选，服务端不强制，因此不提供。
- **回答形态是独立的 Platform Operation Ask，不是 search 的字段或开关。** 结构化输出时回答失去逐句引用（实测 annotations 位置全为 0）。Ask 不用结构化输出，保留 `[[N]](url)` 引用，并沿用主搜索的来源规范化；引用的帖子映射为 Platform Ref，可以接着 fetch。
- **route 按供应商命名为 `xai`，复用其配置节**，与 SerpApi 相同：同一凭据池同时服务主搜索与平台。平台请求只发 `x_search`，不受 `providers.xai.tools` 影响。

## 不选的方案

- **只做回答命令**：拿不到可以继续 fetch 的帖子身份，也不能列出某个账号的帖子。
- **先做 OpenCLI 浏览器 route**：能读到逐字正文与互动数，但需要登录 X，并承担风控与封号风险。它以后可以作为同一平台的第二条 route 加入，不推翻本决定。
- **在主搜索中新增 X 检索能力**：主搜索默认已启用 `x_search` 工具，新增 capability 只会重复。

## 影响

- skill 的平台 reference 须写明 X 条目的正文是转述；需要逐字引用时，按条目 URL 打开原帖。
- 每次请求读取多少帖子由模型决定，`max_turns` 实测不能限制调用次数：一次 search 发生 16 次工具调用、读取 52 条帖子。经 xAI 官方 API 时，按读取的帖子数与用户数计费。
- 接入清单 R4 要求 HTTP route 有 `timeout` 配置项。`xai` 同时是主搜索 provider，没有这个配置项；平台请求与主搜索一样使用命令的总时限。
