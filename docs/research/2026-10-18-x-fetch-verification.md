# X fetch 的已读核对：实测记录

核验日期：**2026-10-18**。端点：SuperGrok 订阅网关提供的 xAI Responses API，模型 `grok-4.5`。请求与 forager 0.14.0 的 `platform x fetch` 逐字段相同（[`probe.py`](x-fetch-2026-10-18/probe.py)），完整的 `response.completed` 保存在 [证据目录](x-fetch-2026-10-18/)，[`summarize.py`](x-fetch-2026-10-18/summarize.py) 列出每个样本的工具调用、注释与帖子。xAI 官方端点没有测试。

## 结论

1. **thread 读取只为 thread 最上面的帖子生成注释。** 请求的帖子是回复时，唯一的 `url_citation` 指向它的父帖，不是它本身；只按注释核对时，回复都会判为未读（#216）。顶层帖（`x:2110126570233094527`）以及父帖不可见的回复（`x:2075228314403307854`）的注释恰好是它们自己，所以能通过。
2. **响应记录每次服务端工具调用及其参数。** 所有样本的 `output` 都有 `custom_tool_call`，`name` 为 `x_thread_fetch`，`input` 为 `{"post_id":"<请求的 id>"}`；存在的帖子 `status` 为 `completed`，不存在的帖子为 `failed`。`usage.server_side_tool_usage_details.x_posts_fetched` 与 thread 中的帖子数一致。
3. **不设 `tool_choice` 时，模型常跳过工具。** 同一条回复 `x:2066593371951390888` 的 6 次请求中，3 次只有 `reasoning`、没有工具调用，约 2 秒后返回空列表，会被当作"帖子不存在"。设 `tool_choice: "required"` 后 7 次都调用了 `x_thread_fetch`。

## 样本

| 样本 | 帖子 | 工具调用 | 注释 | 结构化输出 |
| --- | --- | --- | --- | --- |
| `fail-2096-*` | 回复 `2096318217585074319` | `x_thread_fetch`，completed | 父帖 `2096265603820540069` | root 为请求的帖子；thread 深度另有 parent |
| `reply-2098-*`、`required-2098-*` | 回复 `2098634733248720992` | 同上 | 父帖 `2098632766308880851` | parent、root、reply |
| `reply-2066-*`、`required-2066-*` | 回复 `2066593371951390888` | 同上 | 父帖 `2066587058261393906` | parent、root |
| `reply-2075-*` | `2075228314403307854`，thread 中没有父帖 | 同上 | 它自己 | root、reply |
| `ok-2110-*` | 顶层帖 `2110126570233094527` | 同上 | 它自己 | root 与 3 条 reply |
| `default-2066-metadata-3`～`5` | `2066593371951390888`，未设 `tool_choice` | 无 | 无 | 空列表 |
| `required-missing-metadata-1` | 不存在的 `2066593371951390889` | `x_thread_fetch`，failed | 无 | 空列表 |

## 对实现的影响

- fetch 的已读证据改为：一条 `completed` 的 `x_thread_fetch` 调用，其 `post_id` 等于请求的 id；或请求的 id 出现在注释中。根帖 URL 与请求一致不算证据，因为请求本身给出了这个 URL。
- fetch 请求设 `tool_choice: "required"`。
- 修改后的二进制对上表 5 条帖子的两种深度，以及另外 5 条 TeamYouTube 回复的 thread 深度，全部成功；不存在的帖子仍为 Parameter（不存在或不可见）。
- 未验证：search 是否同样会跳过工具。#218 第 2 条记录的"同一过滤条件时有时无"可能与此有关。
