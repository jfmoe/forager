# forager

forager 按检索能力组织供应方，并把尚未满足自动路由条件的外部能力隔离在显式验收入口中。

## Language

**AnySearch Acceptance Surface**:
通过 `forager anysearch search` 与 `forager anysearch domains` 验证传输、操作和具体垂直域的实验入口；它独立于分类器使用的 Vertical Discovery，不代表域级自动路由已经可用。
_Avoid_: AnySearch capability、AnySearch fallback

**Vertical Search Capability**:
可由分类器在明确垂直意图下选择的检索能力，包括 Vertical Discovery，以及针对 Verified Vertical Domain 的结构化搜索。
_Avoid_: AnySearch acceptance surface、web search fallback

**AnySearch Operation Status**:
AnySearch 单项操作的可用状态，分别描述域发现、Vertical Discovery 和域级搜索，不代表任何垂直域已经可供自动路由。
_Avoid_: AnySearch available、vertical search ready

**Verified Vertical Domain**:
必填参数、成功结果和失败行为均已通过域级验收，且分类器能够可靠构造请求的 domain/sub-domain 组合。
_Avoid_: discovered domain、configured domain、supported AnySearch

**Capability Seam**:
围绕一种检索能力定义的稳定契约，只由提供该能力的供应方共享，不要求不同能力具有相同操作。
_Avoid_: universal provider interface、extra search channel

**Documentation Search Capability**:
面向外部库、SDK、API、框架和官方技术文档的检索能力；普通技术知识问答或仅出现技术名词不自动构成该能力需求。
_Avoid_: general technical search、Context7 capability、docs provider

**Supplemental Web Search Capability**:
主搜索之外，为时效、新闻、地区、政策、行情、交叉验证或来源补强提供的 Web 发现能力；它不代表每次普通搜索都会执行的主搜索。
_Avoid_: main search、all web access、web provider

**Primary Search Source**:
主搜索回答归属的来源，由 search 输出的 `sources` 表示；它不同于非主搜索能力产生但尚未消费的候选，也不因被引用而自动成为已验证证据。
_Avoid_: supplemental source、verified evidence、all search sources

**Search Candidate**:
普通 search 中由非主搜索能力产生、可供调用方选择后续动作的候选，由 `extra_sources` 表示；候选可以由 URL、provider 专属标识或结构化记录定位，在被相应读取或取证流程消费前不是证据。
_Avoid_: Supplemental Search Candidate、primary source、verified evidence、merged source

**Web Fetch Capability**:
读取、提取或核验已知 URL 或 PDF 正文的能力；它不负责发现链接。
_Avoid_: web search、link discovery

**Normalized Fetch Content**:
Web Fetch Capability 经 provider 解码与质量门控后接受的 provider 无关 Markdown 正文；它不包含传输包装或诊断，也不承诺不同 provider 选出相同的正文边界。
_Avoid_: raw provider response、cleaned content

**Provider Acceptance Operation**:
只属于某个供应方显式验收入口的操作，不构成 Capability Seam，也不允许分类器据此跨能力调用。
_Avoid_: provider capability、fallback operation

**Sub-domain Parameters**:
由具体垂直子域定义的开放结构化参数；它们随 Verified Vertical Domain 的契约变化，不属于通用搜索字段。
_Avoid_: provider arguments、domain flags、vertical search configuration

**Domain Discovery**:
查询父域下可用子域及其参数契约的验收操作；其语义独立于供应方当前使用的工具名称。
_Avoid_: list domains、get sub-domains tool

**Vertical Search Request**:
指向明确 domain/sub-domain 并携带查询和 Sub-domain Parameters 的搜索请求；不指定垂直子域的通用搜索不属于该概念。
_Avoid_: general search、domain-less search

**Vertical Discovery**:
分类器在明确垂直意图下使用无域搜索发现候选的过程；它不证明任何垂直域已验收，也不属于通用 Web Search 兜底。
_Avoid_: general search、web search、verified vertical search

**Configured AnySearch**:
已通过统一 `keys` 凭据池提供至少一个认证凭据的 AnySearch；该状态允许分类器执行自动 Vertical Discovery。
_Avoid_: reachable AnySearch、anonymous AnySearch、enabled endpoint

**Verified Domain Contract**:
经域级验收确认的子域参数与可观察行为约定，是自动域级路由可依赖的稳定知识；实时 Domain Discovery 结果本身不是该契约。
_Avoid_: live schema、discovered schema、upstream manifest

**Verified Domain Manifest**:
受版本控制的 Verified Domain Contracts 集合；只有进入该清单的垂直域才能被声明为已支持，实时发现的新域仍保持未验收状态。
_Avoid_: domain catalog、live discovery result、supported-by-default list

**Automatic Domain Search**:
分类器从自然语言中选择具体垂直子域并构造其 Sub-domain Parameters 的搜索过程；它不同于无域的 Vertical Discovery 和用户指定目标的显式域级搜索。
_Avoid_: vertical discovery、explicit vertical search、vertical intent routing

**Default Search Invocation**:
一次通过 forager 默认聚合搜索入口发起并到达终态的请求，无论结果成功、失败或超时；供应方验收、抓取、诊断及其他专用操作不属于该概念。
_Avoid_: successful search、provider search、all commands

**Search Result Journal**:
默认启用、按发生顺序保存每次 Default Search Invocation 的结果面与执行过程面的本地集合。结果面包含查询、回答、来源及 research 的引用与证据；过程面包含计划摘要、供应方尝试链、终态归因、预算、分类器耗时和能力缺口。字段以架构规格的白名单为准，不保存请求或响应头、请求体、原始响应体、凭据、分类器 prompt 或工具 trace。它用于复盘与诊断，不是调试消息流，也不作为搜索响应缓存。
_Avoid_: debug log、response cache、search history

**Caller Capability Declaration**:
调用方为一次普通 `forager search` 显式声明的完整、权威检索能力集合；空集合也是有效声明。forager 不以本地规则、URL 识别、严格校验或分类器补充该集合；声明只能引用 Capability Seam，不能选择供应方。research 使用计划注入，不使用该声明。
_Avoid_: provider override、router hint、capability patch

**Classifier Capability Decision**:
意图分类器为普通 search 产出的完整检索能力集合，或为裸 research 产出的 Schema v1 计划（`intent_signals` + `decomposition`）；分类器不能选择供应方。
_Avoid_: classifier additions、classifier patch、provider decision

**Capability Vocabulary**:
统一拥有 capability 身份、顺序、选择语义和语义例句的领域词汇表；分类器 prompt、`--capabilities` 校验与 forager skill 契约共同消费它，而 provider fallback 与路由控制流不属于该词汇表。
_Avoid_: Skill capability list、routing rules DSL、provider registry

**Research Evidence Pipeline**:
research 引擎的职责定位：确定性地执行计划、发现候选、取证、按子问题记账覆盖并落盘证据；它不做语义综合，也不含 main search 定向——综合归消费方 agent，定向在计划之前由 skill 编排完成。
_Avoid_: research answer engine、engine synthesis、final answer generator、in-engine orientation

**Research Evidence Index**:
Research Evidence Pipeline 在标准输出中交付的轻量证据目录；它描述每条证据的身份、来源、覆盖归属与本地正文路径，但不重复输出已经落盘的正文。
_Avoid_: evidence preview、inline evidence content、research answer

**Citation Binding**:
回答文本中的内联标记与某条来源或证据之间可机器读取的对应关系；它表达引用归属，不代表系统已验证该证据确实支持对应陈述。
_Avoid_: claim verification、source list

**Provider Credential Pool**:
某一供应方上由 TOML `keys` 真数组配置的认证凭据集合；八个供应方统一使用这一形状，单凭据是单元素数组。运行时按轮询选用，并在额度或限流类失败时于同一次请求内换用其他凭据。
_Avoid_: key pool、key rotation、API key list、high-availability credential pool
