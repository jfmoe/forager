# 平台原生正文是第三种全文来源

小红书 fetch（`xiaohongshu_browser`，规格第 7 章「小红书」）要交付笔记全文。ADR 0020 把平台 fetch 的全文来源穷举为两种：一组按序读取的 URL，或一个在同一 attempt 内校验过的本地文件，两者都走同一条 Web Fetch 链。本记录增加第三种来源 **Native**，取代 ADR 0020「Consequences」中"route 的全文来源只有 URL 或本地文件、平台 fetch 对两者运行同一条 Web Fetch 链"的穷举表述；ADR 0020 的其余内容不变。

## 为什么不能交给 Web Fetch

- 笔记只在用户自己的登录会话中、带 Access Token 时才能打开；匿名抓取 canonical URL 或 `access_url` 都拿不到正文（2026-10-08 实测：不带 cookie、带 token 的请求返回 200，但页面里没有该笔记）。第三方 Web Fetch provider 没有这个会话。
- route 打开笔记页时已经从服务端渲染状态中读到了结构化的标题、正文、标签与图片，并核对过 `noteId`。把同一页面再交给 Web Fetch 只会多一次访问、多一份风控风险，而且结果更差。
- 正文是纯文本加图片列表，不需要格式转换。

## 决定

- `FullTextSource` 增加 `Native(String)`，只携带 Markdown 正文。route 在取得元数据的同一个 attempt 内构造并核对它；标题、正文、图片全为空时在这个 attempt 内报 Quality，不另记 attempt。
- core 的平台 fetch 遇到 Native 来源时跳过 Web Fetch 链与薄正文门，用条目的 canonical URL、route id 与正文构造 `PlatformContent`，交给 CLI 现有的交付逻辑（文件、`content_path`、`content_len`、`--format content`、`--content-dir`、写入失败为 Runtime）。Native 来源没有原始文件，因此没有 `--keep-pdf`。
- 注册信息为每条 route 声明 fetch 全文是否由 route 自己读取（`native_full_text`）。`plan_fetch` 据此回答"本计划是否可能需要 Web Fetch"：请求 `full_text` 且计划中有任一 route 不读原生正文时为是。CLI 只在为是时做"没有已配置的 Web Fetch provider 即飞行前退 3"的预检。arXiv 与 SSRN 的 route 声明不读原生正文，行为不变。
- `content_provider` 定义为实际产出 Markdown 的 provider：Web Fetch 来源时是 Web Fetch provider，Native 来源时是 route id。`content_url` 对 Native 来源是条目的规范身份 URL，不承诺可以匿名打开。

## 接受的边界

- **token 在本机进程参数中可见**：route 以具名参数 `--xsec-token` 把 Access Token 传给 OpenCLI，同一台机器上的其他进程可以从进程列表中看到它。token 离开用户的登录会话就打不开笔记，而且它本来就出现在站点的分享链接中，所以不另建传递通道。
- **成功输出的 `access_url` 含 token**：它是打开笔记的推荐入口，调用方需要用它重新 fetch 或读取评论。除此之外 token 不进入 ref、canonical `url`、cursor、journal、attempt 消息、`--verbose` attempts 与失败时的 stderr：route 在记录 attempt 之前，按本次请求的 token 值清理来自 OpenCLI stderr、外壳解码错误与页面事实的消息。

## 影响

- ADR 0009（Web Fetch 以 provider 为先的正文契约）与 ADR 0015（结果只按引用交付已落盘的材料）的作用域不变：Web Fetch capability 仍不含平台知识，Native 正文同样写入本地文件、stdout 只给路径。
- Native 正文仍受 OpenCLI stdout 4 MiB 上限约束。
- 以后其他平台的 route 若能在同一 attempt 内读到并核对正文，可以复用 Native 来源，只需在注册信息中声明 `native_full_text`。
