# 退役主搜索 openai_compatible 供应方

forager 删除主搜索的 `openai_compatible` 供应方，以及只为它存在的机制：同端点模型链（`fallback_models`）、进程内 model 断路器、SSE→HTTP 传输回退、stream/no-stream 双形状 doctor 探测、尝试记录的 `breaker_event` 字段、chain 的预运行 gate，以及 doctor 的 `config_warnings`（同故障域 fallback 警告）。旧的 `providers.openai_compatible.*` 配置键、对应的 `FORAGER_*` 环境变量与 `doctor --provider openai_compatible` 按未知输入报错，不保留别名或兼容层。

Main Search 仍是 Capability Seam：Capability Catalog 保留 `main_search` 条目，`search.backends` 保留，合法值只剩 `xai`。在出现新的主搜索供应方之前，主搜索没有 backend 级 fallback：xai 失败即主搜索失败，按现有终态归因与退出码报告。

## 依据

本机 2026-09-19 至 2026-10-18 的 journal（443 次 search）显示，这个 fallback 从未起作用：

- 它只在 13 次 search 中被调用，39 次 attempt 全部失败（上游返回 `Service temporarily unavailable`，归为 network），没有一次成功。
- 本机配置中它与 xai 使用同一个 endpoint 和同一个模型，与首选 backend 处在同一个故障域：xai 失败时它也必然失败。
- 为它维护的模型链、断路器、传输回退、双形状探测、clean EOF 终止语义、配置项、setup 提问与 smoke 用例，贯穿 provider、chain、config、doctor、smoke、setup 与规格文档，供应方实现本身约 900 行。

## 保留 Main Search seam

以后计划接入 gemini、codex 等主搜索候选。保留 seam、目录条目与 `search.backends`，新候选作为同一 seam 的供应方加入即可，不必重建结构。只剩一个合法值的 `search.backends` 是为此付出的代价。

## 不保留同故障域检查

`config_warnings` 只在主搜索链有两个以上 backend 时才可能非空。只剩 xai 后它恒为空，因此一并删除。将来新候选如果仍可能与首选 backend 配到同一 endpoint 和模型，再按需恢复这类检查。

## 数据的局限

本次数据只说明同故障域的 fallback 没有价值，不能说明故障域独立的 fallback 没有价值，这也是保留 seam 的原因。

## 重新评估的条件

- 出现与 xai 故障域独立（不同 endpoint 或不同模型提供方）、并经过验收的主搜索候选；
- 或 journal 显示 xai 失败已频繁到足以让主搜索需要 backend 级 fallback。
