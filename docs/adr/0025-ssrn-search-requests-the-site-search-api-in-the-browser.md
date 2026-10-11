# SSRN 检索在用户浏览器中请求原站检索接口

本记录修订 ADR 0020 中 `ssrn_browser` search 的读取方式。ADR 0020 的其余决定不变：route 由用户自行启用；永不解或绕过站点验证，遇到验证以 Auth 失败；每个 OpenCLI 操作计一次访问，单并发，每 5 秒一个。调研与实测见 [SSRN browser 检索改用原站接口的实测](../research/2026-10-11-ssrn-search-api.md)。

## 背景

`ssrn_browser` 原本在 Chrome 中打开 `papers.ssrn.com` 的结果页并读取页面。SSRN 经常把结果页升级为需要人工勾选的 Cloudflare 验证；页面读取还依赖 DOM 选择器、渲染时序、一次性导航参数和逐项控件核对，OpenCLI 的页面跳转本身也会失败。结果页上的 Posted 日期又按浏览器本地时区渲染。

结果页自己的数据来自一个匿名只读接口：`GET https://api.ssrn.com/papers/v1/papers/search/advanced`。页面请求它时不带 cookie，也没有 token 或签名。浏览器之外直接请求会被 Cloudflare 拦截，在浏览器里请求则正常返回。

## 决定

process route 可以在用户自己的 Chrome 中，发出站点页面本身就会发出的只读请求，前提是以下条件同时成立：

- 与原站页面同一端点、同一参数集合。
- 不带 cookie 或其他凭据（`credentials: 'omit'`），不构造 token、签名或验证字段。
- 不解、不绕过任何验证；请求被 Cloudflare 拦截时以 Auth 失败，由用户在 Chrome 中处理。
- Access Policy 不变：一次 OpenCLI 命令只发一次请求，仍按每个命令计一次。

`ssrn_browser` 的 search 据此改为：adapter 在不跳转的空白标签页里发出与原站相同的检索请求，交回实际请求地址、HTTP 状态、content type 与响应体；route 核对请求地址中的每个条件，再解码、归一化与翻页。fetch（详情页、摘要与全文下载）仍读取页面。

## 与其他浏览器 route 的区别

本决定不是对浏览器 route 自行构造请求的普遍放开。

- **ChatGPT（ADR 0024）**：`backend-api` 需要账号会话、access token 和反滥用字段（`chat-requirements`、工作量证明、Turnstile）。自行请求意味着使用或构造这些凭据与字段，因此 `chatgpt_browser` 只读取页面自己收到的响应。
- **小红书**：搜索与评论接口要求前端在真实浏览器中生成签名与指纹（`a1`、`b1`）。自行请求就要逆向或调用签名逻辑，因此 `xiaohongshu_browser` 只读网络抓包与页面状态。
- **SSRN 检索接口**：原站匿名使用的公开只读 GET，不需要会话、凭据、签名或验证字段，满足上文全部条件。

其他站点要采用同样的做法，须先确认满足上述条件，并在新记录中写明。

## 影响

- 检索不再加载 SSRN 页面，页面级验证、页面渲染和页面跳转失败不再影响检索。2026-10-11 实测：同一时段内详情页要求人工验证、fetch 以 Auth 失败时，接口检索照常返回结果。
- 检索结果的 `posted` 按接口给出的 UTC 时间取日期，与详情页一致，不再随机器时区变化。
- adapter 契约升为 `forager-ssrn/4`。旧 adapter 由契约版本检查拒绝并提示重新安装。
- 该接口没有公开文档，随时可能变化。契约版本、请求地址核对与严格解码用于发现变化；无法识别的响应以 Runtime 失败，不返回假空页。
- SSRN 使用条款的风险判断沿用 ADR 0020。访问强度不高于原做法：每个操作一次接口请求，而原来是一次完整页面加载及其全部子请求。
