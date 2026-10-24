# Polymarket 平台用一条 route 结合三个官方 API，概率只取买卖两边都有挂单的中点

forager 的第八个平台 `polymarket` 只有一条匿名 route `polymarket_api`，默认启用。它在 route 内部结合 Polymarket 的三个公开读接口：

- Gamma（`gamma-api.polymarket.com`）：发现与元数据；
- CLOB（`clob.polymarket.com`）：实时买卖价与最近成交；
- Data API v2（`data-api.polymarket.com/v2`）：价格历史。

平台提供 search、fetch 与 Platform Operation Price History。设计见 [Polymarket 平台设计](../design/2026-10-24-polymarket-platform.md)，事实与证据见 [Polymarket 接入调研](../research/2026-10-24-polymarket-integration.md)。

## 背景

2026-10-24 的文档核对与匿名实测：

- **三个接口的读请求都不需要凭据。** geoblock 只限制下单，不限制读取。
- **Gamma 的价格可能滞后。** `outcomePrices` 等于 Gamma 自己 bid/ask 的中点，响应经 CDN 缓存最长 300 秒。
- **空订单簿会给出假数。** 订单簿为空时，CLOB `/midpoint` 与 Gamma 都给出 0.5。这是占位值，不是交易者的判断。
- **官方显示规则不可靠。** 文档说价差超过 $0.10 时网页改显示最近成交价，但唯一的页面样本在价差 0.627 时仍显示中点。
- **neg-risk 组里有不交易的占位 market。** 它们未关闭、`active` 为 false，却仍 `acceptingOrders: true`；没有缓存价格，CLOB 也没有订单簿。`presidential-election-winner-2028` 的 128 个 market 中有 74 个是这样。
- **CLOB 的两种最近成交价不一致。** neg-risk market 上，`/books` 的 `last_trade_price` 是互补价，与 `/last-trade-price` 不一致。
- **只有 slug 有网页 URL。** polymarket.com 的 URL 只含 slug，数字 id 没有对应页面。没有文档说明 slug 是否会变；已有实现提醒 slug 可能被重新创建的 event 复用。
- **条款限制针对抓取与机构用途。** ToS §4.2 禁止用 robots 或 crawlers 抓取站点；Capital Market Client 与市场数据分发商未经书面同意，不得经 API 使用数据。

## 决定

- **概率只取买卖两边都有挂单的中点。** Market-Implied Probability 是一个结果在订单簿两边都有挂单时最优买价与卖价的中点。
  - 任一边没有挂单、市场停止交易时，概率为 `null`，并写明原因；
  - 未关闭而 `active` 为 false 的 neg-risk 占位 market 算作停止交易（`suspended`），不请求 CLOB。`active` 只对未关闭的 market 读取，因为已结算 market 的 `active` 仍为 true；
  - 已结算的市场给出兑付值，标为结算，不称为概率；
  - forager 同时交付 bid、ask、价差与最近成交，由调用方判断宽价差市场的可信度，不替它在中点与成交价之间选择。
- **一条 route 访问三个端点。** 链语义只取第一条被接受的 route 的结果、不合并，而 fetch 要把 Gamma 的结构与 CLOB 的实时报价组合在一起，所以沿用 ADR 0027 的多端点 route 规则，每个端点一个 URL 配置键。
  - CLOB 只用来增强结果：它失败时，fetch 改用 Gamma 的缓存报价，按同一条规则计算，标明来源；
  - search 只用 Gamma 的价格，保持一次请求；
  - Price History 只有 Data API 能提供，它失败即命令失败。
- **event 为主，market 可直达。** Platform Ref 有两种 kind，与网页 URL 层级对应：`polymarket:<event_slug>`（默认）与 `polymarket:market/<market_slug>`。search 每个 event 一条，保留互斥 market 组的整体。
- **ref 用 slug，链上身份作输出字段。** 数字 id 作 ref 时，用户贴来的网页链接要联网才能解析，按平台契约只能拒绝。`event_id`、`market_id` 与 `condition_id` 随每次 fetch 输出；程序化消费方以 `condition_id` 为 market 的存储键，`event_id` 为 event 的存储键。
- **默认启用。** route 只调用官方明确声明不需要 key 的公开读接口，不读页面，访问节奏远低于官方限额。条款的两条限制写进 skill 的平台 reference，由用户按自己的身份判断。
- **价格序列与结算值分开。** Price History 的序列只含市场价格；市场已结算时，兑付值单独给出。平台没有说明历史点取的是成交价还是中点，所以它不称为概率历史。

## 不选的方案

- **直接用 Gamma `outcomePrices`**：一次请求就够，但会交付滞后最多 5 分钟的值，也会把空订单簿的 0.5 当成概率。
- **复刻网页显示规则**（价差超过 0.10 改用最近成交价）：规则与实测矛盾；最近成交可能是几天前的，会把旧价格当作当前概率。
- **market 为主或只有 event**：前者丢掉互斥 market 组，一个 13 选 1 的 event 会占满 search 的 limit；后者让用户贴来的 market 链接失去精度，history 只能按标签匹配 market。
- **由用户手动启用**（同 `youtube_web`）：把条款判断明确交给用户，但第一次使用要多一步配置。`youtube_web` 读的是未公开接口、违反条款，这里调用的是官方为开发者公开的接口，两者情况不同。
- **查询扩展与本地相关性过滤**：Gamma 搜索的相关性差，但判断相关性、换词重搜属于调用方；在平台 search 里维护启发式规则，会让结果不可预期。

## 影响

- 低流动性市场的概率常为 `null`，调用方要读 `basis` 与 bid/ask，不能假定每个结果都有数。
- neg-risk 组各 market 的概率之和可以不等于 1，forager 不归一化。
- search 的价格可能滞后最多 5 分钟，要拿实时值就运行 fetch。
- slug 失效时 fetch 报条目不存在，调用方需要重新检索；跨时间引用同一市场时应对比 `event_id`。
- 三个接口都匿名，不消耗任何额度；Polymarket 可以随时调整限速，ToS 保留了这项权利。
- Polymarket US 是身份空间不同的另一个产品，不在本平台内。
