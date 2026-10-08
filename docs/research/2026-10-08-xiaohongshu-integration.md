# 小红书 Platform 接入可行性

核验日期：**2026-10-08**。仓库基线：`79bd9ac3928f38230366aafb06f157851798f7cc`；本机 `forager 0.8.0`、OpenCLI `1.8.6`、Browser Bridge 扩展 `1.0.24`。本文是调研与接口草案，**尚未实现小红书 Platform**。版本、源码提交和本机命令记录见[来源清单](xiaohongshu-2026-10-08/source-manifest.json)、[命令帮助](xiaohongshu-2026-10-08/opencli-help.json)、[现场记录](xiaohongshu-2026-10-08/live-probe.json)。下文事实均以本次核验为准；建议、推断与待验证事项分别标明。

> 后续：同日登录后的实测表明，浏览器 route 只读页面自身的接口响应即可完成搜索、筛选、翻页、详情与评论，签名路线则频繁失效。实现方案改为 OpenCLI 浏览器 route，见 [小红书平台与 OpenCLI 浏览器 route 设计](../design/2026-10-08-xiaohongshu-opencli-route.md)；下文的 TikHub 推荐不再是首选。

## 建议

**完整目标的推荐方案：新增 `xiaohongshu` Platform，优先验证用户自备 key 的第三方 HTTP route `tikhub`，只采用 App V2；浏览器 route `xiaohongshu_browser` 作为个人、本机、显式启用的替代。** TikHub 的文档覆盖关键词搜索、五种所需排序、类型和时间筛选、续页、笔记详情、评论及子评论、用户已发布笔记；它接受笔记 ID，能避开浏览器路线“有稳定身份却没有访问令牌”的主要障碍。它不是官方授权 API，收费、条款及实际返回形状仍须验收；推荐的是工程路线，不能据此宣称站点许可或生产可用。[TikHub 搜索文档][TH-search]、[详情接口契约][TH-sdk]、[TikHub 条款 §§1、4、6][TH-terms]。

**最小可行方案：同一 HTTP route 先交付 search 和 `fetch --depth metadata`，保留真实作者、发布时间、互动数、标签与已验证的媒体字段；不自动补详情、不自动读评论、不下载媒体。** 搜索先验收默认条件和连续两页，再逐项开放排序、类型、时间 L1 选项；评论以显式 L2 操作加入。首期 `full_text`、`thread` 明确不支持，因为当前正文交付契约需要调整。该收缩保留平台必须提供的 search/fetch，同时不把完整笔记正文错误塞入 metadata。[平台接入契约][F-platform]、[当前 fetch 类型][F-types]、[TikHub API 定义][TH-sdk]。

建议首期 **`platforms.xiaohongshu.order = []`**：用户明确配置 `tikhub` 的 key 并把它加入 order，才会产生计费请求。浏览器 route 永远只允许用户手动加入 order，不进入 `default_order`。HTTP route 技术上可以像 Scholar 一样进入默认 order，但本次尚未验证付费接口及适用授权，不建议现在这样做。[ADR 0020][F-adr20]、[ADR 0021][F-adr21]。若用户不愿注册第三方服务，可先验证自有 OpenCLI adapter 的浏览器原型；本机目前被登录墙挡住，不能把它当成已跑通的最小 Platform 实现。[实测][E-live]。

没有找到可直接供普通 CLI 调用、覆盖任意公共笔记检索和评论读取的官方开放接口。这个结论仅针对已读取的公开目录，不排除合同合作能力或登录后额外权限。官方电商接口与广告/蒲公英接口的用途、准入见下文。[电商类目与权限][OFF-apps]、[商业开放平台][OFF-ad]。

## 路线对比

表中“支持”指文档或源码存在对应实现；只有现场记录明确成功的功能才属于本次实测可用。

| 数据源/路线 | 搜索、详情、评论、用户笔记 | 优势 | 额外成本与风险 | 建议及证据 |
|---|---|---|---|---|
| 官方电商开放平台 | 已核验目录是商品、订单、物流等；未找到任意社区笔记能力 | 文档化签名、授权和商家身份 | 资质审核、应用权限、店铺授权；业务目标不匹配 | 不作为本 Platform 来源；[类目][OFF-apps]、[授权][OFF-auth] |
| 官方聚光/蒲公英 Marketing API | 推广管理、报表、博主报价与商业数据；公开资料不足以证明任意笔记搜索、全文与评论 | 适用于已获授权的品牌营销场景 | 商业准入与 OAuth；登录后目录尚未核验；营销记录不能冒充全站内容 | 有适用合同后单独评估；[官网][OFF-ad]、[文档入口][OFF-ad-docs] |
| 自建 Web HTTP/签名逆向 | 开源代码涵盖全部目标，包括独立分页 | 控制字段和分页，Rust HTTP 运行时较轻 | 签名、会话、设备上下文和接口持续变化；需保管站点 Cookie；条款、风控与账号风险 | 不推荐首期；[MediaCrawler client][MC-client]、[Spider_XHS PC API][SP-api] |
| OpenCLI 1.8.6 内置 adapter | 搜索与详情、有限评论/回复、有限用户笔记；没有公开续页和搜索筛选参数 | 本机已装，复用 Chrome，不需要导出 Cookie | 输出缺媒体/真实发表时间/评论 ID；滚动不是可恢复分页；本机搜索遇登录墙 | 用于验证和参考，不直接当 forager route；[帮助][E-help]、[源码][OC186-search]、[实测][E-live] |
| forager 自有 OpenCLI adapter | 可按页面事实补字段、控件校验和统一外壳；分页仍需验证 | 符合现有 process transport，可固定只读命令集合 | DOM/Pinia 维护、令牌传递、新正文来源契约；登录与风控仍在 | 个人用途的 opt-in 替代；[SSRN adapter][F-ssrn]、[上游当前搜索][OC-search] |
| MediaCrawler / xhs / Spider_XHS | 底层 HTTP 接口与签名参考较完整 | 可理解接口、数据形状和失败状态 | Python/浏览器/Node 运行时；许可与账号风险；当前可用性未实测 | 参考实现，不引入主执行路径；见开源项目表 |
| xiaohongshu-mcp | Rod 浏览器读取搜索、详情及评论状态、用户主页 | Go 实现、详情字段丰富、Apache-2.0 | 自带浏览器与会话文件、独立 MCP 服务；不符合现有 OpenCLI transport；当前实现还有指纹配置 | 只参考读取逻辑；[源码][MCP-search]、[浏览器构造][MCP-browser] |
| TikHub App V2 | 文档覆盖全部目标，ID 读取与独立评论分页 | HTTP JSON、无须提交个人小红书 Cookie；供应方维护采集 | 基准 $0.01/请求；字段响应弱类型；官网自相矛盾；站点授权不保证 | 完整目标首选候选，key 自备，验收后启用；[接口][TH-sdk]、[价格][TH-product]、[条款][TH-terms] |
| Apify Actors | Actor 各异，有搜索、详情和评论，常为自动遍历后导出 Dataset | 托管执行与异步数据集 | Actor、计算/代理费和远端 Cookie 暴露；维护状态不一；Dataset 分页不等于站点分页 | 独立备选，不采用固定“万能 Apify”契约；[搜索 Actor][AP-search]、[评论 Actor][AP-comments] |
| Web Search / Web Fetch | 可发现索引中的笔记；部分公开页能读正文与图，另一些拒绝访问 | 复用现有能力，无新增平台 route | 无原生筛选/评论完整性；快照陈旧；页面导航可能被误当正文 | 显式补充发现，不跨平台 fallback；[匿名 HTTP][E-http]、[页面抽取][E-web] |
| SerpApi | 官方引擎目录未找到小红书原生引擎；可借 Google/Bing 搜索 `site:` | 可以复用已有搜索服务 | 搜索引擎排名、页码与日期不等于小红书原生语义 | 只作 Web Search 候选；[官方 API 目录][SERP] |

## 条款与合规边界

### 用户协议与社区规范

本次从官网 `/terms` 读取的中文《小红书用户服务协议》标注更新日期 **2026-03-23**，开头区分中国境内注册的小红书用户与境外 rednote 产品；因此不能仅根据当前机器位于美国推定账号适用哪份协议。官网同文也可从 `ZXXY20220331001` 读取。[官网协议][XHS-terms]、[官方协议地址][XHS-terms-cn]。

§4.1 的平台使用规范原文包含 **“反向工程、反向汇编、反向编译”**、**“非法抓取、模拟下载”**，并限制经 **“非小红书公司开发、授权、许可的第三方软件”** 登录或使用平台。完整条款还限制信息盗取、干扰和不合法的使用方式。这里保留原文短摘录及条款号；它不是一条“只要低频就准许抓取”的授权。[§4.1 原文][XHS-terms-cn]。

官方英文 Community Guidelines 要求转载取得许可并标来源，原文为 **“If you need to reprint any content, please secure permission and indicate the source.”**；还要求尊重他人隐私及版权。**本次读取的这份规范没有找到专门授权爬虫，也没有找到直接以 crawler/bot 为对象的条款**；自动化边界的证据来自用户协议和 robots，不能编造“社区规范明确禁止一切自动读取”的句子。[官方社区规范][XHS-community]。搜索候选中的备案站 `id=10` 实际是《互联网站管理工作细则》，不是社区规范；没有拿它当社区授权依据。[该页面][XHS-beian]。

某些 Actor 用“打击 AI 托管运营账号公告”作为示例笔记。本次读取该笔记只取得 `300031` 错误页，没有取得公告原文；因此不据第三方示例断言其完整规则、实施日期或对单纯只读行为的适用范围。[受限页面][XHS-policy-note]、[观测][E-web]。

**建议边界**：仅读取用户有权访问的内容；浏览器路线必须 opt-in，遇登录墙、验证码、账号安全限制立即停止并让用户在 Chrome 手动处理。不得自动解验证码、轮换代理/账号以逃避限制、制造访问令牌、发帖、点赞、收藏、评论、关注或修改账号。低频和本机登录只能减少部分技术暴露，不能消除协议限制；开源许可也只授予软件权利。[ADR 0020][F-adr20]、[平台条款][XHS-terms-cn]。

### robots.txt 原文与含义

`www.xiaohongshu.com/robots.txt` 经 Web Fetch 读取，裸域 `xiaohongshu.com/robots.txt` 经匿名 HTTP 返回同样规则；`www` 的一次本机 TLS 请求超时。通用组原文为：[当前 robots][XHS-robots]、[本地快照][E-robots]。

```text
User-agent: *
Disallow: /
```

同一文件把 Googlebot 限制为仅允许 `/worldcup26`；Baiduspider、bingbot 等命名组另外允许 `/explore`、部分 sitemap 等路径。允许命名搜索引擎访问不能外推成允许 forager 或伪装成该爬虫访问。robots 是机器访问规则，不是内容版权许可；本草案不以改 User-Agent 绕过通用组。[完整原文][XHS-robots]。

### 第三方条款

TikHub 条款更新于 **2026-03-17**：§1 明确非平台关联方，不保证其访问方法获得原平台授权；§4.2 将数据使用、隐私及原平台条款责任交给客户。§3 是内部合法业务用途许可并限制转售服务，§6 不保证具体可用率；§15.4 终止时要求删除缓存或存储数据。故官网“99.9%”宣传不能替代合同保证，BYOK 也不能把合法性责任转移给供应商。[TikHub Terms][TH-terms]、[产品宣传][TH-product]。

Apify 的 General Terms §5.8 要求客户对数据合法性负责，§11 涉及从未授权来源提取数据的责任；AUP 禁止侵犯第三方权利、虚假互动与造成过度负荷。Actor 是社区开发者维护，使用 Apify 不代表小红书授权。[General Terms][AP-terms]、[AUP][AP-aup]、[Actor 来源标识][AP-search]。

## 官方开放平台

**电商开放平台**：开发者指南要求创建应用前完成资质审核；商家自研绑定店铺主账号，并明确该流程不接受个人/个体店铺。应用类目包括商品工具、打单、订单处理、ERP、直播工具、会员通等，按类目授予 API 权限。官方授权文档采用 appId、sign、timestamp、授权码/访问令牌与刷新令牌；这些是开放平台授权体系，不能拿来签 `edith` 社区接口。[准入][OFF-register]、[类目权限][OFF-apps]、[授权协议][OFF-auth]。

**聚光/蒲公英**：官网公开展示账户管理、投放增删改查、推广报表、博主数据与私信；蒲公英优秀案例描述年流水超过 500 万客户的报价与投后洞察 API。该数字是官网场景描述，**不足以证明所有开发者一律适用同一硬门槛**。文档入口列聚光、蒲公英、私信三方、乘风及 OAuth2.0，但本次匿名读取止于目录/登录界面，没有取得完整笔记端点的权限说明。[商业平台首页][OFF-ad]、[文档入口][OFF-ad-docs]。

**结论与缺口**：可以确认官方商业 API 存在，不能确认存在面向任意公共笔记的开放 search/feed/comments API。第三方把接口叫“PGY/蒲公英”不等于它属于官方授权开放平台。若有真实商业账号，下一步应由合作方确认权限范围、笔记正文和评论的可读范围、保存/再分发许可及合同限速，不能用社区 Cookie 或抓后台页面替代审批。[官方权限表][OFF-apps]、[TikHub 非关联声明][TH-terms]。

## Web 接口与签名路线

以下是**开源调用方源码事实，不是小红书官方稳定 API 契约**，本次没有发送这些签名请求。

| 操作 | 已读取的路径与请求字段 | 响应/分页证据 |
|---|---|---|
| 笔记搜索 | MediaCrawler：`POST edith.xiaohongshu.com/api/sns/web/v1/search/notes`，`keyword,page,page_size,search_id,sort,note_type` | 调用方处理搜索结果；同一搜索 ID 和下一页位置应保存；[client][MC-client] |
| 更新的搜索路线 | Spider_XHS：`POST so.xiaohongshu.com/api/sns/web/v2/search/notes`，另含 `session_id,ext_flags,geo,image_formats`；时间/范围/距离通过非默认 `filters` | 代码称对应 2026-07 浏览器请求；本次未复现，不能推断 v1 已全局下线；[PC API][SP-api] |
| 笔记详情 | `POST /api/sns/web/v1/feed`，`source_note_id,image_formats,extra.need_body_topic,xsec_source,xsec_token` | `items[0].note_card`；ID 必须与请求对应；[client][MC-client] |
| 一级评论 | MediaCrawler 当前用 `GET /api/sns/web/v2/comment/page`，`note_id,cursor,top_comment_id,image_formats,xsec_token` | `comments,has_more,cursor`；旧 xhs 代码还有 v1 路径，版本不可混用；[MC][MC-client]、[xhs client][PY-core] |
| 楼中楼 | `GET /api/sns/web/v2/comment/sub/page`，另含 `root_comment_id,num` | 每个根评论独立 cursor、has_more；不是遍历一级评论即可自动完整；[client][MC-client] |
| 用户已发布笔记 | `GET /api/sns/web/v1/user_posted`，`user_id,num,cursor,image_formats`，当前 MC 另传 xsec 上下文 | `notes,cursor,has_more`；主页资料不是笔记列表；[client][MC-client] |

MediaCrawler 的排序枚举是 `general`、`time_descending`、`popularity_descending`；类型是 `0/1/2`（全部/视频/图文）。Spider_XHS 还实现最多评论、最多收藏以及时间 filters。它们不能证明所有取值在当前账号、当前端点上有效；不能因有名为 `note_time` 的参数就承诺筛选已验收。[MC 枚举][MC-fields]、[Spider_XHS 搜索体][SP-api]。

详情字段需要按传输来源分别解码，不能把 Web snake_case、浏览器 camelCase 和第三方包装混用。MC 存储层读取 `note_id,title,desc,type,time,last_update_time`、`user`、`image_list`、`tag_list` 和 `interact_info` 中的 liked/collected/comment/share_count；图片优先用 url_default，视频由专门提取逻辑处理。浏览器 MCP 类型则是 `noteId,imageList,interactInfo` 等形状。评论可有 id/content/create_time/user_info/like_count/pictures/target_comment，以及内嵌子评论和未加载回复的分页状态。以上证明字段提取有源码先例，不证明当前所有页面都返回全部字段；发布时间和更新时间、根评论与实际回复目标都应分开。[MC note/comment 投影][MC-store]、[浏览器类型][MCP-types]、[评论分页][MC-client]。

### 签名、Cookie 与访问上下文

ReaJason/xhs 的 client 让外部 signer 接收 URI、请求数据、`a1` 和 `web_session`；示例使用 Playwright 在页面调用 `window._webmsxyw(url,data)` 取得 `X-s/X-t`。辅助实现还生成 `x-s-common`，涉及 a1、时间、浏览器/本地存储字段。GET query 编码、POST 序列化与参与签名的数据必须一致；照抄一个签名值或只换 Cookie 不能形成可靠客户端。[xhs client][PY-core]、[签名服务示例][PY-sign]、[辅助签名][PY-help]。

MC 当前 `playwright_sign.py` 的名字虽保留 Playwright，实际内容已使用 **`xhshow>=0.2.0` 的纯算法**产生 `x-s,x-t,x-s-common,x-b3-traceid`；浏览器仍用于取得登录态，README 说明 CDP 可接已有 Chrome。不能仅凭文件名或旧教程描述它为“每次调用浏览器签名”。[当前 signer][MC-sign]、[依赖与运行方式][MC-readme]。

Spider_XHS 的认证源码要求服务器发放的 `web_session` 和 `a1`，指出 web_session 是 HttpOnly；它另维护设备/会话相关状态和 Node 签名代码。`a1` 是签名上下文的重要组成，`web_session` 是登录会话，两者不能简单当作同一个 API key。`xsec_token` 来自搜索/分享/主页上下文，供具体笔记/页面访问；它不是 note ID，也不是能由 ID 纯函数计算的稳定身份。[认证与状态源码][SP-auth]、[请求签名源码][SP-params]、[详情请求][MC-client]。

**维护与风控**：MC 显式处理 HTTP 401/403/429、461/471 验证状态及业务安全错误；OpenCLI 源码识别 `300017/300031`、安全限制与访问链接异常。近期 OpenCLI issue 还报告登录账号的直接搜索 URL 跳验证码页、AI 搜索布局改变、重复筛选控件；这些是报告人的观测，不是本机复现。只读请求也已有账号违规报告，不能估算封号概率或保证某个间隔安全。[MC 请求层][MC-client]、[风险检测][OC-risk]、[搜索故障 #2562][OC-issue2562]、[账号报告 #842][OC-issue842]。

建议不在 Rust 内维护签名算法，不引入 Cookie 轮换、自动代理切换或指纹伪装作为恢复策略；把正式授权接口或可审计的用户操作入口作为边界。若以后确需直连，签名/会话维护和合法授权必须作为独立项目验收，不能藏进一个普通 HTTP adapter。

## 浏览器 process route

### 本机 OpenCLI 1.8.6 的真实命令面

已执行 `opencli list`、`opencli xiaohongshu --help` 及逐命令帮助。安装文件与上游 `v1.8.6` 的 search/note/comments/user/download/note-helpers 六个文件 SHA-256 一致；本机帮助不能用上游 main 的新参数补全。[帮助投影][E-help]、[文件比对][E-manifest]。

| 命令 | 参数与输出 | 重要边界 |
|---|---|---|
| `search <query>` | `--limit` 默认 20，源码限制 1–100；`rank,title,author,likes,published_at,url` | DOM 搜索与有限滚动；无 sort/type/time/page/cursor；JSON 还可能包含 DOM 抽取的 `author_url`，columns 不是完整类型契约；[源码][OC186-search] |
| `note <note-id>` | 实际要求含 xsec 上下文的完整 URL；返回 `{field,value}` 行：title、author、content、likes、collects、comments、可选 tags | 没有媒体、note ID、作者 ID 或真实发表时间字段；不是 JSON note_card；[源码][OC186-note] |
| `comments <note-id>` | `--limit` 默认 20、最多 50 个根评论；`--with-replies` 默认 false；rank、author、userId、profileUrl、text、likes、time、is_reply、reply_to | 滚动最多三轮，回复展开最多三轮；无 comment ID、cursor、has_more；1.8.6 的 reply_to 统一指向根评论作者，不能重建精确回复到回复关系；[源码][OC186-comments] |
| `user <id>` | 用户 ID/主页 URL；`--limit` 默认 15；id、title、type、likes、url | 读 `__INITIAL_STATE__.user` 并有限滚动；没有独立续页 token；[源码][OC186-user] |
| `download <note-id>` | 完整 URL或 xhslink 短链；`--output`；输出 index、type、status、size | 确有从页面状态/DOM提取图片、视频流的代码，但命令会执行下载，不是只返回媒体 URL 的详情 API；本次未运行；[源码][OC186-download] |

一个易产生错误事实的地方：**search 的 `published_at` 来自 note ID 前 8 个十六进制字符推算时间，再加 UTC+8；不是平台返回的发布时间。** forager 的 `published` 不应直接接收这个字段；没有真实 time 时留空，最多另标 `id_time_estimate` 和“估计”，首期建议不输出这个估计。[`noteIdToDate`][OC186-search]。

### 当前上游与自有 adapter 的差异

浅克隆 main 的提交为 `24136945847afbfad266c6c46a8cd335377f9112`；它的 search 已包含 `--sort`、`--note-type`、`--publish-time`、`--scope`、`--location` 的可见控件映射与选择状态检查。它仍返回采集后的列表，没有公共页游标；最新 issue 报告部分布局下筛选仍失败。**有新参数不等于应该直接升级后当稳定 route**。[固定源码][OC-search]、[故障 #2576][OC-issue2576]。

当前 main 的 note/comments/download 共享风险处理：导航后等 2–5 秒，遇安全限制默认冷却 8–18 秒后重载一次；1.8.6 没有这段恢复。该逻辑未提供跨命令累计访问控制，相关 issue 仍讨论会话层限速。forager 自有 adapter 建议安全状态直接终止，不把上游自动重载策略复制进来。[风险 helper][OC-risk]、[会话限速 issue][OC-issue2208]。

建议以独立 site 名 `forager-xiaohongshu` 安装自有 adapter，提供 `contract/search/note/comments/replies`，输出 `contract/status/data` 的版本外壳；只登记读取命令，不调用写入 adapter。复用 OpenCLI 的 Browser Bridge，采用 DOM/可信页面状态；允许观察页面正常产生的请求以校验参数和分页，**不手工伪造签名或主动调用绕过页面门控的接口**。该方案沿用 SSRN 公开的 adapter 契约思想，但其分页和字段提取尚未实现。[现有 SSRN adapter][F-ssrn]、[process transport 契约][F-platform]。

搜索应每次重设默认筛选，核对实际选中项、结果身份与页面是否就绪；对经典布局和 AI 布局分别验收。DOM 虚拟列表会移除旧卡片，仅读滚到底后的 DOM 会漏首屏；当前 1.8.6 特意先读取首屏再滚动。[本机版本源码][OC186-search]、[新布局报告][OC-issue2562]。

**分页建议**：只有取得可恢复的原生 page/search_id 或评论 cursor，并验证同条件续页，才公开 next_cursor。不要把“滚动停止”“采够 limit”“显示 50 条评论”当成终页。若浏览器只能重开搜索后滚到第 N 项，它最多是有限列表原型，应明确没有可靠续页；不能伪造可恢复 cursor，也不声称评论读全。[forager 游标契约][F-platform]、[内置滚动代码][OC186-comments]。

## 开源项目核验

均按 `gh repo clone OWNER/REPO "$(mktemp -d)/REPO" -- --depth 1` 等价方式浅克隆至系统临时目录，未安装依赖或运行它们。下表“最后提交”指此次克隆的默认分支；GitHub pushed_at 可能来自其他引用，不代表默认分支代码更新。[提交与元数据][E-manifest]。

| 项目 | 当前实现与范围 | 维护证据 | 许可证与采用判断 |
|---|---|---|---|
| NanmiCoder/MediaCrawler | Playwright/CDP 登录；httpx 调 Web API；当前 signer 是 xhshow 纯算法；搜索、详情、评论/子评论、创作者笔记 | 默认分支 `93c405f`，2026-10-08；10-06 仍有用户主页解析失败报告；不是本次成功性测试 | **NON-COMMERCIAL LEARNING LICENSE 1.1**：学习研究、禁止未获书面同意的商业用途及大规模爬取；不把代码复制进 forager；[LICENSE][MC-license]、[client][MC-client]、[#990][MC-issue990] |
| ReaJason/xhs | Python requests client，外部 Playwright JS signer；部分自带签名辅助；旧 API及分页 | 默认分支 `f4b62d9`，2025-07-01；README 自称 Python 练习项目并提醒授权问题；未证明 2026-10 仍可用 | MIT；签名服务、浏览器和旧协议维护由使用方承担；[README][PY-readme]、[client][PY-core]、[LICENSE][PY-license] |
| xpzouying/xiaohongshu-mcp | **Go Rod/CDP 浏览器自动化，不是 Playwright**；读 `__INITIAL_STATE__`，详情有图片、视频、time、评论 cursor/hasMore；有可见筛选 | 默认分支 `a5c8f77`，2026-09-23；repo pushed_at 为 10-05；本次未运行 | Apache-2.0；额外服务、内置浏览器、Cookie文件与指纹配置成本；不能直接当现有 OpenCLI route；[search][MCP-search]、[types][MCP-types]、[browser][MCP-browser]、[LICENSE][MCP-license] |
| cv-cat/Spider_XHS | Python requests + Node JS 签名；PC API、创作者及蒲公英接口；最新搜索用 so/v2 | 默认分支 `ebb6c4f`，2026-09-27；代码、注释提供近期接口迁移线索；未做 live 验证 | README badge 写 MIT，却同时写禁止商业化，浅克隆没有 LICENSE，GitHub license=null；**授权不明确，不能按 badge 当 MIT**；[README][SP-readme]、[API][SP-api]、[元数据][E-manifest] |

MediaCrawler “仍维护”和 MCP “字段齐全”都不能推出整个链路仍可用；近期问题仅证明失败类型存在，不能推出所有用户都会失败。本次唯一 OpenCLI站内搜索是 Auth 失败，其他候选没有 live-provider 成功证据。[本机记录][E-live]、[MC 风控报告][MC-issue915]。

## 第三方 API 技术与费用

### TikHub App V2

Base URL 为 `https://api.tikhub.io`，鉴权是 `Authorization: Bearer <user-key>`。调用方不需要传个人小红书 Cookie；SDK的 App V2 参数集合也不要求它。这仅描述对客户的 API 契约，供应方怎样取得底层内容尚未独立核验。[使用指南][TH-guide]、[固定 OpenAPI][TH-sdk]。

**版本冲突**：2026-06-29 的官方指南称 App V1、Web V2/V3 于 06-17 永久下线，App V2 是唯一推荐系列；当前产品页却仍把 Web V3 列为维护中的备选。两份一手资料不一致，不能任选一份宣称现状确定。推荐只验收 App V2，并向供应方确认 Web V3；不把它加入 fallback。[迁移公告][TH-guide]、[当前产品页][TH-product]。

| GET 端点（均在 `/api/v1/xiaohongshu/app_v2/`） | 参数、字段与实现注意点 | 证据 |
|---|---|---|
| `search_notes` | keyword；page 从 1 起；sort_type、note_type、time_filter；续页保留首次的 search_id、search_session_id；source/ai_mode 建议固定，不透传 | [端点文档][TH-search] |
| `get_image_note_detail` | note_id/share_text 二选一；文档说明图文和视频都能取得基础详情，但视频只有封面、没有播放链接 | [当前 OpenAPI][TH-sdk] |
| `get_video_note_detail` | 视频详情/播放链接；类型未知时先调图文详情，确认 video 后再调此端点，可能产生两次费用 | [当前 OpenAPI][TH-sdk] |
| `get_note_comments` | cursor、index 首次为 0、pageArea、sort_strategy；续页恢复返回的 cursor/index/pageArea；文档建议 latest_v2，default 可能丢失或重复评论 | [当前 OpenAPI][TH-sdk] |
| `get_note_sub_comments` | 必填 comment_id；首次 index=1；续页状态是 `data.data.cursor` 对象中的 cursor 和 index，不能把整对象塞到字符串参数 | [子评论端点][TH-subcomments]、[OpenAPI][TH-sdk] |
| `get_user_posted_notes` | user_id/share_text、cursor；指南示例取最后一条 note 的 cursor，但其他描述又说 note_id，需用真实响应验收 | [指南][TH-guide]、[OpenAPI][TH-sdk] |

搜索取值与网站所需条件能对应：综合/最新/最多点赞/最多评论/最多收藏；全部/视频/图文；不限/一天内/一周内/半年内。英文优先、直播、AI 模式虽然第三方有参数，不属于本次需求，不建议首期暴露。[搜索端点][TH-search]。

**字段置信度**：产品页给出示例 note_id/title/type、user、image_list、tag_list、interact_info；接口文档说明有正文、媒体和作者，但 OpenAPI 的成功响应只是通用 `ResponseModel`，data 未给完整强类型 schema。不能从营销示例确定每个响应的 JSON path、时间单位、图片数量、视频档位和缺失值语义；正式实现前应取得经脱敏的搜索、图文、视频、一级/二级评论、用户列表响应各一份。[产品示例][TH-product]、[通用响应定义][TH-sdk]。

**费用**：小红书产品页列 App V2 **$0.01/请求**，注册体验额度 **$0.05**；全站价格“低至 $0.001”不能套给小红书。价格页说明最小充值 $5、默认 10 RPS、按 endpoint 计价；它不是每月免费搜索额度。例：一页搜索 + 一个图文详情 + 一页根评论 + 一页子评论，按基准约 $0.04；未知类型的视频再补一次则约 $0.05，不含重试/额外页。计算是建议预算，不是报价保证。[小红书价格][TH-product]、[计费规则][TH-pricing]。

详情与评论的当前文档特别提醒：错误或不存在的 ID 有时仍正常响应，在 data 内给上游错误，**仍计费**。HTTP 200 不能直接当成功，doctor 不应默认跑付费搜索，自动 retry/fallback 也可能增加费用。[OpenAPI billing notice][TH-sdk]。产品页还提及 24 小时 cache_url；不要输出它，避免把可分享的缓存地址作为永久来源或泄露返回数据。[产品说明][TH-product]。

### Apify Actors 与 SerpApi

`kuaima/xiaohongshu-search` 的输入示例包括 search_key、filter、maxItems、scrape_detail、cookie_val；输出示例有 title/date/author/like_count/href/desec/collect_count/chat_count/tags/noteType。Pricing tab 明列 **$20/月**、一天试用，以及 CU 和代理费用；不是仅按返回条目计费。当前商店页显示社区维护、月活为 0，不能以“最后修改一天前”证明运行成功。[Actor 输入与输出][AP-search]、[当前价格][AP-search-price]。

`khadinakbar/xiaohongshu-comments-scraper` 声称无需登录，输出 commentId/parentCommentId、正文、作者、点赞、时间；其 README 只承诺公共可见、有限滚动/已展开回复，不承诺全量。当前页面明确 **Under maintenance**；列价 $0.003/评论或回复、启动 $0.00005。示例 $0.06005 对应 20 条，并非实时可用保证。[Actor 原文][AP-comments]。

`edgy_dock/xiaohongshu-rednote-scraper` 声称 Chromium 执行站点 JS、监听正常网络响应；输入要求 a1/web_session Cookie，并推荐代理。其列价启动 $0.005、笔记 $0.0015、作者 $0.004、评论 $0.0004。这是另一套运行和收费模型，还会把用户站点会话交给远端 Actor；不建议 forager 替用户导出 Cookie。[Actor 说明与价格][AP-browser]。

Apify HTTP 接入需要用户自己的平台 token，典型执行为启动 Actor run、等待终态、读取 Dataset；对服务请求可用 Bearer header，不把 token 放日志/URL。Dataset 的 offset 分页只是在翻已经产出的数据，不能自动满足 Platform 的原生搜索或评论续页语义。若接入，应明确 actor ID、版本、输入 schema、终态、收费单位、页大小和允许的续页方式。[Apify API][AP-api]、[Actor API 示例][AP-search-api]。

SerpApi 官方 API 目录和本次针对 Xiaohongshu/Rednote 的官方站搜索均未找到对应 engine；因此仅能建议使用其已有 Google/Bing 引擎做 `site:xiaohongshu.com` 发现，不能称之为“小红书 SERP API”。目录缺项不排除以后增加服务。[官方搜索 API][SERP]。

## 身份、公开页面与现场实测

### 笔记身份和 URL

OpenCLI 的 URL parser 支持 `/explore/<id>`、`/search_result/<id>`、`/note/<id>`、`/discovery/item/<id>`，当前 helper 还识别 `/user/profile/<user>/<note>`；搜索通常返回含 xsec 参数的链接，download 另可消费 `xhslink.com` 短链。TikHub 文档还列 `xhslink.cn`。这些是调用方支持形式，不是官方保证所有形式等价可访问。[OpenCLI helper][OC186-helper]、[当前 helper][OC-helper]、[第三方参数定义][TH-sdk]。

建议首期仅将观测到的 **24 位十六进制 note ID** 纳入平台身份空间，统一小写，不按前缀推断时间；允许零 IO 识别经过主机白名单校验的已知长 URL。canonical URL 统一为 `https://www.xiaohongshu.com/explore/<note_id>`，**不含 xsec 参数**；规范身份链接不保证匿名可读。需要联网才能展开的 xhslink 短链按现有契约飞行前退 2，由用户提供展开后的真实长 URL；不为接入此平台暗加网络解析到 types。[Ref 契约][F-platform]、[调用方 URL 验证][OC-helper]。

### 实测结果

全部为读取；没有运行 login、download、发布或互动命令，没有导出用户 Cookie。仅 **1 次小红书站内 OpenCLI 命令**，失败后停止。公开页另外做少量匿名 HTTP 和已有 Web Fetch；没有调用 Web 签名端点或付费 API。[站内记录][E-live]、[HTTP 记录][E-http]、[Web Fetch 记录][E-web]。

| 时间/入口 | 命令或请求 | 结果与证据边界 |
|---|---|---|
| 2026-10-08；本机 doctor | `opencli doctor` | 退出 0，daemon 1.8.6、extension 1.0.24 连接正常；不证明小红书已登录；[脱敏连接投影][E-live]、[桥接文档][OC-bridge] |
| 站内搜索；准确 UTC 在 JSON | `opencli xiaohongshu search 咖啡 --limit 3 -f json --trace off` | 退出 **77**，`AUTH_REQUIRED`，提示搜索结果被登录墙阻挡。没有结果 URL，因此没继续 note/comments；[命令与错误][E-live] |
| 11:32:01 UTC；匿名 HTTP | `GET /explore/6a6f376f000000002203370b`，无 Cookie/Authorization | 200，HTML 71,828 bytes，含真实标题与 `__INITIAL_STATE__`；证明至少这篇 token-free URL 当时匿名可达，不代表搜索、评论可用；[字段投影][E-http] |
| 匿名 HTTP；同日 | `GET /explore/69afda73000000002800b3f2` | TLS handshake timeout；不能据此判定删除/登录墙；[错误][E-http] |
| 11:33:03 UTC；匿名 HTTP | 再读第一篇并尝试投影 SSR 字段 | 读取超时，没有取得字段投影；不将第一次 HTML 中存在 initial state 推断为已验证全部详情字段；[错误][E-http] |
| 已有 Web Fetch 链；同日 | `forager fetch` 第一篇 canonical URL | 抽取到标题、作者、正文、图片及“编辑于 08-02”，评论区为加载中；没有保留图片访问地址。没有向服务提交个人小红书凭据，远端执行会话状态未知，不能等同于本机未登录浏览器结果；[投影][E-web] |
| 已有 Web Fetch 链；同日 | `forager fetch` 第二篇 canonical URL | 返回 `/404` 和 `300031`，没有正文；不能将导航/页脚当笔记，也不能直接断言该笔记不存在；[投影][E-web] |

doctor 的只读成功输出已投影到现场记录，未保存浏览器 profile 标识；OpenCLI 小红书命令帮助与源码文件哈希已落盘。匿名 HTTP 第二次读取第一篇未完成，不再自动重试；详情字段完整性仍待后续有授权的样本验收。[本机文件比对][E-manifest]、[HTTP 记录][E-http]。

**可达性结论**：未登录并非所有笔记一律无正文；有些原始 HTML 已包含 SSR 状态，另一些页面被限制，原生搜索在本机被登录墙挡住。xsec 参数是提高已发现笔记可读性的访问上下文，不能把“所有 URL 必须有 token”或“裸 ID 总能读”任一说法当全局事实。[匿名样本][E-http]、[内置 note 输入限制][OC186-helper]、[受限样本][E-web]。

通用发现建议仍用现有 `forager search 'site:xiaohongshu.com 关键词'`，然后读取实际返回的 URL。索引 snippet 保持 snippet 语义；页面读取必须校验 note ID、标题/作者/正文容器，拒绝登录页、推荐卡片、导航和风控页。Web Search 排序与日期筛选不是小红书站内筛选，也不能补齐评论；它不成为同平台 route 的无条件 fallback。[平台链契约][F-platform]、[本机搜索/页面差异][E-live]、[E-web]。

## 与 forager 契约的映射草案

### Platform、Route、Ref 与默认行为

| 项目 | 建议 |
|---|---|
| Platform id | `xiaohongshu`；展示名“小红书（Xiaohongshu / RED）”；不把 `rednote.com` 自动当同平台，需另核验身份及账号域 |
| HTTP route id | `tikhub`，按供应商命名，可复用后续其他平台；`credentials_required: true`，key 自备并进入现有 Credential Pool |
| process route id | `xiaohongshu_browser`，`OpenCli { site: forager-xiaohongshu, contract: forager-xiaohongshu/1 }`；不将内置 xiaohongshu 命令视为自有 adapter |
| 身份 kind | 首期只有 note；字符串 `xiaohongshu:<24hex>`。`xhs:<id>` 可作未来显式别名，不与 canonical 前缀混用 |
| canonical URL | `https://www.xiaohongshu.com/explore/<id>`；示例 `xiaohongshu:6a6f376f000000002203370b`；解析 canonical URL 得同一个 ref |
| order | 首期默认 `[]`；用户可选 `[tikhub]` 或 `[xiaohongshu_browser]`，browser 永不自动启用；普通 search/research 不自动调用此平台 |
| 非站点 key | process route `credentials_required: false` 表示 forager 不管理 keys；**不表示站点无需登录** |
| 短链 | 飞行前退 2；展开长链是显式独立动作，不能混入零 IO ref parser |

以上为建议，遵循 Platform/Route 身份、传输、opt-in 与配置约束；rednote 与 xiaohongshu 的区别有用户协议依据。[平台契约][F-platform]、[术语][F-glossary]、[ADR 0019][F-adr19]、[协议适用范围][XHS-terms-cn]。

### search L0/L1 与分页

L0 保留关键词与 limit。查询词按小红书关键词原样编码，**不承诺 arXiv 布尔语法或 Google 运算符**；以下 L1 为封闭枚举，均可选。值域来自浏览器源码与第三方文档，正式支持须逐项现场校验。[参数分层][F-platform]、[页面筛选映射][OC-search]、[TikHub 参数][TH-search]。

| 拟议 flag | 默认及值域 | TikHub / 浏览器映射 |
|---|---|---|
| `--sort` | comprehensive；latest、most-liked、most-commented、most-collected | general/time_descending/popularity_descending/comment_descending/collect_descending；浏览器“综合/最新/最多点赞/最多评论/最多收藏” |
| `--note-type` | all；image、video | 不限/普通笔记/视频笔记；浏览器“不限/图文/视频” |
| `--publish-time` | anytime；day、week、half-year | 不限/一天内/一周内/半年内；不添加任意日期区间或未核验 month/year |

首期不做 account scope、附近/同城、英文优先、直播或 AI 模式；若未来增加，每条 route 必须提供纯支持检查，不能静默忽略。OpenCLI 1.8.6 内置命令不支持这些非默认值；不是将 flag 附在命令后就能实现。[本机帮助][E-help]、[支持检查契约][F-platform]。

结果页为现有 `{platform,provider,items,next_cursor}`；搜索卡片只有标题、作者、互动/封面时标记 metadata，确有正文预览才标 snippet。title 不从推荐卡片兜底，authors 保留作者显示名与平台 user_id，published 只用真实 time/有明确精度的显示时间，缺失可空；不复用 OpenCLI 的 ID 日期估计。[输出契约][F-platform]、[当前页面字段][MCP-types]、[估计日期代码][OC186-search]。

HTTP cursor 采用 `v1.tikhub.<payload>`，恢复 query、options、limit、op、page、search_id/search_session_id。**App V2 search 文档没有 page_size/limit 参数**；需验证实际页长与 limit 的关系。若返回页长大于 limit，要保留未交付条目位置/可恢复页内余项，不能截掉后直接跳下一页。重取同页会额外计费且可能漂移；首期验收前不要承诺无漏项的任意 limit。[搜索参数表][TH-search]、[SDK schema][TH-sdk]、[cursor 契约][F-platform]。

cursor 不含站点 Cookie、xsec 或 API key；不透明通常只是编码，不等于加密。仅恢复非认证分页上下文。若浏览器分页确实需要 xsec，不能把它塞进公共 cursor，应把安全的会话内引用与跨调用恢复要求列为契约决策；未解决之前不开放 browser 续页。带 cursor 只在原 route/operation 继续，不 fallback，不允许与显式 query/L1/limit 同传。[平台 cursor 规则][F-platform]。

### fetch 输入与 Content Depth

**必须先解决两个既有契约缺口**：

1. `PlatformFetchRequest` 目前只有 reference/depth；CLI 解析原 URL 后仅构造这两项，xsec 信息会丢失。建议将“稳定身份”与“可选的敏感访问上下文”分离，在命令内传递已验证原长 URL，禁止 Serialize/Debug/日志外泄；不把 token 放 ref、canonical URL、公开条目或持久 cursor。裸 ref 由 TikHub ID 接口读取；browser 如无用户提供长 URL或已登录页面中的真实链接，明确拒绝，不能猜 token 或假称已删除。[当前请求类型][F-types]、[当前 CLI][F-cli]、[浏览器输入要求][OC-helper]。
2. 现有全文来源只有 URL/本地文件，而本地媒体类型只有 PDF；full_text 编排必经 Web Fetch，通用薄正文门至少要求 200 字符并检查行密度。笔记原生 desc 可能很短，且正文已在浏览器/API 中取得；把 canonical URL再送 Web Fetch 会丢登录/令牌上下文，把导航补长或造 PDF 会改变内容语义。[类型][F-types]、[正文编排][F-fetch]、[薄正文门][F-engine]、[阈值][F-outcome]。

建议在后续实现前更新规格：增加“route 已校验的原生文本”正文来源，由 core 统一交付 Markdown；完整性以 note ID、可信容器/响应类型、上游 desc 是否完整为证据，不以短文本长度决定失败。保留文件输出、content 格式、来源归因与失败阶段，adapter 不横向 import Web Fetch provider。不建议仅为短笔记全局放宽 Web Fetch 质量门。[当前职责边界][F-architecture]、[正文交付契约][F-platform]。

浏览器访问上下文还有一个待决策问题：原长 URL 如果出现在子进程 argv，可能被本机进程检查工具读到；若随公共结果落盘，则直接违背“不输出 token”的目标。建议只在命令生命周期内从 stdin 或浏览器会话内传递。**当前 OpenCliCommand 只有通过 argv 传递的 named options，这需要扩展 process transport，不能直接声称已有 stdin 输入契约。** 未来给用户接续浏览器 fetch 的入口也要显式解决令牌来源和生命周期。不要为了方便 search→fetch 就新增永久 Cookie/token 缓存；如果需要跨调用受保护的上下文，必须先设计其访问范围、过期和清理契约。[内置命令 URL 参数][OC186-helper]、[forager 进程传输][F-opencli]。

| depth | 小红书建议语义与阶段 |
|---|---|
| metadata | 题名、作者、发布时间、笔记类型、标签、互动、媒体元数据；没有正文。首期默认并支持 |
| snippet | 搜索/页面真实可见的正文截片，记录截断；没有截片就不标此 depth。fetch 首期可不支持 |
| full_text | 完整 title + desc + 标签 + 已验证图片/视频链接；不自动 OCR 图片、不声称看懂视频，不包含评论。待原生文本来源与交付契约落定后支持 |
| thread | 如未来支持，应明确根笔记 + 有界评论/回复、覆盖范围与继续位置；不能借此声称全量。首期不支持 |
| abstract | 不使用；笔记没有本草案定义的学术摘要形状 |

深度没有全局高低排序；显式 full_text/thread 不可暗降级 metadata。媒体链接本身不等于图片/视频内容已读取；下载/转录是独立目标。需要认证或带敏感签名的媒体地址也不能原样进 attempts/永久来源，本调研没有保存任何真实媒体 URL。[Depth 与交付契约][F-platform]、[页面媒体形状][MCP-types]。

### 评论选 L2，thread 留待后续

**推荐显式 L2 `comments <note-ref>` 和 `replies <note-ref> --comment-id <id>`**：评论结果种类改变，子评论增加必需输入，符合 L2 定义；独立分页能限制一次调用工作量。首条 route 用 inherent 方法，第二条真正实现后才提升 trait，不现在设计通用 MCP/CLI route。[L2 规则][F-platform]、[Scholar 先例][F-adr21]。

拟议评论页包含 note_ref、provider、comments、next_cursor；评论含 comment_id、root_comment_id、parent/target_comment_id（缺失则 null）、author、text、likes、published、reply_count、可用媒体；楼中楼明确保留所属根与实际回复目标。每个根回复有自己的 cursor。`--with-replies` 若提供，应为有界预览并带各根继续位置，不能默认遍历所有根和回复；“有回复数”不等于回复已获取。[MC 评论形状][MC-client]、[MCP 评论类型][MCP-types]、[第三方分页][TH-subcomments]。

候选 thread 的优点是一次消费根内容与讨论，额外代价是隐藏的 N+1 请求、费用、输出规模与多游标；当前 fetch 没有评论 limit/replies limit/cursor 输入。直接增加 fetch 参数会把有界读取与分页压进深度概念，不适合最小实现。可以后续把 thread 定义为显式预算内的讨论快照，但需先更新契约。[fetch 请求类型][F-types]、[现有深度规则][F-platform]。

可选 `user-notes <user-id>` 也作为独立 L2，不用 search 的 `--author` 改变必需输入。首期不需要新 user kind；只有加入用户实体 fetch 时再定义 user ref 与往返规则。只读取“已发布笔记”，不混入喜欢/收藏列表。[用户列表端点][TH-sdk]、[L2/Ref 规则][F-platform]。

### 访问策略、失败与接入登记

browser 建议每 OpenCLI 命令至少 **10 秒**、并发 **1**，permit 持有至进程回收；命令内最多一页且有滚动/回复预算，doctor 仅在 order 启用后运行。10 秒是建议初值，**不是站点安全阈值或许可**；实测登录墙不因等候而变成可重试请求。HTTP TikHub 建议首期每次发送至少 1 秒、并发 1，付费探针单独显式运行；每次重试都经过 limiter。[访问策略与限速单位][F-platform]、[ADR 0020][F-adr20]、[供应方 RPS][TH-pricing]。

建议业务错误分类：登录墙/验证码/安全限制 → Auth并停止；明确请求限流 → RateLimited；第三方余额不足 → QuotaExhausted；真实“无结果” → LegitimateEmpty；字段/身份不匹配 → Quality/Runtime；明确不存在 → attempt Parameter。`300031`、空 DOM、HTTP 200 的上游服务异常不能自动当 LegitimateEmpty 或“已删除”。具体业务码仍需响应验收，不复制其他平台的错误映射。[错误阶段契约][F-platform]、[受限样本][E-web]、[付费业务错误说明][TH-sdk]。

未来实施按 R1–R8登记 Platform/ProviderId、factory 支持检查、配置与 runtime、doctor、smoke、acceptance fixture、ref 样例；L2 也登记 fixture。模块放 `capabilities` 的平台 adapter 与 `infra/types` 平台叶子，CLI/core 只做参数与编排，公共门面仍为 app/config/types；有模块或正文来源所有权改变时同步第 4 章。词表、skill reference、CLI/config/test 规格及术语一并更新。本次只给清单，不修改这些文件。[接入清单][F-platform]、[架构][F-architecture]、[术语][F-glossary]。

## 风险、开放问题与下一步验收

| 问题 | 已知事实 / 不确定性 | 建议验收条件 |
|---|---|---|
| 适用许可 | 用户协议限制第三方/逆向；robots 通用组禁止；第三方不保证原站授权 | 个人使用明确接受 opt-in 边界；商用取得适用数据访问/使用许可；不把技术成功当授权；[条款][XHS-terms-cn]、[供应方条款][TH-terms] |
| 本机登录 | bridge 正常，搜索 Auth；详情与评论尚未实测 | 用户自行登录后，另一次低频验证搜索、图文/视频、一级/二级评论；本次不代登录；[实测][E-live] |
| 供应方现状 | TikHub App V2有文档，Web V3公告冲突；未使用 key | 确认有效端点/计费/条款；采集脱敏响应；不先配置退役系列 fallback；[指南][TH-guide]、[产品页][TH-product] |
| 分页正确性 | browser 没公共 cursor；HTTP 未文档化 page_size，子评论状态复合 | 搜索连续两页、limit小于页长、终页、空页、恢复参数、重复与排名变化；评论与每个根独立续页；[参数][TH-search]、[评论][TH-subcomments] |
| 字段可信度 | OpenCLI 发布日期是估计；DOM互动可能是缩写；time/update显示语义不同 | 元数据真实来源与时间单位、时区/精度；`1.2万` 保留原显示和近似标识，缺失不可变 0；作者ID与显示名分离；[日期代码][OC186-search]、[note字段][MCP-types] |
| 媒体完整性 | 内置 note 无媒体，download会下载；第三方视频可能需第二请求 | 图文多图、视频流/封面、空正文、无标题、过期媒体地址与权限；不输出 sensitive signed URL；[download][OC186-download]、[详情接口][TH-sdk] |
| 正文交付 | 现有 request 丢URL上下文；本地源仅 PDF；短文薄正文门 | 先批准原生文本来源/敏感上下文契约；验证短笔记能完整交付，错误页不能通过；[当前类型][F-types]、[质量门][F-engine] |
| 可用性与账号风险 | 有只读账号违规/验证码报告；概率未知 | 有限访问与停止策略，手动恢复，不宣传“不会封号”；[报告][OC-issue842]、[新布局][OC-issue2562] |
| 代码许可 | MC 非商业；Spider_XHS许可矛盾；宽松许可也不授权站点内容 | 不复制不明许可代码；独立实现已核验的 adapter，保留必要许可；[MC LICENSE][MC-license]、[Spider README][SP-readme] |

**实施顺序建议**：先确认第三方适用范围和少量真实响应，再做 metadata MVP；第二阶段开放已验收 L1筛选与 comments/replies L2；第三阶段解决原生 full_text 交付；browser 的敏感URL上下文与分页单独验收。若第三方或浏览器都不满足授权/稳定性约束，继续用现有 Web Search/Fetch 显式补充发现，不注册一个仅换名字而不能满足契约的平台。

## 来源与证据索引

所有远端资料和本地文件在 2026-10-08 读取。源码链接固定至本次克隆提交；issues 是当日状态，报告人观察未独立复现。搜索答案仅用于定位，未将其当事实依据；forager research 的候选中有二手文章，本报告只采用已读取的一手正文/源码。未保存 Cookie、API key、xsec 值、验证码会话、真实签名媒体地址或原始浏览器状态。[安全证据文件][E-manifest]。

[F-platform]: ../spec/forager/07-platforms.md
[F-architecture]: ../spec/forager/04-architecture.md
[F-glossary]: ../../GLOSSARY.md
[F-adr19]: ../adr/0019-platforms-as-a-dimension-separate-from-capability-seams.md
[F-adr20]: ../adr/0020-opt-in-browser-routes-paced-per-operation.md
[F-adr21]: ../adr/0021-scholar-only-through-a-user-keyed-serp-api.md
[F-ssrn]: ../../skills/forager/opencli/ssrn/
[F-types]: ../../src/infra/types/platform.rs
[F-cli]: ../../src/cli/platform.rs
[F-fetch]: ../../src/core/platform_fetch.rs
[F-engine]: ../../src/core/engine.rs
[F-outcome]: ../../src/infra/types/outcome.rs
[F-opencli]: ../../src/capabilities/providers/opencli.rs
[E-live]: xiaohongshu-2026-10-08/live-probe.json
[E-help]: xiaohongshu-2026-10-08/opencli-help.json
[E-manifest]: xiaohongshu-2026-10-08/source-manifest.json
[E-http]: xiaohongshu-2026-10-08/anonymous-http.json
[E-web]: xiaohongshu-2026-10-08/web-observations.json
[E-robots]: xiaohongshu-2026-10-08/robots.txt
[XHS-terms]: https://www.xiaohongshu.com/terms
[XHS-terms-cn]: https://agree.xiaohongshu.com/h5/terms/ZXXY20220331001/-1
[XHS-community]: https://www.xiaohongshu.com/en/community_guidelines
[XHS-robots]: https://www.xiaohongshu.com/robots.txt
[XHS-beian]: https://beian.xiaohongshu.com/?id=10&module=content
[XHS-policy-note]: https://www.xiaohongshu.com/explore/69afda73000000002800b3f2
[OFF-register]: https://open.xiaohongshu.com/document/developer/file/32
[OFF-apps]: https://open.xiaohongshu.com/document/developer/file/33
[OFF-auth]: https://open.xiaohongshu.com/document/developer/file/38
[OFF-ad]: https://ad-market.xiaohongshu.com/
[OFF-ad-docs]: https://ad-market.xiaohongshu.com/docs-center
[OC186-search]: https://github.com/jackwener/OpenCLI/blob/cad35e7a6a5ff3f7d6b859bfa4c45195c0390260/clis/xiaohongshu/search.js
[OC186-note]: https://github.com/jackwener/OpenCLI/blob/cad35e7a6a5ff3f7d6b859bfa4c45195c0390260/clis/xiaohongshu/note.js
[OC186-comments]: https://github.com/jackwener/OpenCLI/blob/cad35e7a6a5ff3f7d6b859bfa4c45195c0390260/clis/xiaohongshu/comments.js
[OC186-user]: https://github.com/jackwener/OpenCLI/blob/cad35e7a6a5ff3f7d6b859bfa4c45195c0390260/clis/xiaohongshu/user.js
[OC186-download]: https://github.com/jackwener/OpenCLI/blob/cad35e7a6a5ff3f7d6b859bfa4c45195c0390260/clis/xiaohongshu/download.js
[OC186-helper]: https://github.com/jackwener/OpenCLI/blob/cad35e7a6a5ff3f7d6b859bfa4c45195c0390260/clis/xiaohongshu/note-helpers.js
[OC-search]: https://github.com/jackwener/OpenCLI/blob/24136945847afbfad266c6c46a8cd335377f9112/clis/xiaohongshu/search.js
[OC-helper]: https://github.com/jackwener/OpenCLI/blob/24136945847afbfad266c6c46a8cd335377f9112/clis/xiaohongshu/note-helpers.js
[OC-risk]: https://github.com/jackwener/OpenCLI/blob/24136945847afbfad266c6c46a8cd335377f9112/clis/xiaohongshu/risk-control.js
[OC-bridge]: https://github.com/jackwener/OpenCLI/blob/24136945847afbfad266c6c46a8cd335377f9112/docs/guide/browser-bridge.md
[OC-issue2562]: https://github.com/jackwener/OpenCLI/issues/2562
[OC-issue2576]: https://github.com/jackwener/OpenCLI/issues/2576
[OC-issue842]: https://github.com/jackwener/OpenCLI/issues/842
[OC-issue2208]: https://github.com/jackwener/OpenCLI/issues/2208
[MC-client]: https://github.com/NanmiCoder/MediaCrawler/blob/93c405f89a8b3bc718c181a39106f6c0da83ff5f/media_platform/xhs/client.py
[MC-sign]: https://github.com/NanmiCoder/MediaCrawler/blob/93c405f89a8b3bc718c181a39106f6c0da83ff5f/media_platform/xhs/playwright_sign.py
[MC-fields]: https://github.com/NanmiCoder/MediaCrawler/blob/93c405f89a8b3bc718c181a39106f6c0da83ff5f/media_platform/xhs/field.py
[MC-store]: https://github.com/NanmiCoder/MediaCrawler/blob/93c405f89a8b3bc718c181a39106f6c0da83ff5f/store/xhs/__init__.py
[MC-readme]: https://github.com/NanmiCoder/MediaCrawler/blob/93c405f89a8b3bc718c181a39106f6c0da83ff5f/README.md
[MC-license]: https://github.com/NanmiCoder/MediaCrawler/blob/93c405f89a8b3bc718c181a39106f6c0da83ff5f/LICENSE
[MC-issue990]: https://github.com/NanmiCoder/MediaCrawler/issues/990
[MC-issue915]: https://github.com/NanmiCoder/MediaCrawler/issues/915
[PY-readme]: https://github.com/ReaJason/xhs/blob/f4b62d9f8e4078e631fc6e4ec8e430bc711ee9f0/README.md
[PY-core]: https://github.com/ReaJason/xhs/blob/f4b62d9f8e4078e631fc6e4ec8e430bc711ee9f0/xhs/core.py
[PY-sign]: https://github.com/ReaJason/xhs/blob/f4b62d9f8e4078e631fc6e4ec8e430bc711ee9f0/example/basic_sign_server.py
[PY-help]: https://github.com/ReaJason/xhs/blob/f4b62d9f8e4078e631fc6e4ec8e430bc711ee9f0/xhs/help.py
[PY-license]: https://github.com/ReaJason/xhs/blob/f4b62d9f8e4078e631fc6e4ec8e430bc711ee9f0/LICENSE
[MCP-search]: https://github.com/xpzouying/xiaohongshu-mcp/blob/a5c8f7799980ba1fdd501999843eb2d17e4c9a9f/xiaohongshu/search.go
[MCP-types]: https://github.com/xpzouying/xiaohongshu-mcp/blob/a5c8f7799980ba1fdd501999843eb2d17e4c9a9f/xiaohongshu/types.go
[MCP-browser]: https://github.com/xpzouying/xiaohongshu-mcp/blob/a5c8f7799980ba1fdd501999843eb2d17e4c9a9f/browser/browser.go
[MCP-license]: https://github.com/xpzouying/xiaohongshu-mcp/blob/a5c8f7799980ba1fdd501999843eb2d17e4c9a9f/LICENSE
[SP-api]: https://github.com/cv-cat/Spider_XHS/blob/ebb6c4fbeaedf1237190ebc0c8e3ae8b7ddd030e/apis/xhs_pc_apis.py
[SP-auth]: https://github.com/cv-cat/Spider_XHS/blob/ebb6c4fbeaedf1237190ebc0c8e3ae8b7ddd030e/xhs_utils/xhs_pc/auth.py
[SP-params]: https://github.com/cv-cat/Spider_XHS/blob/ebb6c4fbeaedf1237190ebc0c8e3ae8b7ddd030e/xhs_utils/xhs_pc/params.py
[SP-readme]: https://github.com/cv-cat/Spider_XHS/blob/ebb6c4fbeaedf1237190ebc0c8e3ae8b7ddd030e/README.md
[TH-guide]: https://blog.tikhub.io/zh/article/7
[TH-search]: https://docs.tikhub.io/420136398e0
[TH-subcomments]: https://docs.tikhub.io/420748830e0
[TH-sdk]: https://github.com/TikHub/TikHub-API-Python-SDK/blob/1ede56bcc53f8c037744cbe2b70c93541b5cf840/spec/openapi.json
[TH-product]: https://tikhub.io/zh/xiaohongshu-api
[TH-pricing]: https://tikhub.io/pricing
[TH-terms]: https://user.tikhub.io/terms
[AP-search]: https://apify.com/kuaima/xiaohongshu-search
[AP-search-price]: https://apify.com/kuaima/xiaohongshu-search/pricing
[AP-search-api]: https://apify.com/kuaima/xiaohongshu-search/api
[AP-comments]: https://apify.com/khadinakbar/xiaohongshu-comments-scraper
[AP-browser]: https://apify.com/edgy_dock/xiaohongshu-rednote-scraper
[AP-terms]: https://docs.apify.com/legal/general-terms-and-conditions
[AP-aup]: https://docs.apify.com/legal/acceptable-use-policy
[AP-api]: https://docs.apify.com/api/v2
[SERP]: https://serpapi.com/search-api
