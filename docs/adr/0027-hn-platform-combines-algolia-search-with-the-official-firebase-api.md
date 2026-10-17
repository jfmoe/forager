# Hacker News 平台用一条 route 结合 Algolia 检索与官方 Firebase API，讨论串按文件交付

forager 的第六个平台 `hn`（Hacker News）只有一条匿名 route `hn_api`。它在 route 内部结合 HN 的两个公开端点：Algolia HN Search API（`hn.algolia.com/api/v1`）与 HN 官方 Firebase API（`hacker-news.firebaseio.com/v0`），提供 search、fetch 与 Platform Operation Stories。

## 背景

2026-10-17 的实测：

- Algolia 能按关键词、类型、作者、讨论串、时间、分数检索，每个查询最多返回前 1000 条；`/items/<id>` 一次返回整棵评论树（3768 个节点约 2MB、3.8 秒）。树中子评论按 id（即发表时间）排序，不是 HN 页面的排名顺序；计数与新条目有约 3 分钟的索引延迟。每个 IP 每小时最多 10,000 次请求。
- Firebase 只能按 id 逐条读取：`kids` 是按页面排名排序的直接回复，计数是实时的，另有 top、new、best、ask、show、job 六个按排名排序的官方榜单。官方说明「目前没有限速」。没有检索能力，读一棵评论树要逐条请求。

两个端点各有对方缺少的能力：检索与整棵树只有 Algolia 有，排名、实时计数与榜单只有 Firebase 有。

## 决定

- **一条 route 访问两个端点，而不是两条 route。** 第 7 章的链语义是第一条被接受的 route 产出整个结果、不合并结果；两条 route 无法在一次 fetch 里组合 Algolia 的树与 Firebase 的排名。平台契约因此增加一条规则：一条 route 可以访问同一平台的多个官方端点，每个端点各有一个 URL 配置键，所有端点共用这条 route 的访问策略（每 0.5 秒 1 个请求、并发 1，合每小时最多 7200 次）。
- **Firebase 只增强结果。** 增强请求（排名、实时计数、删除标记、story 标题）失败时，保留 Algolia 的结果并给出诊断，不判失败。唯一例外是 Stories 的榜单列表只有 Firebase 能提供。
- **请求数保持常数。** 只为根条目取 Firebase 排名：顶层评论按 HN 页面排名排列，更深层按时间排列。每个操作最多 3 次请求，不随评论数增长；只有 Stories 在 Algolia 漏掉条目时逐条补取。
- **讨论串按文件交付。** `thread` 是 HN fetch 的默认深度，讨论串写成 Markdown 文件，stdout 只给路径与统计（ADR 0015）。平台契约原本规定非全文深度一律内联，现改为由平台为 `thread` 深度声明交付方式：route 返回 Native 正文时写文件（HN），否则内联（X）。大讨论串渲染后可达百万字符，内联会撑满调用方的上下文。
- **榜单是独立的 Platform Operation Stories。** 它不需要查询词，改变了必需输入；条目带名次，不签发 cursor（榜单随时在变，翻页无法复现）。

## 不选的方案

- **只用 Algolia**：评论按时间排列，读文件开头看不到最受认可的讨论；没有首页榜单。
- **只用 Firebase**：没有检索；读一棵几千条评论的树要几千次请求。
- **两条 route（`hn_algolia` 与 `hn_firebase`）**：链语义不合并结果，Firebase 单独作为后备时连检索都做不了。
- **嵌套回复也按排名排列**：每个有回复的节点都要一次 Firebase 请求，请求数随评论数增长。
- **抓取 HN 的 HTML 页面**：大讨论串分页，层级关系难以可靠解析，且不是官方接口。

## 影响

- 讨论串不是 HN 页面的完整镜像：不含已删除与 dead 的评论，最近几分钟的评论可能缺失；search 与 Stories 的计数可能滞后几分钟，metadata 与 thread 的计数由 Firebase 校正。skill 的平台 reference 写明这几点。
- Algolia 的每小时上限按 IP 计，被封禁后需要发邮件申诉；访问策略在多个进程之间共享并留有余量。Firebase 的「不限速」是现状，不是承诺。
- 两个端点都是匿名接口，不消耗任何额度；只有 `full_text` 读取外链文章时消耗用户配置的 Web Fetch provider 的额度。
