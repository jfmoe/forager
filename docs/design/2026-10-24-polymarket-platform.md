# Polymarket 平台与 `polymarket_api` route 设计

状态：search、fetch 与 history 已实现并写入 spec（第 2、3、4、5、7 章）；决策记录见 [ADR 0032](../adr/0032-polymarket-platform-prices-from-two-sided-order-books.md)。日期：2026-10-24。外部事实与证据见 [Polymarket 平台接入调研](../research/2026-10-24-polymarket-integration.md)（下称「调研」），本文引用其中已核实的事实，不重复证据。

## 目标与边界

为 forager 新增第八个平台 `polymarket`，让调用方按主题找到预测市场的事件，并取回每个结果当前的市场隐含概率及其历史走势。

- **要解决的问题**：Web 搜索只能找到转述的赔率与新闻稿，数值滞后、来源不明。Polymarket 有官方匿名 API，可以直接拿到订单簿上的买卖价，并能判断这个价格是否可以当概率读。
- **做**：search（按主题找 event）、fetch（一个 event 或 market 的当前概率与结算状态）、L2 操作 history（一个 market 的价格历史），第一期全部交付。
- **消费方**：agent 经 forager skill 的即席查询；程序化消费方（例如单用户的 fire）直接调用平台命令、读取 JSON，不另设接口。输出因此带稳定的链上身份与取值时刻，见「fetch」。
- **不做**：下单、钱包与任何需要 CLOB 凭据的接口；WebSocket；抓取 polymarket.com 页面（ToS §4.2 禁止）；评论、持有人、成交明细；批量 fetch（多个市场按 ref 逐个调用，节奏由访问策略控制）；Polymarket US（CFTC 监管的独立产品，身份空间不同，需要时作为独立平台另行设计）；进入 research 证据管线；查询扩展与本地相关性过滤（判断相关性、换词重搜归调用方）。

## 关键判断

### 概率取订单簿 midpoint，但只在两边都有挂单时才算

调研确认了三件事：

1. 官方把价格定义为隐含概率，polymarket.com 显示的是 midpoint。文档说价差超过 $0.10 时改显示最近成交价，但唯一的页面样本仍显示 midpoint。
2. Gamma 的 `outcomePrices` 就是 Gamma 自己 bid/ask 的中点，经 CDN 缓存最长 300 秒。
3. 订单簿为空时，CLOB `/midpoint` 与 Gamma `outcomePrices` 都给出 `0.5`。这个 0.5 是占位值，不是市场判断。

所以 forager 的规则是：只有一个 outcome 的买方与卖方都有挂单时，才把 `(best_bid + best_ask) / 2` 作为它的概率；否则概率为 `null`，并写明原因。不采用「价差大于 0.10 改用最近成交价」：文档与实测矛盾，而且最近成交可能是几天前的。forager 同时交付 `best_bid`、`best_ask`、`spread` 与 `last_trade_price`，由调用方判断宽价差市场的可信度。

### 一条 route 访问三个官方端点

沿用 ADR 0027 的多端点 route 规则：route `polymarket_api` 匿名访问 Gamma（发现与元数据，主端点）、CLOB（实时买卖价与最近成交）和 Data API v2（价格历史）。拆成多条 route 不可行，因为链语义只取第一条被接受的 route 的结果、不合并：fetch 必须把 Gamma 的结构和 CLOB 的实时价格组合在一起。

CLOB 只用来增强结果。CLOB 请求失败时，fetch 保留 Gamma 的数据，并按同一条两边挂单规则改用 Gamma 缓存的 bid/ask，`price_source` 标为 `gamma`，`last_trade_price` 为 `null`，汇总一条诊断，不判失败。history 只有 Data API 能提供，它失败即命令失败。

### ref 用 slug，不用数字 id

polymarket.com 的 URL 只含 slug，数字 id 没有对应页面。用 slug 作 ref 能与 canonical URL 往返，也能直接解析用户贴来的链接。代价是 slug 可能变化：Gamma schema 有无说明的 `pastSlugs` 字段，调研没有找到 slug 是否会变的文档。数字 `event_id`、`market_id` 与 `condition_id` 作为输出字段交付，供调用方核对。

### 默认启用

`polymarket_api` 进入默认 order。它只调用 Polymarket 官方文档化、明确不需要 key 的公开读接口，不读页面，访问节奏远低于官方限额。条款相关的两点写进 skill 的平台 reference，不在 route 层拦截：

- ToS §4.2 禁止用 robots 或 crawlers 抓取站点；
- Capital Market Client 与市场数据分发商未经书面同意，不得经 API 使用 Data。

后一条取决于用户身份：个人研究不属于这两类，机构用途需要用户自己取得同意。

备选方案是像 `youtube_web` 一样由用户手动启用。它的好处是把条款判断明确交给用户，代价是第一次使用要多一步配置。当前接口是官方为开发者公开的，所以本提案不选它。

## 平台 `polymarket`

### 身份

kind 有两种，都没有版本。slug 是 Gamma 返回的原文，由 ASCII 字母、数字、`-` 与 `_` 组成，区分大小写，长度 1–255，全为数字的输入是数字 id，不是 slug。规则来自 2026-10-24 的抽样，见「实测结果」；少数 slug 含 `.`，但 Gamma 自己的按 slug 查询拒绝它们，所以不接受。

| kind | ref | canonical URL |
|---|---|---|
| `event`（fetch 的默认 kind） | `polymarket:<event_slug>` | `https://polymarket.com/event/<event_slug>` |
| `market` | `polymarket:market/<market_slug>` | `https://polymarket.com/market/<market_slug>` |

- `/market/<slug>` 会 307 跳到所属 event 下的 market 页，但它本身就能定位这个 market，所以作为 canonical URL，满足往返性质。
- 可识别的 URL：主机为 `polymarket.com` 或 `www.polymarket.com`（http 或 https，主机不区分大小写），路径为以下三种之一：
  - `/event/<e>` → event；
  - `/event/<e>/<m>` → market `<m>`，丢弃 event 段，market slug 本身唯一；
  - `/market/<m>` → market。
- 路径前可以有一个语言前缀（`/zh/`、`/zh-hant/` 等，按调研列出的集合）；忽略 query（如 `?tid=`）、fragment 与末尾 `/`。
- 飞行前退 2 的输入：
  - `polymarket.us` 与 `gateway.polymarket.us`：消息说明这是另一个产品；
  - 数字 id、`/sports/...`、`/profile/...` 等其他路径，以及相似域名。
- ref 解析在 types 叶子 `platform_polymarket` 中，是零 IO 纯函数。

### Content Depth

- search 条目恒为 `metadata`。
- fetch 支持 `metadata`（默认）与 `abstract`。
  - `metadata` 的书目信息就是 event 与每个 market 的状态和概率。
  - `abstract` 另外内联结算规则：event 的 `description`，以及与 event 规则不同的 market 规则。实测 neg-risk event 的 13 个 market 规则逐字相同，去重能避免重复十几遍。规则决定概率衡量的到底是什么，调用方在解读概率前应读它。
  - `full_text` 与 `thread` 由支持检查拒绝，飞行前退 2。

### 状态与价格的派生规则

这是本平台唯一需要独立测试的业务规则，放在 types 叶子中，写成零 IO 纯函数，输入是 Gamma market 加可选的 CLOB 报价。

market `status`：

| 条件 | `status` |
|---|---|
| `closed` 且 `umaResolutionStatus` 为 `resolved` | `resolved` |
| `closed`，未结算 | `closed`（等待结算或争议中） |
| 未 `closed`，`active` 不为 false，`acceptingOrders` 为 true | `open` |
| 未 `closed`，`active` 为 false 或 `acceptingOrders` 为 false | `suspended` |

`active` 只对未 `closed` 的 market 读取：已结算市场的 `active` 仍为 true，不能用它判断是否还在交易。未 `closed` 而 `active` 为 false 的是 neg-risk 组的占位 market（见「实测结果」）：它们 `acceptingOrders` 为 true，但没有 `outcomePrices`，CLOB 也没有订单簿，所以为 `suspended`，不请求 CLOB。Gamma 没有给出 `active` 时按 true 处理。

每个 outcome 的 `price` 与 `basis`：

| 情形 | `price` | `basis` |
|---|---|---|
| `resolved` | `outcomePrices` 中该 outcome 的兑付值（1、0，或争议结果的 0.5） | `resolution` |
| `open`，CLOB 报价两边都有挂单 | `(best_bid + best_ask) / 2` | `midpoint` |
| `open`，CLOB 失败，Gamma `bestBid` 与 `bestAsk` 都存在（只适用于第一个 outcome，第二个取 1 减去它） | Gamma `outcomePrices` | `midpoint_cached` |
| `open`，一边或两边没有挂单 | `null` | `no_two_sided_book` |
| `closed` 或 `suspended` | `null` | `not_trading` |

- **挂单判定**：CLOB `POST /prices` 对没有买单的一侧返回 `"0"`、没有卖单的一侧返回 `"1"`。有效价格落在 `[tick, 1 − tick]` 内，所以 0 与 1 只表示没有挂单。这是推断，见「仍未实测的部分」。
- **不采用残留值**：已结算 market 的 Gamma `bestBid`、`bestAsk` 与 `lastTradePrice` 是残留值，从不当作结果。
- **market `probability`**：`outcomes` 恰为 `["Yes", "No"]` 时，是 Yes 的 `price`；其他 outcome 名（体育市场的队名等）时为 `null`，调用方读 `outcomes`。
- **`outcomes` 条目**：`{name, price, basis, best_bid, best_ask}`。market 另有 `spread`（Yes 一侧）、`last_trade_price`（只取 CLOB `last-trades-prices`，不取 `/books` 的同名字段：调研实测它在 neg-risk market 上给出互补价；CLOB 不可用时为 `null`，不改用 Gamma 缓存的 `lastTradePrice`）、`one_day_change`、`one_week_change` 与 `one_month_change`（Gamma 的 `oneDayPriceChange`、`oneWeekPriceChange`、`oneMonthPriceChange`，可能滞后），以及 `prices_as_of`（CLOB 请求完成时的本机 UTC 时间，`gamma` 来源时为 Gamma 的 `updatedAt`）。
- **neg-risk event**：各 market 的 `probability` 之和可以不等于 1（实测 0.988）。forager 不归一化，event 带 `neg_risk: true`，由 skill reference 说明。

## Route `polymarket_api`

- **传输与凭据**：`Http`，`Credentials::Unused`，经 `execute_anonymous` 执行。`native_full_text: false`，因为 fetch 不提供 `full_text`，计划从不需要 Web Fetch 预检。
- **配置**：

  | 配置键 | 用途 | 默认值 |
  |---|---|---|
  | `providers.polymarket_api.url` | Gamma | `https://gamma-api.polymarket.com` |
  | `.clob_url` | CLOB | `https://clob.polymarket.com` |
  | `.data_url` | Data API v2，路径带 `/v2` | `https://data-api.polymarket.com` |
  | `.timeout` | 每个请求 | 30 秒 |

  `platforms.polymarket.order` 默认 `[polymarket_api]`。
- **访问策略**：每 0.25 秒 1 个请求、并发 1，三个端点共用，跨进程生效。这个节奏合每 10 秒 40 次，是官方最严的相关限额（Gamma `/public-search` 每 10 秒 350 次）的约九分之一。官方说限额按 IP，超限时排队或返回 429，两种都要处理。
- **重试**：沿用共享重试策略。Data API 的 `503 request_timeout` 与带 `Retry-After` 的 429 交给共享 status 映射，429 为 RateLimited，匿名 route 无凭据可换，直接成为终态。
- **请求数**：每个操作是常数。
  - search 1 次；
  - fetch 3 次：Gamma、CLOB `/prices`、CLOB `/last-trades-prices`；
  - history 2 次：Gamma 取 token、Data API 取历史；序列超过一页时另有至多 4 次续页。

  例外是 outcome 多于 250 个的 fetch：CLOB 批量请求按每批 500 项切分（`/prices` 一批是 250 个 token 的买卖两侧，`/last-trades-prices` 一批是 500 个 token），详见「fetch」。

## search

`forager platform polymarket search QUERY [--limit N] [--include-closed] [--cursor CURSOR]`

- **请求**：`GET <url>/public-search?q=<查询词>&limit_per_type=<limit>&page=<n>&search_tags=false&search_profiles=false`。
  - 默认加 `events_status=active`，`--include-closed` 时不发送；
  - 不发送 `optimized=true`，它会去掉 `conditionId` 与 `clobTokenIds`；
  - 不提供排序：`sort` 实测无效，官方没有说明排序规则。
- **查询词**：必填，去空白后为空时飞行前退 2；原样发送，不承诺任何运算符。
- **limit 与分页**：`--limit` 为 1–20，默认 10。实测 `page` 从 1 开始，页大小就是 `limit_per_type`。
  - 页位置是 `page`，cursor 为 `v1.polymarket_api.<payload>`，恢复查询词、`include_closed`、limit 与下一页的 `page`；
  - `pagination.hasMore` 为 true 且下一页不超过第 100 页时签发 cursor：Gamma 以 422 拒绝 `page` 大于 100，支持检查在飞行前拒绝更深的 cursor；
  - 排序没有承诺跨时间稳定，批量使用方按 ref 去重。
- **合法空集**：只有 `pagination` 而没有 `events` 键时为 `items: []`，退 0。响应缺少 `pagination` 为 Runtime。
- **条目**：每个 event 一条。
  - 公共字段：`ref`、`url`、`depth: metadata`、`title`、`authors: []`（市场没有作者）、`published`（`startDate`，精确到秒的 UTC）。
  - 平台字段：`event_id`、`status`（event 的所有 market 都已结算为 `resolved`，任一 market 为 `open` 为 `open`，其余为 `closed`）、`end_date`、`volume`、`volume_24h`、`liquidity`、`neg_risk`、`market_count`。
  - `top_markets`：按 `probability` 降序的前 5 个 market，概率为 `null` 的排在后面，每项为 `{ref, label, probability, basis, status}`，`label` 取 `groupItemTitle`，没有时取 `question`。
  - `price_source: "gamma"`：search 只用 Gamma 自带的价格，按「状态与价格的派生规则」的 `midpoint_cached` 与 `resolution` 两行计算，不请求 CLOB。要拿实时概率，就对感兴趣的 event 运行 fetch。
- **不合法的条目**：无法定位的条目被跳过，stderr 汇总一条诊断：event 或它的某个 market 的 slug 不合法，或缺少 `markets`。少数旧 market slug 含 `.`，Gamma 自己的 slug 查询也拒绝它们，所以带这种 market 的 event 无法完整定位，但不说明响应有缺陷。其他解码失败与 fetch 相同，为 Runtime，整页失败。上游有 event 而全部被跳过为 Runtime。

## fetch

`forager platform polymarket fetch REF|URL [--depth metadata|abstract]`

- **元数据**：event 请求 `GET <url>/events/slug/<slug>`，market 请求 `GET <url>/markets/slug/<slug>`。
  - market 的响应内嵌所属 `events`，输出带 `event: {ref, url, title}`；
  - 404 `not found error` 为 attempt 级 Parameter，消息为 `Polymarket event not found: polymarket:<slug>; slugs can change, search again`（market 同理）；
  - 返回的 `slug` 与请求不一致为 Runtime；
  - JSON 编码字符串字段（`outcomes`、`outcomePrices`、`clobTokenIds`）解码失败，或 `outcomes` 与 token 数不一致，为 Runtime。market 有 token 时，不论状态都核对数量；只有 `open` 的 market 必须有 token，因为部分早年已关闭或已结算的 market 没有 `clobTokenIds`（见「实测结果」）。
- **token 选择**：`version` 为 `v2` 时取 `positionIds`，否则取 `clobTokenIds` 解码后的数组。实测 V2 样本两者相同，这条规则遵照官方文档，不依赖两者相同。
- **实时报价**：只为 `status` 为 `open` 的 market 请求。
  - `POST <clob_url>/prices`：每个 token 的 BUY 与 SELL 两项，每批最多 500 项（实测 501 项返回 400 `Payload exceeds the limit`）；
  - `POST <clob_url>/last-trades-prices`：同一批 token；
  - 价格字段同时接受字符串与数字（文档示例是数字，实测是字符串），Gamma 的 `outcomePrices` 元素与 Data API 的历史点同样如此；
  - 某个 token 缺失于响应时，该 outcome 按 `no_two_sided_book` 处理，并汇总一条诊断；
  - 整个 CLOB 请求失败时，回退规则见「一条 route 访问三个官方端点」。
- **输出**：
  - event：公共字段、search 的平台字段，以及 `markets` 数组（每个 market 一项）；
  - market：公共字段与单个 market 的字段。

  market 项包含：

  | 类别 | 字段 |
  |---|---|
  | 身份 | `ref`、`url`、`market_id`、`condition_id`、`question`、`label` |
  | 状态 | `status`、`end_date` |
  | 概率与报价 | `probability`、`outcomes`、`spread`、`last_trade_price`、`one_day_change`、`one_week_change`、`one_month_change` |
  | 规模 | `volume`、`liquidity` |

  event 级另有 `price_source`（`clob` 或 `gamma`）与 `prices_as_of`，每次 fetch 都带，落库的消费方据此区分实时值与缓存值。不输出 token id：调用方从不需要它，history 接受 market ref。
- **存储键**：slug 可能变化，也可能被重新创建的 event 复用，所以程序化消费方应以 `condition_id`（链上身份）为 market 的存储键，`event_id` 为 event 的存储键，slug 只用于展示与 URL。skill reference 写明这一点，并提醒跨时间引用同一市场时对比 `event_id`。
- **`abstract`**：另有内联 `rules`（event `description`），market 项在规则与 event 不同时另有 `rules`。`--format content` 输出规则文本。规则为空时为 Quality（退 5），与 YouTube 描述为空的规则相同。
- **大型 event**：markets 数组不截断。实测 128 个 market 的 `presidential-election-winner-2028` 输出约 100 KB，远低于 4 MiB 的协议上限，暂不需要 `--limit`。

## history（L2 平台操作）

`forager platform polymarket history MARKET [--outcome NAME] [--interval 1d|1w|1m|max] [--from DATE --to DATE] [--bucket 1m|5m|30m|1h|3h|12h|1d]`

- **为什么是新操作**：它的结果是时间序列，不是条目，改变了结果种类。只由 `polymarket_api` 实现，写成 inherent 方法 `PolymarketApi::history`；provider factory 声明 route 集合 `POLYMARKET_HISTORY_ROUTES` 与支持检查；core 新增 `platform_history`，编排方式与 `platform_stories` 相同，attempt 的 operation 为 `history`。
- **输入**：MARKET 接受 market ref 或 URL。
  - event ref 在飞行前无法知道它有几个 market，所以也接受：取回后恰有一个 market 时用它；多于一个时为 attempt 级 Parameter，前 10 个 market 的 ref 与 label 列在 stderr 的诊断中。失败载荷的 message 截断到 500 字符，十个长 ref 放不下，所以列表不放进 message。
  - `--outcome` 默认第一个 outcome（Yes），按名称不区分大小写匹配；没有这个 outcome 为 attempt 级 Parameter，消息列出可用名称。
- **时间窗**：`--interval` 与 `--from/--to` 互斥，默认 `--interval 1m`。
  - `--from/--to` 为 UTC 日期，含两端，区间不超过 15 天（Data API 显式窗口的上限），超过时飞行前退 2；
  - `--bucket` 默认按时间窗选择，使点数保持在约 30–200 个：

    | 时间窗 | 默认 `--bucket` |
    |---|---|
    | `1d` | `5m` |
    | `1w` | `1h` |
    | `1m`、`max` | `1d` |
    | 显式区间 | ≤2 天为 `5m`，其余为 `1h` |

    不带 bucket 时，服务端对 `1d` 会返回 1436 个点，会撑满调用方的上下文。
  - 显式 `--bucket` 取 Data API 支持的任一网格，供回测选择粒度。Data API 对 `max` 与 `1m` 要求 bucket 至少 600 秒，`1w` 至少 300 秒，违反时飞行前退 2。粒度超出保留期（1 分钟至少 7 天、5 分钟至少 60 天、30 分钟至少 90 天，3 小时与 12 小时永久）时上游返回空页，forager 不替换粒度。
- **请求**：
  1. Gamma 取 market 与 token（同 fetch）；
  2. `GET <data_url>/v2/prices-history?token_id=<id>&interval=<i>&bucket_seconds=<s>`，或 `start`/`end` 加 `bucket_seconds`；
  3. `pagination.has_more` 为 true 时，在同一次 route 运行内续页，最多 5 页：`next_cursor` 只记位置，续页带同一组参数加 `cursor`。仍未读完时 stderr 给出“未列全”诊断，提示加大 bucket。
- **输出**：
  - 形状：`{platform, provider, market: {ref, url, question, status}, outcome, interval, bucket_seconds, points: [{t, price}], settlement}`；
  - `t` 为精确到秒的 UTC；
  - 已结算 market 末尾 `resolution_seconds` 为 0 的结算点不放进 `points`，而是作为 `settlement: {t, price}`，未结算时为 `null`。这样序列里只有市场价格，不混入兑付值。未结算 market 的序列末尾同样有一个 `resolution_seconds` 为 0 的点，那是最新一次成交的观测，留在 `points` 中，所以拆分以 market 的 `status` 为 `resolved` 为前提。显式区间时输出的 `interval` 为 `null`，另有 `from` 与 `to`。
  - 合法空序列（超出保留期）返回 `points: []`，退 0。

  文档没有说明历史点的 `price` 是成交价还是 midpoint，skill reference 照此说明，不把它称作“概率历史”。

## 错误归因

| 情况 | 结果 |
|---|---|
| Gamma 404 `not found error` | attempt 级 Parameter，退 4 |
| Gamma 422 `validation error` | Runtime：请求由 forager 构造，出现即 forager 缺陷，消息带上游 `error` |
| Data API 400 | Parameter，消息取上游 `error`（实测超窗口为 400） |
| 429 | RateLimited |
| 其他状态 | 沿用 net 的共享映射 |
| 响应不是预期结构 | Runtime |
| CLOB 失败 | 不判失败，见「一条 route 访问三个官方端点」 |

## doctor 与 smoke

- **doctor**：注册探针为 `DoctorProbe::PlatformSearch`（查询 `election`、limit 1）。deep `doctor --provider polymarket_api` 在检索通过后另外检查：
  - `clob`：`GET <clob_url>/time`；
  - `data`：`GET <data_url>/v2/status`。

  任一项失败即 doctor 失败。shallow 检查沿用匿名 HTTP route 对 `url` 的可达性检查。
- **smoke**：route 匿名且在默认 order 中，默认运行。
  - C43：`search "fed rate" --limit 3`。
  - C44：fetch canary `polymarket:presidential-election-winner-2024`（已结算，不会再变），要求 Trump market 的 `status` 为 `resolved`、Yes `price` 为 1、`basis` 为 `resolution`，并且不请求 CLOB。
  - C45：fetch C43 第一条，覆盖实时报价路径，只要求结构合法，不要求概率非空。
  - C46：history canary `polymarket:market/will-donald-trump-win-the-2024-us-presidential-election --interval max`，要求 `settlement.price` 为 1。

## 实现改动清单

按 `07-platforms.md` 接入清单：

| 编号 | 改动 |
|---|---|
| R1 | `catalog::POLYMARKET`；`types::Platform::Polymarket`；新 types 叶子 `platform_polymarket.rs`（ref、选项、条目字段、状态与价格派生、history 请求与结果） |
| R2 | `ProviderId::PolymarketApi` 与 `ProviderRegistration`（访问策略、smoke 用例） |
| R3 | factory 的 search/fetch 支持检查与 `POLYMARKET_HISTORY_ROUTES`；`PlatformRoutesRuntimeConfig` 字段 |
| R4 | 配置 schema 的 `url`、`clob_url`、`data_url`、`timeout` 与 `platforms.polymarket.order` |
| R5 | `PlatformSearch` 探针及 deep 的 `clob`、`data` 检查 |
| R6 | C43–C46 与第 5 章矩阵 |
| R7 | `tests/acceptance-manifest.json` 中 `(polymarket_api, platform:polymarket:search|fetch|history)` 的 fixture |
| R8 | checklist 样例表的 `event` 与 `market` 样例 |

代码组织：

- **provider**：`providers/polymarket_api.rs`（route、Gamma 请求与解码）、`polymarket_api_quotes.rs`（CLOB 报价与回退）、`polymarket_api_history.rs`（Data API）。按端点职责拆分，控制在 1000 行以内。
- **core**：`core/platform_history.rs`。
- **cli**：`cli/polymarket.rs`，渲染并入 `platform_render.rs`。

`GLOSSARY.md`（Polymarket Event、Polymarket Market、Market-Implied Probability、Price History）与 ADR 0032 已随本设计写入。实现的同一改动还须更新：

- 第 2、3、4、5、7 章；
- skill 的平台词表、平台 reference 与 CLI reference。

平台 reference 写明以下几点：

- 概率是订单簿 midpoint，不是校准过的预测；
- neg-risk 组的概率之和不必为 1；
- 宽价差与低流动性时要看 bid/ask；
- search 的价格可能滞后最多 5 分钟；
- 历史价格的来源未说明；
- slug 可能变化或被复用，程序化消费以 `condition_id` 与 `event_id` 为存储键；
- ToS §4.2 的两条限制。

## 测试接缝

- **types 纯函数**：ref 往返与 URL 解析（语言前缀、`/event/<e>/<m>`、`/market/`、`polymarket.us` 拒绝）；状态与价格派生表的每一行；history 时间窗与 bucket 校验。这一层覆盖大部分规则。
- **route**：本地假 HTTP 服务分别模拟三个端点，覆盖以下场景：
  - search 的空集、分页与跳过诊断；
  - fetch 的 CLOB 成功、CLOB 失败回退、token 缺失、已结算不请求 CLOB、404、JSON 编码字段损坏；
  - history 的单 market event、多 market event 报错与结算点拆分。
- **live**：smoke C43–C46。

## 交付范围

第一期交付 search、fetch（`metadata` 与 `abstract`）与 history，spec 第 2、3、4、5、7 章一次写全。曾考虑的更小方案（不请求 CLOB，概率全部取 Gamma 的缓存值）不采用：它会把空订单簿的 0.5 占位值当作概率交付，违背 Market-Implied Probability 的定义。

## 实测结果（2026-10-24）

- **slug**：抽样 Gamma 约 88,000 个 event 与 market 的 slug，字符都在 `[a-z0-9-]` 内，另有 6 个含 `.`、4 个含 `_`，没有大写字母，最长 244 个字符。Gamma `/events/slug/<slug>` 对含 `.` 的 slug 返回 422 `slug is invalid`，含 `_` 的可以查到。解析规则因此接受字母、数字、`-` 与 `_`，长度 1–255。
- **CLOB 批量上限**：`POST /prices` 与 `POST /last-trades-prices` 每次最多 500 项，501 项返回 400 `{"error":"Payload exceeds the limit"}`。
- **没有挂单的表示**：两个 V2 样本确认了「挂单判定」。
  - 有挂单：`polyv2zec-updown-5m-1792836000` 的订单簿为 0.01/0.99，`/prices` 给出 BUY 0.01（最优买价）与 SELL 0.99（最优卖价）。
  - 空订单簿：`polyv2bkbsl-kar-fen-2026-10-25` 给出 BUY `"0"` 与 SELL `"1"`。
  - 同一批里，`/last-trades-prices` 给空订单簿的 token 返回 0.5，从未成交的 token 则不出现在结果里；未知 token 的 `/prices` 返回 `{}`。
- **neg-risk 占位 market**：`presidential-election-winner-2028` 的 128 个 market 中有 74 个是占位 market（如 `will-person-ai-win-…`）。
  - Gamma 字段：`active: false`、`acceptingOrders: true`、没有 `outcomePrices`、`bestBid` 为 0、`bestAsk` 为 1、`manualActivation: true`。
  - CLOB：`/book` 返回 404 `No orderbook exists`，`/prices` 不返回它们。
  - 只按 `closed` 与 `acceptingOrders` 判断时，它们会成为 `open`、`no_two_sided_book`，让 CLOB 请求多出 148 个 token，并使「没有报价」的诊断计入 148 个结果。派生表因此对未 `closed` 的 market 读取 `active`，它们为 `suspended`、`not_trading`，不请求 CLOB。
- **没有 token 的已关闭 market**：2026-10-24 用 Gamma `/markets/keyset` 抽样 5,000 个 market（`closed=true` 与 `closed=false` 各一半）。有 token 的 market 中，`outcomes` 与 token 数量全部一致；25 个已关闭未结算与 14 个已结算的 market（如 `will-the-kansas-city-chiefs-win-superbowl-55`、`will-the-los-angeles-lakers-win-the-nba-21-22-championship`）没有 `clobTokenIds`，`open` 的 market 都有 token。所以 token 数量在有 token 时一律核对，只有 `open` 的 market 必须有 token。抽样中的 `outcomePrices` 元素都是字符串。
- **`/public-search`**：
  - `page` 上限为 100（第 200 页返回 422 `must be less than or equal to 100`）；`limit_per_type=20` 时，第 95 页的 `hasMore` 已为 false。
  - 无论 `search_tags` 与 `search_profiles` 取何值，响应都不含 tags 与 profiles 键。
- **其他**：
  - Cloudflare 按 User-Agent 拦截部分客户端：Python urllib 的默认 UA 得到 403（error 1010），forager 与 curl 的 UA 正常。
  - CLOB `/time` 返回一个裸数字。
  - Gamma 常缺 `oneMonthPriceChange`，已结算 market 缺 `liquidity`。
- **Data API `/v2/prices-history`**：样本为已结算的 Trump canary（`will-donald-trump-win-the-2024-us-presidential-election` 的 Yes token）、未结算的 `will-no-fed-rate-cuts-happen-in-2026` 与一个月前结算的 `will-no-fed-rate-hikes-happen-in-2026-20260623190852889`。
  - **显式区间加 bucket**：`start`/`end` 加 `bucket_seconds` 严格按网格返回。`end` 不含端点：2026-10-10 0 点到 10-24 0 点（14 天）以 3600 秒返回 337 个点，10-10 到 10-25（15 天）返回 344 个点，10-09 到 10-25（16 天）返回 400 `'start' to 'end' must span at most 15 days`。窗口末端在未来也被接受。`start=0` 返回 400 `invalid timestamp`。
  - **interval 加 bucket**：`max` 加 86400 对 canary 返回 308 个点（不受 30 天限制），`max` 加 43200 与不带 bucket 都返回 615 个点；`max` 加 3600 返回空页，因为 `max` 在 10800 秒以下只覆盖最近 30 天。`max` 加 300 返回 400 `bucket_seconds must be at least 600 for this interval`，`1w` 加 60 返回 400 `... at least 300 ...`。`1d` 加 300 返回 287 个点，`1w` 加 3600 返回 168 个，`1m` 加 86400 返回 31 个。
  - **默认 bucket 的点数**：`1w`、`1m` 与 `max` 的默认值落在几十到三百个点之间；`1d` 的 `5m` 为 287 个，2 天显式区间的 `5m` 为 577 个，15 天显式区间的 `1h` 为 360 个，都多于「约 30–200 个」的目标。实现保留 #249 的映射，没有据此修改。
  - **续页 cursor**：`limit=20` 时 `pagination` 为 `{limit: 20, offset: 0, has_more: true, next_cursor: <base64>}`，解码后只含页大小、位置时间戳、bucket 与已读点数。只带 `cursor` 返回 400 `required query param 'token_id' not provided`，带 `token_id` 而缺时间窗返回 400 `provide a time component ...`；带同一组参数加 `cursor` 返回下一页（`offset: 20`）。换一个 `bucket_seconds` 再带同一 cursor 不报错，而是按新网格从同一位置继续，所以续页必须原样重发参数。
  - **超出保留期的空页**：未结算样本以 60 秒取 2026-09-20 到 09-22 返回 200、`data: []`、`has_more: false`。一个月前结算的样本取 `1m` 加 86400 也返回空页，连结算点都没有（结算点放在最后一行之后一个 bucket 处，落在窗口外），所以已结算 market 的 `settlement` 也可能为 `null`。未知 token 同样返回 200 空页。
  - **末尾的 `resolution_seconds: 0`**：未结算样本每个时间窗的最后一点都是 `resolution_seconds` 为 0 的单次观测（如 `1792827258`，不在网格上），是最新成交，不是结算点。已结算样本的最后一点是兑付值（canary 为 `2024-11-07T00:00:00Z` 的 1.0，一个月前结算的样本为 0.0），前面没有另一个单次观测。
  - **`/v2/status`**：返回 `{"data": {"computed_at", "age_seconds", "serving", "ingestion", ...}}`，deep doctor 只核对 `data` 是对象。

## 仍未实测的部分

- **slug 的变化**：slug 是否会变、旧 slug 是否仍可查询（`pastSlugs`）仍未知。
- **Data API 的多页序列**：默认 bucket 下没有超过一页（10,000 个点）的序列；5 页上限与「未列全」诊断只由 fixture 测试覆盖。
