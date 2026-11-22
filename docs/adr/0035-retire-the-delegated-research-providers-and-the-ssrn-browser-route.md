# 退役 ssrn_browser、gemini_browser 与 chatgpt_browser，OpenCLI 冻结

forager 删除三个经本机 OpenCLI 驱动用户 Chrome 的供应方，以及只为它们存在的命令、参数、输出字段、配置、概念与机制：

- `gemini_browser`、`chatgpt_browser`：`forager gemini` 与 `forager chatgpt` 命令、Delegated Research Report 概念与委托研究基础设施整体退役，不改接官方 API。只为"不属于任何平台、只拥有操作的 process 供应方"存在的规则一并删除：这类注册类别、doctor 默认跳过与 adapter `status` 探针、前台窗口模式、会改变账号状态的写操作只执行一次及其"结果未知"归因。
- `ssrn_browser`：SSRN 平台保留，search 与 fetch 只剩默认 route `ssrn_crossref`。只有原站 route 能执行的检索条件、只有它填写的条目字段、SSRN 正文（`full_text`）以及只为 SSRN PDF 存在的本地文件正文来源随之删除。平台 fetch 的正文来源只剩 URL 与 Native（ADR-0022）。

`xiaohongshu_browser` 与 OpenCLI transport 保留。旧命令、参数、配置段、环境变量、order 中的旧 route id 与旧 SSRN cursor 由现有机制拒绝，不保留别名或兼容层（沿用 ADR-0030、ADR-0034）。`config unset` 不认识已退役的键，用户须手工编辑配置文件删除它们。

## 依据

- **Deep Research 维护成本高。** 两个委托研究 adapter 的页面读取、抓包完成条件和点击路径只能靠 live 实测证明，没有 live smoke 用例：live 用例要么读取用户的私人会话，要么消耗 Deep Research 额度。实测还会撞上 Google 异常流量页、Browser Bridge 随机丢失响应体、`Navigation rejected` 等问题，每次排查都受页面打开预算约束。
- **OpenCLI 抢占前台。** 运行时会操作用户正在使用的 Chrome；发起 Deep Research 更必须使用前台窗口。这干扰日常使用。
- **OpenCLI 是外部依赖。** 它需要全局 npm 安装 OpenCLI 2.x，并手动更新 Browser Bridge 扩展；版本漂移会让 route 失效。
- **`ssrn_browser` 不可靠。** SSRN 的 Cloudflare 验证经常拦截它。它在 OpenCLI 1.8.6 时期做过对比实测与验收，迁到 OpenCLI 2.x 后没有 live 用例，也没有再实测。

## SSRN 只经 Crossref 的代价

依据 2026-09 两轮对比实测（[route 对比](../research/2026-09-26-ssrn-route-comparison.md)、[检索对比](../research/2026-09-28-ssrn-search-comparison.md)）：

- 已知论文命中接近：前 10 命中 Crossref 7/10、browser 8/10；MRR@20 Crossref 0.575、browser 0.481。
- 两条 route 前 20 条的重合中位数只有 6 条，Crossref 还会漏掉没有 DOI 的论文；有 DOI 的近期论文，Crossref 登记与 SSRN 发布相差 0–1 天。

用户因此失去 SSRN 正文、精确的 Posted / Last revised / Date Written 日期、只有原站支持的检索条件，以及只有原站能搜到的那部分候选。默认配置下 SSRN 检索的行为不变：`ssrn_browser` 从未进入默认 order（ADR-0020）。

## 保留 xiaohongshu_browser 作为过渡

ADR-0036 取代本节：`xiaohongshu_browser` 原地改用 forager 自带的浏览器传输与 Managed Browser，OpenCLI transport 与 process route 的配置形状已删除。

小红书没有公开 API，暂时没有替代路线。抢占前台与外部依赖对 `xiaohongshu_browser` 同样成立，所以它只是过渡保留，计划以后用无头浏览器方案替换；在此之前它与 OpenCLI transport 一起保留，process route 的配置形状（`command`、`timeout`）、退出码映射、process 访问策略与 opt-in 启用规则继续服务它。

## OpenCLI 冻结

ADR-0036 取代本节：OpenCLI 已整体退役，浏览器类 route 走 forager 自带的浏览器传输。

OpenCLI 只服务 `xiaohongshu_browser`，直到无头浏览器方案替代它。不再新增 OpenCLI route 或 adapter；以后的浏览器类 route（包括 bilibili 字幕）走无头浏览器方案。

## 取代范围

- 本记录整体取代 ADR-0023（委托研究报告与只由点名命令运行的 process 供应方）、ADR-0024（接受 OpenAI 使用条款风险的条件）与 ADR-0025（SSRN 检索在浏览器中请求原站接口）。
- 本记录取代 ADR-0020 中"正文由本地文件转换"一节，以及其 Consequences 中关于本地文件来源的条目。ADR-0020 的 opt-in 与 process 访问策略原则、ADR-0022 的 Native 来源继续有效。
- 带日期的设计与调研文档作为历史记录保留。

## 重新评估的条件

- **委托研究**：出现不抢占前台、不依赖外部浏览器驱动的接入方式，且确有需求。
- **SSRN**：无头浏览器方案就绪，且确实需要 SSRN 正文、精确日期或原站独有的候选。
