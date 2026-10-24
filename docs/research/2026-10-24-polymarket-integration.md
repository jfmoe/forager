# Polymarket 平台接入调研

核验日期：**2026-10-24**。仓库基线：`main` 上的 `df35af4afa1290f2f31f78df5718683e2b6bcb4d`（release `v0.16.0` 之后的一个文档提交）；本机 `forager 0.16.0`。本文是调研记录，**没有实现 Polymarket Platform，也没有修改 spec 或 ADR**；目标是确认能否以内置平台的形式检索 Polymarket 的事件与市场，并取回其概率。原文快照、匿名实测响应（已裁剪）与源码摘录保存在 [证据目录][E-dir]，逐文件来源、取回时间与 SHA-256 见 [manifest][E-manifest]，版本与出口见 [环境][E-env]。

每条事实标注核实方式：**〔文档〕** 指 docs.polymarket.com、官方 OpenAPI/AsyncAPI 规范或条款原文；**〔实测〕** 指 2026-10-24 04:46–04:54 UTC 的匿名 curl 请求；**〔源码〕** 指 Polymarket 官方 GitHub 仓库的浅克隆。实测经本机现有代理出口，`polymarket.com/api/geoblock` 判定为美国加州、`blocked: true`；结论只代表这个出口和这一时段，不外推到所有地区。[地域实测][E-geoblock]、[环境][E-env]。

## 结论摘要

- **读接口全部匿名可用。** 发现与元数据走 Gamma API（`https://gamma-api.polymarket.com`），实时价格与订单簿走 CLOB API（`https://clob.polymarket.com`），历史价格、成交、持仓、未平仓量与结算状态走 Data API（`https://data-api.polymarket.com/v2`）。文档说明公开市场数据无需凭据，Data API v2 规范写明所有数据路由公开、不需要 key；本次在被 geoblock 判定为封锁的出口上，所有读请求都返回了 200。〔文档〕〔实测〕[D-api]、[S-data]、[E-geoblock]。
- **身份是两层：event 与 market。** event 用数字 `id` 与 `slug` 定位，market 用数字 `id`、`slug` 或链上 `conditionId` 定位；取价格要用 outcome 的 token ID（CTF 市场在 JSON 编码字符串 `clobTokenIds` 中，Protocol V2 市场在数组 `positionIds` 中）。`outcomes` 与 `outcomePrices` 也是 JSON 编码的字符串。〔文档〕〔实测〕[D-details]、[E-event-slug]。
- **“概率”最贴近的量是订单簿 midpoint。** 文档说价格就是隐含概率，页面显示 midpoint，价差超过 $0.10 时改显示最近成交价；但实测中 Gamma 的 `outcomePrices` 在价差 0.626 时仍等于 midpoint，polymarket.com 服务端渲染的页面也显示 midpoint（63%），而不是最近成交价（46%）。〔文档〕〔实测〕[D-prices]、[E-wide-gamma]、[E-wide-web]。
- **Gamma 的价格字段可能滞后几分钟。** Gamma 的 event/market 响应带 `Cache-Control: public, max-age=300`，经 CloudFront 缓存，实测命中缓存时 `age: 24`；CLOB 价格端点不缓存。同一时刻两者的 `bestAsk` 相差 0.002。〔实测〕[E-stale-r1-gamma]、[E-stale-r2-gamma]、[E-stale-r1-book]。
- **搜索用 `GET /public-search`。** 它按 `page` 翻页，返回 `pagination.hasMore` 与 `totalResults`；OpenAPI 只列参数名，没有语义说明；默认结果包含已关闭的 event。〔文档〕〔实测〕[D-search]、[E-search-election]。
- **价格历史有两个端点。** 2026-09-04 起文档把 Data API `GET /v2/prices-history` 作为 CLOB `GET /prices-history` 的替代，但后者仍在 API 参考中并实测可用。两者显式时间窗都有上限（v2 为 15 天；CLOB 40 天被拒）；`interval=max` 可取全历史。v2 对已结算市场在序列末尾追加一个结算点（0 或 1）。〔文档〕〔实测〕[D-changelog]、[D-hist-v2]、[E-hist-resolved-v2]。
- **条款不禁止匿名读取，但限制抓取与面向资本市场机构的数据使用。** Terms of Use §4.2 禁止用 robots/crawlers 抓取、禁止“为未授权目的收集信息”，并禁止 Capital Market Client 与市场数据分发商在未获书面同意时访问或使用任何形式的 Data（明确包括经 API 获取），也禁止向它们转售或再分发 Data。geoblock 文档只把地域限制用于下单。〔文档〕[L-tos]、[D-geo]。
- **Polymarket US 是独立产品。** 它是 CFTC 监管的交易所，公共只读 API 在 `https://gateway.polymarket.us`，与国际版的 id/slug 空间不同。〔文档〕〔实测〕[US-what]、[US-intro]、[E-us-search]。

设计含义（意见，非已核实事实）集中在文末「对 forager 平台设计的含义」。

## 服务与端点清单

下表列出与“检索事件/市场并取得概率”相关的只读端点。除另注明外，参数与形状来自官方 OpenAPI，并有本次实测响应。[D-overview]、[S-gamma]、[S-clob]、[S-data]。

| 服务 | 端点 | 作用 | 鉴权 | 核实 |
|---|---|---|---|---|
| Gamma | `GET /public-search?q=` | 搜索 event、tag、profile | 无 | 〔文档〕〔实测〕[D-search]、[E-search-fed] |
| Gamma | `GET /events/{id}`、`GET /events/slug/{slug}` | 单个 event，内嵌全部 market | 无 | 〔文档〕〔实测〕[E-event-id]、[E-event-slug] |
| Gamma | `GET /events/keyset`（推荐）、`GET /events`（offset，计划弃用） | 列表与过滤，含 `title_search` | 无 | 〔文档〕〔实测〕[D-changelog]、[E-keyset-title] |
| Gamma | `GET /markets/{id}`、`GET /markets/slug/{slug}` | 单个 market | 无 | 〔文档〕〔实测〕[E-market-id]、[E-market-slug] |
| Gamma | `GET /markets/keyset`（`condition_ids`、`clob_token_ids`、`question_ids` 等过滤） | 按链上身份查 market | 无 | 〔文档〕〔实测〕[E-keyset-condition] |
| Gamma | `GET /tags`、`/tags/slug/{slug}`、`/tags/.../related-tags[/tags]` | 分类与相关分类 | 无 | 〔文档〕〔实测〕[D-discover]、[E-tag] |
| Gamma | `GET /series`、`/series/{id}` | 周期性 event 的分组 | 无 | 〔文档〕〔实测〕[E-series] |
| Gamma | `GET /comments?parent_entity_type=&parent_entity_id=` | event/series/market 评论 | 无 | 〔文档〕〔实测〕[S-gamma]、[E-comments] |
| Gamma | `GET /sports`、`/sports/market-types`、`/teams` | 体育元数据 | 无 | 〔文档〕[D-discover] |
| CLOB | `GET /midpoint`、`POST /midpoints` | best bid/ask 的中点 | 无 | 〔文档〕〔实测〕[E-midpoint]、[E-midpoints-post] |
| CLOB | `GET /price?side=`、`GET`/`POST /prices` | 单边最优价 | 无 | 〔文档〕〔实测〕[E-price-buy]、[E-prices-post] |
| CLOB | `GET /book`、`POST /books` | 订单簿摘要，含 `last_trade_price`、`tick_size`、`neg_risk` | 无 | 〔文档〕〔实测〕[E-book] |
| CLOB | `GET /spread`、`GET /last-trade-price`、`POST /last-trades-prices` | 价差、最近成交 | 无 | 〔文档〕〔实测〕[E-spread]、[E-ltp] |
| CLOB | `GET /prices-history`、`POST /batch-prices-history`（≤20 个 token） | 历史价格（旧位置） | 无 | 〔文档〕〔实测〕[D-hist-clob]、[D-hist-batch]、[E-hist-clob-1d] |
| CLOB | `GET /markets-by-token/{token_id}` | token → conditionId 与两侧 token | 无 | 〔文档〕〔实测〕[D-by-token]、[E-by-token] |
| Data v2 | `GET /v2/prices-history` | 历史价格（新位置） | 无 | 〔文档〕〔实测〕[D-hist-v2]、[E-hist-v2-1d] |
| Data v2 | `GET /v2/trades`、`/v2/holders`、`/v2/oi`、`/v2/live-volume` | 成交、持有人、未平仓量、event 成交量 | 无 | 〔文档〕〔实测〕[D-migrate]、[E-trades]、[E-holders]、[E-oi] |
| Data v2 | `GET /v2/resolutions`、`GET /v2/tokens`、`GET /v2/status` | 结算生命周期、token 反查、数据新鲜度 | 无 | 〔文档〕〔实测〕[D-res-state]、[E-resolutions]、[E-tokens]、[E-status] |
| WebSocket | `wss://ws-subscriptions-clob.polymarket.com/ws/market` | 公共订单簿/价格/生命周期推送，按 token 订阅 | 无 | 〔文档〕，未实测 [D-api]、[D-wss] |
| WebSocket | `wss://ws-live-data.polymarket.com`（RTDS） | 公共参考价、评论、成交流；参考价已迁到需 CLOB 凭据的 PolyBolt，`comments` 仍在 RTDS | 文档未写需要 | 〔文档〕，未实测 [D-api]、[D-changelog] |

补充事实：

- 交易、个人订单与成交、API key 管理等 CLOB 私有请求需要 L1（EIP-712 钱包签名）派生的 L2 HMAC 凭据；只读市场数据不需要。〔文档〕[D-api]。
- Data API v1（无 `/v2` 前缀的 `/trades`、`/oi` 等）已冻结。迁移文档写明“**Data API v1 is retired on October 24, 2026**”；但 2026-10-24 04:50 UTC 的 v1 请求仍返回 200。v1 不应作为新接入的依据。〔文档〕〔实测〕[D-migrate]、[E-v1-trades]、[E-v1-oi]。
- Gamma 与 Data v2 的错误形状不同：Gamma 为 `{"type":"validation error"|"not found error","error":...}`；Data v2 为 `{error, code, retryable, trace_id}`，并在每个响应带 `x-trace-id`。〔文档〕〔实测〕[E-keyset-offset]、[E-event-missing]、[E-hist-v2-40d]、[S-data]。

## 数据模型与身份

### event、market、outcome

- **event** 是展示给用户的问题或主题，包含一个或多个 market；**market** 是可结算的二元条件，有 Yes 与 No 两个 outcome；每个 outcome 有自己的 token。单 market 的 event 与其 market 基本等价；多 market 的 event 把一个大问题拆成若干二元问题，例如每个候选人一个 market。〔文档〕[D-concepts]、[D-overview-md]。
- 互斥的多 market event 组成 **negative-risk 组**：恰好一个 market 结算为 Yes。event 上 `enableNegRisk` 与 `negRiskAugmented` 同时为 true 时是 augmented neg-risk：可后续补入命名 outcome，另有 placeholder 与 “Other”；官方警告 placeholder 与 Other 的含义会变，UI 不显示未命名 outcome。〔文档〕[D-negrisk]、[D-details]。
- 实测 neg-risk event `how-many-fed-rate-cuts-in-2026`（id `51456`）有 13 个 market，`negRisk: true`、`enableNegRisk: true`、`negRiskAugmented: false`；各 market 的 Yes `outcomePrices` 之和为 0.988，不是 1。已结算的 `presidential-election-winner-2024`（17 个 market）之和为 1.0。〔实测〕[E-event-slug]、[E-event-resolved]。
- 多 market event 中每个 market 的短标签在 `groupItemTitle`，例如 `"0 (0 bps)"`、`"Donald Trump"`、`"↑ $105"`。〔实测〕[E-event-slug]、[E-wide-gamma]。

### 标识符

| 标识 | 所属 | 形态与用途 | 核实 |
|---|---|---|---|
| event `id` | Gamma | 数字字符串，`/events/{id}`；Data v2 的 `event_id` 用同一空间 | 〔文档〕〔实测〕[D-data]、[E-event-id] |
| event `slug` | Gamma | 出现在 `polymarket.com/event/<slug>`；`/events/slug/{slug}` | 〔文档〕〔实测〕[D-concepts]、[E-event-slug] |
| market `id` | Gamma | 数字字符串，`/markets/{id}`；Data v2 的 `market_id` | 〔文档〕〔实测〕[D-details]、[E-market-id] |
| market `slug` | Gamma | `/markets/slug/{slug}`；`polymarket.com/event/<event>/<market>` 的第二段 | 〔文档〕〔实测〕[E-market-slug]、[E-web-urls] |
| `conditionId` | 链上 | `0x` + 64 hex；Data v2 的统一查询键 `condition`；Gamma keyset `condition_ids` | 〔文档〕〔实测〕[D-data]、[E-keyset-condition] |
| `questionID` | 链上 | 结算用的问题哈希；Data v2 `/v2/resolutions?question_id=`；Gamma keyset `question_ids` | 〔文档〕[D-concepts]、[D-res-state] |
| `clobTokenIds` | CTF（`version: "v1"`） | **JSON 编码字符串**，如 `"[\"1240…\", \"2129…\"]"`，下标 0 为 Yes、1 为 No | 〔文档〕〔实测〕[D-details]、[E-event-slug] |
| `positionIds` | Protocol V2（`version: "v2"`） | 真正的 JSON 数组；文档要求按 `version` 选字段，两者都可能有值 | 〔文档〕；本次样本均为 v1 [D-details] |
| `negRiskMarketID` | neg-risk 组 | 组内 market 共享；Data v2 `/v2/tokens` 返回 `neg_risk_market_id` 与 `question_index` | 〔实测〕[E-event-slug]、[E-tokens] |

- Gamma `/events/keyset` 与 `/markets/keyset` 的 `version` 省略时默认只返回 CTF（v1）；需要 V2 要显式传 `version=v2` 或重复两次。这一参数出现在文档中，但不在本次取回的 Gamma OpenAPI 参数表里。〔文档〕[D-discover]、[S-gamma]。
- token 反查：CLOB `/markets-by-token/{token_id}` 返回 `condition_id`、`primary_token_id`、`secondary_token_id`；Data v2 `/v2/tokens?token_id=` 还返回 outcome 标签、`event_id`、`event_slug`、`market_slug`、`resolved` 与 `final_price`。〔实测〕[E-by-token]、[E-tokens]。
- `/markets/slug/{slug}` 的响应内嵌所属 `events` 数组（含 event id 与 slug）；同一 market 的 `/markets/{id}` 响应没有 `events`。所以从 market 找 event 应走 slug 端点或 keyset。〔实测〕[E-market-slug]、[E-market-id]。

### 字段：状态、价格、成交量

| 字段 | 含义（文档） | 实测样例 |
|---|---|---|
| `outcomes`、`outcomePrices` | JSON 编码字符串，同下标对应同一 outcome | `"[\"Yes\", \"No\"]"`、`"[\"0.962\", \"0.038\"]"` |
| `lastTradePrice`、`bestBid`、`bestAsk`、`spread`、`oneDayPriceChange` | 最近成交、最优买卖、价差、24 小时价格变化 | `0.961`、`0.961`、`0.963`、`0.002`、`-0.0025`；无买单时 `bestBid` 缺失 |
| `active` | 已部署且未归档 | 已结算市场仍为 `true` |
| `closed` | 已结算或关闭，不能再交易 | 结算后 `true` |
| `archived` | 归档、只读，无交易、无结算更新 | `false` |
| `acceptingOrders` | 订单簿接收新订单 | 结算后 `false` |
| `umaResolutionStatus` | OpenAPI 只给类型（string） | 已结算为 `"resolved"`；未结算市场该键缺失，另有 `umaResolutionStatuses: "[]"` |
| `endDate`、`endDateIso`、`closedTime` | 结束日期、关闭时间 | `2027-01-01T04:59:00Z`；`closedTime: "2024-11-06 15:17:41+00"`（非 ISO 格式） |
| `volume`、`volumeNum`、`volume24hr`、`liquidity`、`liquidityNum` | 成交量与流动性；`volume`/`liquidity` 在 market 上是字符串 | event 上 `volume` 是数字 |

来源：〔文档〕[D-details]、[S-gamma]；〔实测〕[E-event-slug]、[E-market-slug]、[E-event-resolved]。

`active` 与 `closed` 的组合容易误读：已结算的 2024 总统选举 market 为 `active: true, closed: true`。判断“能否交易”应看 `closed` 与 `acceptingOrders`，不能只看 `active`。〔实测〕[E-event-resolved]。

### 已结算市场如何显示结果

- 文档：结算后交易停止，赢方 token 可兑 $1，输方归零；UMA 争议结果为 “Unknown/50-50” 时每个 token 兑 $0.50。〔文档〕[D-resolution]。
- Gamma：Trump market 的 `outcomePrices` 为 `["1", "0"]`，Biden/Harris 为 `["0", "1"]`，`umaResolutionStatus: "resolved"`；但同一响应里 `bestBid`/`bestAsk`（0.997/0.998）与 `lastTradePrice`（Biden 为 1）是残留值，不能当结果读。〔实测〕[E-event-resolved]。
- CLOB：已结算 token 的 `/midpoint` 返回 404 `{"error":"No orderbook exists for the requested token id"}`。〔实测〕[E-midpoint-resolved]。
- Data v2：`/v2/resolutions?condition=` 返回 `status: "resolved"`、`price: "1000000000000000000"`（1e18 表示 Yes）、`was_disputed` 等；规范列出的状态有 `initialized`、`posed`、`proposed`、`challenged`、`reproposed`、`disputed`、`resolved`，condition 维度还可能有 `active` 与 `arbitration`，已结算的 condition 行带 `payouts`（每份 micro-USDC）。`/v2/tokens` 带 `resolved` 与 `final_price`。〔文档〕〔实测〕[S-data]、[E-resolutions]、[E-tokens]。

## “概率”应该取哪个值

### 文档的定义

- 每份价格在 $0–$1 之间，“直接代表市场对该 outcome 概率的看法”；**显示价格是买卖价差的 midpoint，价差大于 $0.10 时改显示最近成交价**。〔文档〕[D-prices]。官方 agent-skills 仓库也写了同一条规则。〔源码〕[G-repos]。
- CLOB `/midpoint` 是 best bid 与 best ask 的平均；`/last-trade-price` 在 token 从未成交时返回占位值 `{"price":"0.5","side":""}`，批量版本则直接省略这类 token。〔文档〕[D-books]、[D-ltp]。
- 文档没有说明 Gamma `outcomePrices` 如何计算，也没有说明价格历史中的 `price` 是成交价还是 midpoint。〔文档〕[D-details]、[D-hist-v2]。

### 实测对照

| 样本（UTC） | Gamma `outcomePrices[0]` | Gamma bid/ask | CLOB midpoint | 最近成交 | 页面显示 |
|---|---|---|---|---|---|
| `will-no-fed-rate-cuts-happen-in-2026`，04:47:52–04:48:00 | 0.962 | 0.961 / 0.963 | 0.962 | 0.961 | 未取 |
| `will-hood-reach-105-in-october-2026`（价差 0.626），04:48:43–04:48:57 | 0.634 | 0.321 / 0.947 | 0.6345 | 0.46 | **63%** |
| `bitcoin-above-80k-on-october-24-2026`，04:52:40–04:52:44 | 0.945 | 0.942 / 0.949 | 0.9445（book 0.942 / 0.947） | 0.945（book） | 未取 |
| `nfl-chi-sea-…-2h-spread-home-21pt5`（无买单，ask 1） | 0.5 | 无 / 1 | 未取 | 0.01 | 未取 |

来源：[E-market-slug]、[E-midpoint]、[E-book]、[E-wide-gamma]、[E-wide-mid]、[E-wide-ltp]、[E-wide-web]、[E-stale-r1-gamma]、[E-stale-r1-book]、[E-wide-spread-list]。

由此可确认：

1. 在所有样本中，Gamma `outcomePrices` 等于 **Gamma 自身 `bestBid`/`bestAsk` 的中点**，没有套用“价差 > $0.10 改用最近成交”的规则；无买单时按 bid=0 计算，得 0.5。〔实测〕
2. polymarket.com 事件页的服务端渲染 HTML 对价差 0.627 的 market 显示 63%，与 midpoint 一致，与最近成交 46% 不一致。**这与文档的显示规则矛盾。** 只有一个样本；该 HTML 带 `age: 151`、`s-maxage=60, stale-while-revalidate=3600`，且客户端水合后的显示没有核实。〔实测〕[E-wide-web]
3. 页面上的 “Buy Yes 94.8¢ / Buy No 67.9¢” 分别是 Yes 的 best ask 与 1 − best bid。〔实测〕[E-wide-web]

### CLOB `/price` 的 side 语义

- API 参考：“Returns the best bid price for BUY side or best ask price for SELL side.”〔文档〕[D-price-ref]
- Market Data 指南：“`BUY` returns the lowest ask … `SELL` returns the highest bid.”〔文档〕[D-books]
- 实测：同一 token、book 为 0.961/0.963 时，`side=BUY` 返回 0.961（best bid），`side=SELL` 返回 0.963（best ask），**与 API 参考一致，与指南相反**。〔实测〕[E-price-buy]、[E-price-sell]、[E-book]

### 新鲜度与缓存

- Gamma event、market、keyset 列表响应为 `Cache-Control: public, max-age=300`，搜索与评论为 `max-age=60`，tag 为 `max-age=1800`，均经 CloudFront。第二次请求同一 market 时 `x-cache: Hit from cloudfront`、`age: 24`，响应与 26 秒前完全相同。〔实测〕[E-stale-r1-gamma]、[E-stale-r2-gamma]、[E-tag]
- CLOB 的 midpoint、price、book、spread、last-trade 不带 `Cache-Control`，`cf-cache-status: DYNAMIC`；两轮 book 的 `timestamp` 不同。〔实测〕[E-midpoint]、[E-stale-r1-book]、[E-stale-r2-book]
- Gamma 的 `updatedAt` 在多 market 上仅比请求早几秒（例如 `04:52:37` 对 `04:52:40` 的请求），但 CDN 仍可能再返回最多 300 秒内的副本。fresh 的 Gamma 响应与 3 秒后的 CLOB book 在 `bestAsk` 上相差 0.002。〔实测〕[E-stale-r1-gamma]、[E-stale-r1-book]
- CLOB 批量 `POST /prices` 与 `POST /midpoints` 返回字符串（`"0.961"`），文档示例是数字。GET 形式的 `/prices?token_ids=…&sides=…` 按文档参数调用返回 400 `{"error":"Invalid payload"}`，原因未查明。〔文档〕〔实测〕[D-books]、[E-prices-post]、[E-midpoints-post]、[E-prices-get]

## URL 形式、slug 与离线解析

| URL 形式 | 实测行为 | 离线可得 |
|---|---|---|
| `https://polymarket.com/event/<event-slug>` | 200；`<link rel="canonical">` 为自身 | event slug |
| `https://polymarket.com/event/<event-slug>/<market-slug>` | 200；canonical 指向 **event** URL | event slug 与 market slug |
| `https://polymarket.com/market/<market-slug>` | **307** 跳到 `/event/<event-slug>/<market-slug>` | market slug（event slug 需联网） |
| `https://polymarket.com/<locale>/event/<slug>`，如 `/zh/` | 200，canonical 保留语言前缀；页面 hreflang 列出 `en`（无前缀）与 `bn`、`de` 等 | 去掉前缀后同上 |
| `…/event/<slug>?tid=<数字>` | 200，canonical 去掉查询串 | 查询串可丢弃 |
| `…/event/<数字 id>`、`…/event/<不存在的 slug>` | 均 200，标题为站点通用标题（软 404） | 不能用 HTTP 状态判断存在性 |
| 体育赛事 `…/event/nfl-chi-sea-2026-11-03` | 200，canonical 为自身 | event slug |

来源：〔实测〕[E-web-urls]。ToS 页面原始 HTML 中的语言前缀为 `bn`、`de`、`es`、`fr`、`hi`、`id`、`it`、`ja`、`pl`、`ru`、`th`、`tl`、`uk`、`vi`、`zh`、`zh-hant`。〔实测〕[L-tos-source]。`/sports/...` 之类的体育路径是否存在独立的 market 页面形式，本次没有核实。

官方统一 Python SDK 的 URL 解析（`_parse_polymarket_url`）只接受 `https`、主机 `polymarket.com` 或 `www.polymarket.com`：event 查找要求路径恰为 `/event/<slug>`；market 查找接受 `/event/<slug>`、`/event/<event>/<market>`（取最后一段）或 `/market/<slug>`。它不处理语言前缀。对单段 `/event/<slug>` 做 market 查找时，它直接把 event slug 当成 market slug，这只在单 market event 两者相同时成立。〔源码〕[G-paths]、[G-paths-test]。

slug 的唯一性与稳定性：

- 文档称“Every market and event has a unique slug”。〔文档〕[D-concepts]
- 同名的周期性 event 用时间戳后缀区分：文档示例 `fed-decision-in-october` 指向 2024 年的旧 event（id `27824`），当前一期是 `fed-decision-in-october-20260617190323537`。所以人读到的标题不能直接拼成 slug。〔实测〕[E-event-doc-example]、[E-keyset-title]
- Gamma Market schema 有 `pastSlugs` 字段（string，nullable，无说明），暗示 slug 可能改过；本次所有响应中都没有出现这个键，**没有找到 slug 会否改变、旧 slug 是否仍可解析的文档**。〔文档〕〔实测〕[S-gamma]
- 数字 `id` 有稳定的直接查找端点（`/events/{id}`、`/markets/{id}`），但没有对应的 polymarket.com 页面 URL。没有查到 id 稳定性的承诺。〔文档〕〔实测〕[E-web-urls]

## 搜索

### `GET /public-search`

- 参数：`q`（必填）、`cache`、`events_status`、`limit_per_type`、`page`、`events_tag`（数组）、`keep_closed_markets`（整数）、`sort`、`ascending`、`search_tags`、`search_profiles`、`recurrence`、`exclude_tag_id`（数组）、`optimized`。OpenAPI 只给名称与类型，没有语义、取值或上限说明。〔文档〕[D-search]
- 响应：`events`（完整 Event，内嵌全部 market 及 `outcomePrices`、`clobTokenIds`）、`tags`、`profiles`、`pagination: {hasMore, totalResults}`。默认返回 5 个 event。〔文档〕〔实测〕[D-discover]、[E-search-fed]
- 实测行为：

| 请求 | 结果 |
|---|---|
| `q=fed rate` | 5 个 event，均为未关闭；`totalResults: 209` [E-search-fed] |
| `q=election&limit_per_type=3` | 前 3 个都是已关闭的 event（2023–2025 年选举）；`totalResults: 4588` [E-search-election] |
| 同上加 `events_status=closed&keep_closed_markets=1` | 与不加时逐字节相同 [E-search-election-closed] |
| `events_status=active&page=1` / `page=2` | 只含未关闭 event，`totalResults: 1899`；两页结果不同；`page=1` 与不带 `page` 相同 [E-search-p1]、[E-search-p2] |
| 加 `sort=volume24hr&ascending=false` | 与不加时逐字节相同，未见排序效果 [E-search-sort] |
| `limit_per_type=100&optimized=true&search_tags=true` | 98 个 event、3 个 tag；顶层改为 `hasMore`，没有 `pagination`；market 只剩 `question`、`slug`、`outcomePrices`、bid/ask 等，没有 `conditionId` 与 `clobTokenIds` [E-search-big] |
| 无匹配 | 200，正文只有 `{"pagination":{"hasMore":false,"totalResults":0}}`，没有 `events` 键 [E-search-empty] |

“election” 在 `events_status=active` 下排在前面的是成交量很小的组合类 event，说明默认排序不是按成交量；排序规则没有文档。〔实测〕[E-search-p1]

### 列表过滤作为补充

`GET /events/keyset` 支持 `title_search`、`tag_id`/`tag_slug`、`closed`（`true`、`false`、`any`）、`series_id`、`game_id`、日期与成交量区间、`order` + `ascending`、`after_cursor`；`limit` 最大 100、默认 20。实测 `title_search=fed&order=volume24hr&ascending=false` 按 24 小时成交量降序返回；`limit=101` 被静默截到 100 条；带 `offset` 返回 422 `"offset is not allowed on keyset endpoints"`。`/markets/keyset` 的 `closed=any` 必须带至少一个身份过滤（每种最多 100 个值），否则 422。event 列表省略 `closed` 时包括已关闭的；market 列表省略时只含未关闭的（2026-04-09 起）。〔文档〕〔实测〕[S-gamma]、[D-discover]、[D-changelog]、[E-keyset-title]、[E-keyset-101]、[E-keyset-offset]、[E-keyset-any]

## 价格历史

| 项 | CLOB `GET /prices-history` | Data v2 `GET /v2/prices-history` |
|---|---|---|
| 文档状态 | 仍在 API 参考；changelog 称 v2 “replaces the CLOB-hosted route” | 2026-09-04 发布的新位置 |
| 键 | `market`（实为 token/asset id） | `token_id` |
| 时间窗 | `interval` ∈ `max, all, 1m, 1w, 1d, 6h, 1h`，或 `startTs`/`endTs` | 三选一：`interval`（同一组值）、`start`[+`end`]、`as_of` |
| 粒度 | `fidelity`，单位分钟，默认 1 | `bucket_seconds` 60–86400；`max`/`all`/`1m` 下限 600，`1w` 下限 300；省略时服务端按约 2,500 点选择 |
| 范围上限 | 文档未写；实测 40 天返回 400 `"'startTs' and 'endTs' interval is too long"` | 显式窗口最多 15 天；实测 40 天返回 400 |
| 分页 | 无 | `limit`（默认且最大 10,000），`cursor`/`next_cursor` |
| 保留 | 文档未写 | 1 分钟至少 7 天、5 分钟至少 60 天、30 分钟至少 90 天；3 小时与 12 小时永久（回到 2022-11-18） |
| 响应 | `{"history":[{"t":…,"p":…}]}` | `{"data":[{"timestamp","price","resolution_seconds"}],"pagination":{…}}` |
| 已结算市场 | `interval=max&fidelity=1440` 返回 307 点，最后一点 0.58，无结算点 | `interval=max` 返回 615 个 12 小时点，末尾 `price: 1.0, resolution_seconds: 0` 的结算点 |
| 缓存 | `max-age=90`，实测 Cloudflare HIT | `max-age=90`（1d）或 `300`（max） |
| 限速（文档） | 1,000 req / 10s | 200 req / 10s |

来源：〔文档〕[D-hist-clob]、[D-hist-v2]、[D-data]、[D-changelog]、[D-rate]；〔实测〕[E-hist-clob-1d]、[E-hist-clob-40d]、[E-hist-v2-1d]、[E-hist-v2-40d]、[E-hist-resolved-v2]、[E-hist-resolved-clob]。

v2 文档还规定：发送 `bucket_seconds` 时严格按该网格返回，超出保留期返回空页而不替换粒度；序列以窗口内最新的真实观测作“终点”，可能落在桶边界之间，各点间隔不能假定均匀；已结算市场的结算点为请求 token 的链上兑付值（输 0、赢 1，作废或拆分时为分数），只在链上结算完成后、分页的最后一页出现。〔文档〕[D-hist-v2]。实测 1d 窗口的终点（`resolution_seconds: 0`，0.962）等于当时的 CLOB midpoint；历史点的价格来源仍无文档。〔实测〕[E-hist-v2-1d]、[E-midpoint]

## 限速与响应头

文档的限速按 IP，由 Cloudflare 执行，“超限时请求被限流（延迟或排队）而不是立即拒绝”，窗口滑动。与本主题相关的数值：〔文档〕[D-rate]

| 范围 | 限额 |
|---|---|
| 全局 | 15,000 req / 10s |
| Gamma 总体 / `/events` / `/markets` / 列表合计 | 4,000 / 500 / 300 / 900 req / 10s |
| Gamma `/public-search` / `/comments` / `/tags` | 350 / 200 / 200 req / 10s |
| CLOB 总体 | 9,000 req / 10s |
| CLOB `/book`、`/price`、`/midpoint` | 各 1,500 req / 10s |
| CLOB `/books`、`/prices`、`/midpoints` | 各 500 req / 10s |
| CLOB `/prices-history` | 1,000 req / 10s |
| Data v2 总体 / `/v2/trades` / `/v2/prices-history` | 800 / 300 / 200 req / 10s |

与之并存的说法和观测：

- Data v2 规范：重查询先短暂排队，排不到时返回 `429` 与 `Retry-After`；每个调用方另有请求配额，突发超额同样 `429`；数据库连接不足时是 `503 request_timeout`。CLOB 的 `ErrorResponse` 有可选 `retry_after_seconds`。这与“限流而非拒绝”的总述并不一致，客户端应同时处理排队变慢与 429。〔文档〕[S-data]、[D-hist-clob]
- 本次约 70 次匿名请求（API 与页面）、间隔 ≥1.2 秒，没有遇到 429；**任何服务都没有返回限速类响应头**（无 `X-RateLimit-*`、`RateLimit-*`）。没有尝试触发限流。〔实测〕[E-manifest]
- Gamma 的响应头是 `server: istio-envoy`、`via: … cloudfront.net`、`x-cache`，没有 Cloudflare 头；CLOB、Data API 与 polymarket.com 为 `server: cloudflare`。所以文档说的 Cloudflare 限流在 Gamma 上由哪一层执行，从响应头看不出来。〔实测〕[E-search-fed]、[E-midpoint]、[E-trades]
- 条款 §3.3 保留随时调整速率限制的权利，可不另行通知。〔文档〕[L-tos]

## 条款与地域限制

**以下是条款原文的工程含义整理，不是法律意见。** 条款版本为 “Effective as of August 11, 2026”，原文是 polymarket.com/tos 页面内嵌的 Google 文档。[L-tos]、[L-tos-source]。

- **适用范围包含 API。** 条款适用于 Site 上的 Content Features 与 Technology Features；“BY ACCESSING … (INCLUDING BY CONNECTING VIA AN API …)” 即视为同意。§1 写明位于 Restricted Jurisdiction 的人**只能**使用 Content Features（新闻与信息），不能使用 Technology Features 与 Platform。〔文档〕[L-tos]
- **交易限制。** 美国、法国、德国、英国、新加坡、泰国、台湾等地的人不得用 Site/Platform 交易；明确禁止用 VPN 绕过。〔文档〕[L-tos]
- **抓取与自动化（§4.2）。** 禁止“Harvest or otherwise collect information … for any unauthorized purpose”；禁止 “data mining tools, robots, crawlers, or similar data gathering and extraction tools to scrape or otherwise remove data”；禁止未经书面同意用手工方式监视或复制站点内容“for any other unauthorized purpose”；禁止造成过载。条款没有给出“授权用途”的定义，也没有另设 API 使用条款。〔文档〕[L-tos]
- **面向机构的数据限制（§4.2）。** 若用户是 Capital Market Client（券商、做市、自营、指数计算、ETF 发行、投顾、对冲基金、金融科技公司、银行、交易所等及其关联方）或市场数据分发商，未经书面同意不得“直接或通过 API … 访问或使用”任何形式（原始、衍生、聚合、匿名化）的 Data；任何人不得把 Data 出售、转授权、再分发给这两类主体。〔文档〕[L-tos]
- **许可（§5.1）。** 授予个人、有限、可撤销、不可转授权、不可转让的许可，用于访问和使用站点及功能。〔文档〕[L-tos]
- **信息不作保证（§2.2）。** 条款称 Content Features 只供参考，不保证及时与完整。〔文档〕[L-tos]
- **geoblock 只针对下单。** 文档写明 “Polymarket restricts order placement from certain geographic locations”，`GET https://polymarket.com/api/geoblock` 的 `blocked` 表示“是否被禁止下单”；分 OFAC 完全封锁、前端与 API 都 close-only、只在前端 close-only 三组。文档没有说读接口受地域限制。实测出口为美国 `blocked: true`，Gamma、CLOB、Data API 读请求均成功；polymarket.com 页面显示 “Continue in view only mode” 并引导到 polymarket.us。〔文档〕〔实测〕[D-geo]、[E-geoblock]、[L-tos-source]
- **两份名单不一致。** 条款交易限制列出匈牙利、斯洛文尼亚、爱尔兰、荷兰；geoblock 文档把爱尔兰、荷兰列为“仅前端 close-only、API 不受限”，没有列匈牙利与斯洛文尼亚。条款另说受限名单以 geoblock 文档页为准。对只读接入影响有限，但不能用其中一份代表另一份。〔文档〕[L-tos]、[D-geo]
- **Polymarket US。** 官方把 Polymarket（国际，链上）与 Polymarket US（法币，CFTC 监管的 DCM/DCO）区分为两个产品。US 的公共 API `https://gateway.polymarket.us` 不需要 key，提供 markets、events、series、sports、search 与价格历史（`symbol` 为 market slug）；交易 API `https://api.polymarket.us` 需要 KYC 后申请的 Ed25519 key。US 文档的限速按源 IP 与 Cloudflare 节点计，“Retail API” 总体 25 req/s，超限返回 429；该数值是否同样适用于公共 gateway，文档没有分开说明。实测 `/v1/search?query=fed` 返回 200，event 为 `fedfnds-ondt-2027-12-31` 这类与国际版不同的 slug 与 id。国际版的条款是否适用于 US 数据、US 是否另有数据条款，本次没有查。〔文档〕〔实测〕[US-what]、[US-intro]、[US-auth]、[US-rate]、[US-search]、[E-us-search]

## 其他相关能力

- **评论**：Gamma `GET /comments` 按 `parent_entity_type`（`Event`、`Series`、`market`）与 `parent_entity_id` 列评论，offset 分页，支持 `order`、`holders_only`、`get_positions`；实测返回正文、时间与用户 profile。event 对象有 `commentCount`。〔文档〕〔实测〕[S-gamma]、[E-comments]
- **相关市场**：没有“相关 market”端点。可用的关联是 event 内的 market、`series`（周期性 event，实测 `nfl`、`nba` 为 `daily`）、tag 与 related-tags 图，以及体育赛事的 `gameId`（主 event 与 `-more-markets`、`-halftime-result` 等伴随 event 共享）。〔文档〕〔实测〕[D-discover]、[E-series]
- **市场活动**：Data v2 `/v2/trades`（按 `condition` 或 `event_id`）、`/v2/holders`、`/v2/oi`；`/v2/status` 报告服务水位与链上同步区块（实测 `age_seconds: 11`）。〔实测〕[E-trades]、[E-holders]、[E-oi]、[E-status]
- **体育**：体育 market 带 `sportsMarketType`（moneyline、spreads、totals 等）与 `gameStartTime`；比赛开始时自动撤单；event 上有主客队 `teams[].ordering`。体育实时比分走 `wss://sports-api.polymarket.com/ws`。〔文档〕[D-discover]、[D-concepts]、[D-api]
- **官方 SDK 与工具**：统一 SDK 为 `Polymarket/py-sdk` 与 `Polymarket/ts-sdk`；旧的 `py-clob-client`、`clob-client`、`rs-clob-client` 已归档，替代为 `*-clob-client-v2`。官方 Rust CLI `Polymarket/polymarket-cli`（MIT，自述为实验性）依赖 crate `polymarket_client_sdk_v2 0.5.1`，无钱包即可 `markets search`、`markets get`。〔源码〕[G-repos]
- **Polymarket Agents 与 MCP**：`Polymarket/agents`（“Trade autonomously on Polymarket using AI Agents”）已归档，最后推送 2024-11-05。`Polymarket/agent-skills` 提供一个 agent skill，但其中合约地址、抵押品与 SDK 仍是 2026-04-28 CLOB V2 升级之前的内容。docs.polymarket.com 的 `llms.txt` 索引中没有官方 MCP 服务器条目。〔源码〕〔文档〕[G-repos]、[D-llms]、[D-changelog]

## 文档与实测的矛盾

| 主题 | 文档 | 实测 | 处理 |
|---|---|---|---|
| 显示价格 | 价差 > $0.10 时显示最近成交价 [D-prices] | 价差 0.627 时 SSR 页面显示 midpoint（63%，不是 46%）[E-wide-web] | 单样本；水合后未核实 |
| `/price` side | 指南：BUY = 最低卖价 [D-books]；参考：BUY = 最优买价 [D-price-ref] | BUY = 最优买价 [E-price-buy] | 以参考与实测为准 |
| CLOB 历史端点 | changelog：v2 替代 CLOB 路由 [D-changelog] | CLOB `/prices-history` 仍 200 [E-hist-clob-1d] | 视为将弃用 |
| Data API v1 | 2026-10-24 退役 [D-migrate] | 当日 04:50 UTC 仍 200 [E-v1-trades] | 不依赖 v1 |
| 限流方式 | 总述：限流、不拒绝 [D-rate]；Data v2：429 + `Retry-After` [S-data] | 未触发 | 两种都要处理 |
| 限流执行层 | Cloudflare [D-rate] | Gamma 响应头只见 CloudFront 与 envoy [E-search-fed] | 未确认 |
| 批量价格类型 | 示例为数字 [D-books] | 字符串 [E-prices-post] | 解析时两者都接受 |
| `GET /prices` | 支持 `token_ids` + `sides` [D-books] | 400 `Invalid payload` [E-prices-get] | 用 POST |
| 搜索参数 | 列出 `events_status`、`sort`、`keep_closed_markets` 等 [D-search] | `sort=volume24hr` 与 `events_status=closed` 无可见效果 [E-search-sort]、[E-search-election-closed] | 只依赖 `q`、`page`、`limit_per_type`、`events_status=active` |
| 受限地区 | 条款与 geoblock 名单不同 [L-tos]、[D-geo] | — | 不影响只读 |

## 补充实测（04:58–05:10 UTC）

以下请求在同一出口上发出，结果汇总在 [补充实测][E-followup]。

- **空订单簿的占位价。** Protocol V2 market（`version: "v2"`，例如 id `5931597`）同时带 JSON 编码的 `clobTokenIds` 与数组 `positionIds`，两者是同一对 id。它的订单簿 `bids` 与 `asks` 都为空，但 CLOB `/midpoint` 仍返回 `{"mid":"0.5"}`，`/book` 的 `last_trade_price` 为 `"0.500"`，Gamma `outcomePrices` 为 `["0.5","0.5"]`、`bestBid: null`、`bestAsk: 1`。所以 0.5 可能只是占位值，不能不看订单簿就当概率。同一 token 的 `POST /prices`（BUY 与 SELL）返回 `{"BUY":"0","SELL":"1"}`，`/spread` 返回 `"1"`；有挂单的 token 返回真实的最优买价与卖价（`0.961`／`0.963`）。单个有 147 档挂单的 `/books` 响应约 5 KB，`/prices` 每个 token 只有两个数。〔实测〕
- **`/book` 的 `last_trade_price` 与 `/last-trade-price` 不一致。** neg-risk event `51456` 中两个 market 的 Yes token，`POST /books` 的 `last_trade_price` 为 0.037 与 0.987，而同一 token 的 `/last-trade-price` 与 `POST /last-trades-prices` 都返回 0.963（`side: "BUY"`），与 Gamma `lastTradePrice` 一致。0.037 与 0.987 恰好是 1 减去对应 Yes 价格。只有这一组 neg-risk 样本；前文非 neg-risk 的 Bitcoin 样本中两者一致。〔实测〕
- **搜索翻页。** `public-search?q=bitcoin&events_status=active&limit_per_type=3` 的第 1–3 页依次给出 9 个 event，与 `limit_per_type=9` 一次返回的 9 个 id 顺序完全相同：`page` 从 1 开始，页大小就是 `limit_per_type`。这只证明在几秒之内结果稳定，不证明排序跨时间稳定。〔实测〕
- **不存在的 slug。** `/events/slug/<不存在>` 与 `/markets/slug/<不存在>` 都返回 404 `{"type":"not found error","error":"slug not found"}`。〔实测〕
- **规则文本重复。** event `51456` 的 13 个 market 的 `description` 彼此相同，开头与 event 的 `description` 也相同。〔实测〕
- **历史点数。** Data v2 `/v2/prices-history?interval=1d` 不带 `bucket_seconds` 时返回 1436 个点。〔实测〕

## 未解决的不确定性

- Gamma `outcomePrices` 的计算规则、更新频率与 CDN 最长滞后没有文档；本文只确认它等于 Gamma 自身 bid/ask 的中点、缓存最多 300 秒。
- polymarket.com 在价差大于 $0.10 时实际显示什么：一个 SSR 样本与文档矛盾，客户端渲染与 app 未核实。
- slug 是否会变更、旧 slug 是否可用（`pastSlugs` 的语义）；数字 id 是否保证稳定。
- `/public-search` 各参数的语义、`limit_per_type` 上限、排序规则与 `page` 的最大深度。
- 价格历史点的价格来源（成交价、midpoint 或其他）。
- Gamma 的限速实际由哪一层执行，超限时是排队、429 还是其他表现。
- 条款中的“授权用途”指什么，个人研究用的 CLI 是否属于“robots … to scrape”；Capital Market Client 条款对 forager 用户的影响取决于用户身份，需由用户自行判断。
- Protocol V2（`version: "v2"`）market 的价格接口：补充实测中 V2 样本的 `positionIds` 与 `clobTokenIds` 相同，CLOB `/book` 以它为 `token_id` 返回 `version: "v2"` 的订单簿；但样本的订单簿都是空的，有挂单的 V2 market 没有实测。〔文档〕〔实测〕[D-books]、[E-followup]
- `/book` 的 `last_trade_price` 在 neg-risk market 上是否总是互补价，只有一组样本。〔实测〕[E-followup]

## 对 forager 平台设计的含义（设计意见，非已核实事实）

以下是基于上述事实的设计取向，供后续写 spec 时讨论；落地前应按 `docs/spec/forager/07-platforms.md` 的接入清单逐项确认。据此写成的设计提案见 [Polymarket 平台设计](../design/2026-10-24-polymarket-platform.md)；提案根据「补充实测」改用 CLOB `POST /prices` 并要求两边都有挂单，与本节的 `/midpoints` 写法不同。

- **route**：一条匿名 HTTP route（例如 `polymarket_api`，`Credentials::Unused`），按 ADR 0027 的多端点 route 规则为 Gamma、CLOB、Data API 各设一个 URL 配置键；Gamma 是主端点，CLOB 与 Data API 失败时保留 Gamma 结果并汇总一条诊断。访问策略可以远低于文档限额，例如单 route 串行、几百毫秒最小间隔。
- **Platform Ref 候选**：
  - `polymarket:event/<event-slug>` ↔ `https://polymarket.com/event/<event-slug>`，可以往返；语言前缀与 `?tid=` 在解析时丢弃。
  - `polymarket:market/<market-slug>` ↔ `https://polymarket.com/market/<market-slug>`。该 URL 会 307 到 event 下的 market 页，因此它可以作为 canonical；`/event/<e>/<m>` 也能离线解析成这个 ref。
  - 不建议用数字 id 作 ref 主身份，因为它没有对应的站点 URL；可作为输出字段，并在 slug 查找 404 时作为后备（需联网，不能放进零 IO 解析）。`conditionId` 与 token ID 可作为输出字段，不作 ref kind。
  - slug 不能从标题推出（时间戳后缀），符合“只用 forager 返回或用户给出的 ref”的消费规则。
- **search**：Gamma `/public-search`，用 `events_status=active` 作默认、L1 选项允许包含已关闭 event；cursor payload 记录 `q`、选项与下一 `page`，`hasMore` 为 false 时 `next_cursor: null`。不传 `optimized=true`，以保留 `conditionId` 与 token。结果条目宜以 event 为单位，附每个 market 的 `groupItemTitle` 与概率。
- **fetch**：Gamma `/events/slug/{slug}` 或 `/markets/slug/{slug}` 取元数据，再用 CLOB `POST /midpoints`（一次最多 500 个 token）补实时 midpoint 与时间戳；输出同时给 `probability`（midpoint）、`best_bid`、`best_ask`、`last_trade_price`、`spread` 和取值来源，Gamma 值标明可能缓存最多 300 秒。已结算市场的 CLOB 返回 404，应改用 Gamma `outcomePrices` 与 `umaResolutionStatus`（或 Data v2 `/v2/resolutions`）表示结果，不能把残留的 bid/ask 当结果。`description` 中含结算规则，可作为 `full_text` 的原生正文候选。
- **L2 操作候选**：`history`（Data v2 `/v2/prices-history`，默认 market 的 Yes token，参数为 interval 或 ≤15 天窗口与 bucket 宽度）是最自然的第一个；`comments` 可复用已有同名操作的形状；`book`、`trades`、`holders` 价值较低，可后置。
- **概率语义**：在 skill 平台 reference 中写明“概率 = 订单簿 midpoint，不是校准过的概率；neg-risk 组的各项之和可以不等于 1；低流动性市场的 midpoint 可能远离最近成交”，并给出 bid/ask 让调用方判断。
- **条款提示**：在平台 reference 中注明 ToS §4.2 对抓取与 Capital Market Client 的限制，route 只调用官方公开 API、不抓取 polymarket.com 页面。Polymarket US 是不同身份空间，若需要应作为独立平台或独立 route 讨论，不能在同一 ref 空间混用。

[E-dir]: polymarket-2026-10-24/
[E-manifest]: polymarket-2026-10-24/source-manifest.json
[E-env]: polymarket-2026-10-24/environment.json
[D-llms]: polymarket-2026-10-24/llms.txt
[D-overview]: polymarket-2026-10-24/doc-api-reference_predictions_overview.md
[D-overview-md]: polymarket-2026-10-24/doc-market-data_overview.md
[D-api]: polymarket-2026-10-24/doc-getting-started_api.md
[D-rate]: polymarket-2026-10-24/doc-api-reference_rate-limits.md
[D-geo]: polymarket-2026-10-24/doc-api-reference_geoblock.md
[D-concepts]: polymarket-2026-10-24/doc-concepts_markets-events.md
[D-prices]: polymarket-2026-10-24/doc-concepts_prices-orderbook.md
[D-resolution]: polymarket-2026-10-24/doc-concepts_resolution.md
[D-negrisk]: polymarket-2026-10-24/doc-concepts_negative-risk.md
[D-discover]: polymarket-2026-10-24/doc-market-data_discover-markets.md
[D-details]: polymarket-2026-10-24/doc-market-data_market-details.md
[D-books]: polymarket-2026-10-24/doc-market-data_prices-order-books.md
[D-search]: polymarket-2026-10-24/doc-api-reference_search_search-markets-events-and-profiles.md
[D-price-ref]: polymarket-2026-10-24/doc-api-reference_market-data_get-market-price.md
[D-ltp]: polymarket-2026-10-24/doc-api-reference_market-data_get-last-trade-price.md
[D-hist-clob]: polymarket-2026-10-24/doc-api-reference_markets_get-prices-history.md
[D-hist-v2]: polymarket-2026-10-24/doc-api-reference_markets_get-a-tokens-price-history.md
[D-hist-batch]: polymarket-2026-10-24/doc-api-reference_markets_get-batch-prices-history.md
[D-res-state]: polymarket-2026-10-24/doc-api-reference_markets_get-resolution-state.md
[D-by-token]: polymarket-2026-10-24/doc-api-reference_markets_get-market-by-token.md
[D-data]: polymarket-2026-10-24/doc-api-reference_data-api_overview.md
[D-migrate]: polymarket-2026-10-24/doc-migrate_data-api-v1-to-v2.md
[D-changelog]: polymarket-2026-10-24/doc-changelog_predictions.md
[D-wss]: polymarket-2026-10-24/doc-api-reference_wss_market.md
[S-gamma]: polymarket-2026-10-24/spec-gamma-openapi.yaml
[S-clob]: polymarket-2026-10-24/spec-clob-openapi.yaml
[S-data]: polymarket-2026-10-24/spec-data-api-v2-openapi.json
[L-tos]: polymarket-2026-10-24/legal-tos-polymarket-com.txt
[L-tos-source]: polymarket-2026-10-24/legal-tos-page.json
[US-what]: polymarket-2026-10-24/us-doc-getting-started_what-is-polymarket-us.md
[US-intro]: polymarket-2026-10-24/us-doc-api-reference_introduction.md
[US-auth]: polymarket-2026-10-24/us-doc-api-reference_authentication.md
[US-rate]: polymarket-2026-10-24/us-doc-api-reference_rate-limits.md
[US-search]: polymarket-2026-10-24/us-doc-api-reference_search_search.md
[G-paths]: polymarket-2026-10-24/github-py-sdk-gamma_paths.py.txt
[G-paths-test]: polymarket-2026-10-24/github-py-sdk-test_gamma_paths.py.txt
[G-repos]: polymarket-2026-10-24/github-repos.json
[E-geoblock]: polymarket-2026-10-24/probe-geoblock.json
[E-search-fed]: polymarket-2026-10-24/probe-gamma-search-fed-rate.json
[E-search-election]: polymarket-2026-10-24/probe-gamma-search-election.json
[E-search-election-closed]: polymarket-2026-10-24/probe-gamma-search-election-closed.json
[E-search-p1]: polymarket-2026-10-24/probe-gamma-search-election-active-page1.json
[E-search-p2]: polymarket-2026-10-24/probe-gamma-search-election-active-page2.json
[E-search-sort]: polymarket-2026-10-24/probe-gamma-search-election-active-sort.json
[E-search-big]: polymarket-2026-10-24/probe-gamma-search-limit-big.json
[E-search-empty]: polymarket-2026-10-24/probe-gamma-search-noresult.json
[E-keyset-title]: polymarket-2026-10-24/probe-gamma-events-keyset-title-search.json
[E-keyset-101]: polymarket-2026-10-24/probe-gamma-events-keyset-limit-101.json
[E-keyset-offset]: polymarket-2026-10-24/probe-gamma-events-keyset-offset.json
[E-keyset-any]: polymarket-2026-10-24/probe-gamma-markets-keyset-closed-any-no-id.json
[E-keyset-condition]: polymarket-2026-10-24/probe-gamma-markets-keyset-by-condition.json
[E-event-slug]: polymarket-2026-10-24/probe-gamma-event-by-slug.json
[E-event-id]: polymarket-2026-10-24/probe-gamma-event-by-id.json
[E-event-doc-example]: polymarket-2026-10-24/probe-gamma-event-by-slug-doc-example.json
[E-event-missing]: polymarket-2026-10-24/probe-gamma-event-by-slug-missing.json
[E-event-resolved]: polymarket-2026-10-24/probe-gamma-event-resolved-by-slug.json
[E-market-slug]: polymarket-2026-10-24/probe-gamma-market-by-slug.json
[E-market-id]: polymarket-2026-10-24/probe-gamma-market-by-id.json
[E-tag]: polymarket-2026-10-24/probe-gamma-tags-slug.json
[E-series]: polymarket-2026-10-24/probe-gamma-series-list.json
[E-comments]: polymarket-2026-10-24/probe-gamma-comments-event.json
[E-midpoint]: polymarket-2026-10-24/probe-clob-midpoint.json
[E-midpoints-post]: polymarket-2026-10-24/probe-clob-midpoints-batch-post.json
[E-price-buy]: polymarket-2026-10-24/probe-clob-price-buy.json
[E-price-sell]: polymarket-2026-10-24/probe-clob-price-sell.json
[E-prices-post]: polymarket-2026-10-24/probe-clob-prices-batch-post.json
[E-prices-get]: polymarket-2026-10-24/probe-clob-prices-batch-get.json
[E-book]: polymarket-2026-10-24/probe-clob-book.json
[E-spread]: polymarket-2026-10-24/probe-clob-spread.json
[E-ltp]: polymarket-2026-10-24/probe-clob-last-trade-price.json
[E-by-token]: polymarket-2026-10-24/probe-clob-markets-by-token.json
[E-midpoint-resolved]: polymarket-2026-10-24/probe-clob-midpoint-resolved.json
[E-wide-spread-list]: polymarket-2026-10-24/probe-gamma-markets-keyset-wide-spread.json
[E-wide-gamma]: polymarket-2026-10-24/probe-gamma-market-wide-spread-by-slug.json
[E-wide-mid]: polymarket-2026-10-24/probe-clob-midpoint-wide-spread.json
[E-wide-ltp]: polymarket-2026-10-24/probe-clob-last-trade-wide-spread.json
[E-wide-web]: polymarket-2026-10-24/web-event-hood-ssr.json
[E-stale-r1-gamma]: polymarket-2026-10-24/probe-stale-r1-gamma.json
[E-stale-r1-book]: polymarket-2026-10-24/probe-stale-r1-clob-book.json
[E-stale-r2-gamma]: polymarket-2026-10-24/probe-stale-r2-gamma.json
[E-stale-r2-book]: polymarket-2026-10-24/probe-stale-r2-clob-book.json
[E-hist-clob-1d]: polymarket-2026-10-24/probe-clob-prices-history-1d.json
[E-hist-clob-40d]: polymarket-2026-10-24/probe-clob-prices-history-40d-range.json
[E-hist-v2-1d]: polymarket-2026-10-24/probe-data-v2-prices-history-1d.json
[E-hist-v2-40d]: polymarket-2026-10-24/probe-data-v2-prices-history-40d-range.json
[E-hist-resolved-v2]: polymarket-2026-10-24/probe-data-v2-prices-history-resolved-max.json
[E-hist-resolved-clob]: polymarket-2026-10-24/probe-clob-prices-history-resolved-max.json
[E-resolutions]: polymarket-2026-10-24/probe-data-v2-resolutions-resolved.json
[E-tokens]: polymarket-2026-10-24/probe-data-v2-tokens.json
[E-status]: polymarket-2026-10-24/probe-data-v2-status.json
[E-trades]: polymarket-2026-10-24/probe-data-v2-trades.json
[E-holders]: polymarket-2026-10-24/probe-data-v2-holders.json
[E-oi]: polymarket-2026-10-24/probe-data-v2-oi.json
[E-v1-trades]: polymarket-2026-10-24/probe-data-v1-trades.json
[E-v1-oi]: polymarket-2026-10-24/probe-data-v1-oi.json
[E-web-urls]: polymarket-2026-10-24/web-url-forms.json
[E-us-search]: polymarket-2026-10-24/probe-us-gateway-search.json
[E-followup]: polymarket-2026-10-24/probe-followup-2026-10-24T05.json
