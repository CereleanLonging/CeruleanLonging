# 第342项：P3-MONEY经济合同清单与生产拒绝点现状

生成时间：2026-10-24T10:40:08.418Z。本轮完成本地盘点与接线计划；本轮不关闭 P3-MONEY，仍属外部经济合同阻塞。contractChecklistReady=true，wiringPlanReady=true；pricesConfirmedFromAuthoritativeSource=false，formalAcceptance=false，releaseAccepted=false，p3MoneyClosed=false。

## 范围、证据与继承

前沿先读两份索引顶部、338 review全文及当天交接目录：最新为341/024，未发现342同名交付。索引中的数字342可出现在旧时间戳，不等于342交付。未安装、未联网、未读或使用凭证、未启动GUI/产品/业务进程；realModelCalls=0、keyAccess=false、realInstallerLaunches=0、productionSourceChanged=false、gitWritten=false。git只读实测1 commit、0 remote。没有运行构建、测试、受控验证器。账户额度没有可靠实时读数，不能声称95%自动监控有效。

引用采用“原路径:行号”，对应逐字节快照与SHA见 sources-20261024T103724018Z.json；报告目录中的 evidence-20261024T103724018Z/ 保存独立原文，活动源码后续变化不影响本轮证据。本文与报告同名时间戳副本相同。338 review:11、43、77–83保持完整经济门槛；不重跑原型接入。复用依据为 PROTOTYPE-REUSE-BASELINE.md:7–25、实施方案v2:77、132、138–144与 prototype-technical-acceptance JSON:5–39：已有接入与取消恢复结论保持原范围，不证明金额合同。

路径约定：合同表中的 Rates、Usage、PriceQuote、TokenCeiling、Reservation、Settlement 及其行号均属于 crates/domain/src/currency_budget.rs；scoped.rs 属于 crates/domain/src/currency_budget/scoped.rs。描述 Unknown/NotSent/Settle/Unpriced 状态转换的 currency_budget.rs 行号属于 crates/core/src/currency_budget.rs；api_observations.rs 属于 crates/core/src/currency_budget/api_observations.rs。count_resolution.rs、api_count_resolution.rs、api_generation_resolution.rs 均属于 crates/domain/src/。generation_resolution.rs 是上述 api_generation_resolution.rs 的简写。所有来源完整路径及SHA以来源清单为准。

独立审阅补收：E9引用的 crates/core/src/main.rs 原未列入62份初始清单，现逐字节快照为 evidence-20261024T103724018Z/063-main.rs；SHA256 dcee5b735169153d182a47cc5275b529603567751ba80049aa63f2a2deba7137，见 sources-addendum-20261024T103724018Z.json。合计63份来源；主清单保留原始62份记录，不覆盖。

## 三处生产拒绝保护的实际原文

1. crates/core/src/host.rs:574：
```rust
pub(super) fn require_currency_execution_available(&self, project: &str) -> Result<(), String> {
    require(
        !self.currency_accounts.contains_key(project)
            && self.project_setups.get(project)
                .is_none_or(|s| s.setup.policy.currency_budget.is_none())
            && self.projects.get(project).and_then(|p| p.revision())
                .is_none_or(|r| r.policy.currency_budget.is_none()),
        "此项目的金额预算尚不可执行；未发送模型请求",
    )
}
```
以上为压缩空白展示，逐字逐行原文以快照574–588为准，错误原文在586。拒绝已开户、setup金额预算或revision金额预算任一存在的项目。

2. crates/core/src/api_execution.rs:126–140：scope的131–139执行 require(store.revision().ok_or("API plan missing")?.policy.currency_budget.is_none(), "此方案的金额预算尚不可执行；未发送API请求")?。错误原文在138。142–144明确 evidence_scope 仅校验原批准范围，不授权发送、应用完成、执行工具或释放金额预留。

3. crates/adapters/src/api_generation.rs:72–79：billable_usage的76–79执行 require(provider != CredentialProvider::A6Api, "A6API monetary usage contract is not approved")?。69–71明确缺缓存明细不等于零缓存/零价格，错误时保留预留；82–97分别拒绝未知缓存读/写计量。

**338 文档记载的行号与当前源码实际位置的差异**：338 review:11记host.rs:574、api_execution.rs:125、api_generation.rs:72。当前依次为574、scope开头126（比记载+1；实际require131/错误138）、billable_usage开头72（实际require76/错误78）。未把338行号当成当前证据。

补充门：management_preflight_host.rs:54–71，特别68–70拒绝“此项目的金额预算尚不可执行：计数费用及服务/上下文报价覆盖尚未确定；未发送模型请求”；api_currency_host.rs:12–18仅cfg(test)夹具可绕过，生产仍调用Host门。已有scoped/count代码不能当作生产已接通。

**合同字段确认前，以上三处保护必须保持拒绝。不得放宽、注释、删除或加 #[ignore] 来让检查变绿。** 本轮没有修改它们。

## 可交由供应商/人类评审的合同清单

下表中“unknown”表示本轮无可批准外部事实；结构存在不表示合同已批准。出口编号见下一节。每一行均为开放项，末列明确归属。

| 字段名 | 必需原因与适用出口 | 当前承接与验证性质 | 缺失事实/来源与归属 |
| --- | --- | --- | --- |
| C01 currency / amount_unit / token_unit / tax_fx | E1–E9需把所有收费换为同一批准额度 | currency_budget.rs:12–15固定整数USD nanodollars；Rates:91–100以每百万token的USD microdollars表示，无自动换汇；非供应商币种证明 | 结算币种、token定义、单位、税/附加费/汇率及换算时点unknown；供应商条款/官方文档。待供应商确认；用户若需非USD另待用户决策 |
| C02 input / output / reasoning / cache_read / cache_write | E1/E2/E3/E4计数与生成必须完整核销，E5–E9不能伪造token账单 | Rates:91–100与Usage:396–405有输入/输出/缓存读写四项；Usage:391–394要求完整非重叠，validate:407–414只是数值范围；ApiUsage:62–67没有独立隐藏推理字段；billable_usage:72–118按provider归一化 | 隐藏推理是包含输出、独立收费或不可见，以及缓存包含/分离、写入TTL/倍率、真实零值证明均unknown；供应商计量规范/官方文档。待供应商确认 |
| C03 price_source / published_at / effective_from_until / approved_version | 所有出口要在原报价下批准，不用后来的价格重算 | PriceQuote:181–196、314–362；ScopedPriceQuote scoped.rs:82–100、167–252有source和有效期；source仅文本校验、日期可验证区间，不认证官网/合同；公布日、抓取日无独立字段 | 当前权威单价、生效日、适用账户/路由/模型unknown。公开276只是历史目录，详下文；须合同或官方页与版本摘要。待供应商确认；报价与预算批准待用户决策 |
| C04 requested_actual_model / service_tier / context_band / billing_basis | E1–E4防路由模型、服务和长上下文变价；CLI/桌面还需实际模型证据 | scoped.rs:16–43、73–116、264–357有模型×服务×输入闭区间覆盖、PerRequest，非格式校验而是有前提的覆盖/算术；不支持whole-session repricing | 实际服务标识、自动路由、阈值边界、整会话重计规则unknown；每端点独立官方规范。待供应商确认 |
| C05 route_attempt / retry / fallback / tools_round_fee | E1供应商管理与工人、E3管理及E5–E9代理内部尝试可能另计费 | api_generation.rs:448–456、480–482本地no retry；omp_process.rs:157–161禁重试/fallback；Reservation:554–558有request/operation/hash，无供应商逐路由账单模型 | 网关内部每次尝试/失败/工具回合/重试是否计费、可否幂等免重复、每路由上界unknown；本地一次POST不证明内部一次收费。待供应商确认 |
| C06 maximum_liability / input_output_ceiling / count_fee / reservation_release | E2/E4计数也可能收费；所有发送必须先有真实上界和持久预留 | ManagementCountPrice:199–223含count金额与body上限；TokenCeiling:367–389；Reservation:550–587有证据hash但hash仅格式；Rates:142–176保守预留，scoped:274–309覆盖最大额；host计数先持久化:284再spawn | token-count是否收费、返回值是否账单硬上界、隐藏token/路由/全请求是否都受上界约束、释放条件unknown。供应商保证必须先到位。待供应商确认；用户风险额度待用户决策 |
| C07 late_response / Unknown / unpriced / finality | 全出口断连后可迟到，不能超时释放或重发 | scoped.rs:311–357按原quote/request评估晚回；currency_budget.rs:434–448 Unknown保留预留、539–544 Unpriced保留并阻断；Settlement:592–619有response与evidence hash但不验账单真实性 | 晚回结算窗口、账单最终性、无结果仍收费、重复账单去重和争议凭证unknown。待供应商确认；人工核销责任待用户决策 |
| C08 cancel_cutoff / fee_after_cancel / refund | E1本地取消，E5 interrupt、E6 abort、E7/E8 Stop均不等于退款 | api_generation.rs:480–482与658；api_host:746–788 CancelRequested；codex_cancellation:113–139、omp_transport/worker:376–390、DesktopStop.cs:164–165；无供应商退款证明 | 何时停止计费、已运行部分、晚回和退款时延、cancel ACK含义unknown。待供应商确认 |
| C09 restart_reconciliation / idempotency / retention | 所有出口中断后保留同一经济责任，不能恢复时重发 | currency_budget.rs:3、434–470；api_call_journal/scoped.rs:234–307原意图/原费用ACK；API recover api_host:727–744，Codex codex_execution:579、OMP omp_host:664起、桌面TaskRead desktop_sessions:776–802；本地机制存在 | 供应商按原ID查账、数据留存时长、重启跨日账期、缺ACK最终处置unknown。待供应商确认 |
| C10 billing_reference / request_response_gateway_mapping / balance_freshness | 所有出口须逐笔核销及识别可用余额；仅请求ID不能推出钱 | Settlement:592–619；count_resolution.rs:24–34、api_count_resolution.rs:11–35、api_generation_resolution.rs:11–35有amount/billing_reference/explanation，主要格式校验，不认证账单；ProjectCurrencyBudget:38–62是本地视图 | 供应商逐笔账单/余额接口可得性、读数时点、延迟和ID映射unknown；338:79保留295 body responseId与网关Request ID未充分映射、actualMoney=null、284 Unknown。账户额度无可靠实时读数，95%不能声称自动监控有效。待供应商确认；人工提供与收尾安排待用户决策 |
| C11 liability_owner / cap_breach / stop_loss / dispute | 每出口超过上界、usage未知或账本异常必须阻止后续支出并保存原证据 | currency_budget.rs:539–544阻断Unpriced，api_observations.rs:236–249费用未知则工具/下一轮阻塞；count_resolution.rs:1–2与generation_resolution.rs:1不授权新请求 | 超额由谁承担、供应商是否硬限额、退款/争议流程unknown；用户预算、容忍度、暂停/通知/人工复核负责人unknown。待供应商确认；止损与责任分配待用户决策 |

## 历史公开价格材料的可引用边界

本轮未联网。已定位 reports/a6api-preparation-276/public-pricing.json:1，SHA256 **2ee8dac06067157f7fcf7c38aa17d3ddacaaf946a0dca469caaee401a146dc9a**。历史采集元数据 public-model-review.json:2–6记录2026-10-23T08:15:50.316914+08:00、公开来源 https://a6api.com/api/pricing；683–686明确authenticated=false、modelCalls=0、accountAvailabilityVerified=false、pricesApproved=false。19–27 gpt-6.1-sol字段含model_ratio=0.5、model_price=0、completion_ratio/cache_ratio/create_cache_ratio=1，**不能解释为免费、不能换算货币单价、不能确认现行报价**。preparation-note.md:19–24说明公开读取及公开价格不构成用户批准报价。public-site-review.json:2–7和41的“预算自动熔断”只是历史公开前端文案；其SHA同见来源清单。

只读研究在发现的10个a6api目录未定位独立HTML报价原件；已有公开JSON/JS材料并不缺失。未将HTML未定位扩大为报价不存在，也没有读取账户/账单私密材料。pricesConfirmedFromAuthoritativeSource=false：没有获批准的现行完整报价与经济保证，公开倍率/文案不够。

## 真实发送出口与接线顺序计划（不实现）

统一CLI workbench_cli.rs:1062–1122、1684–1721与Tauri main.rs:72–105走同一认证IPC/Host；project_runner.rs:455–475才分叉各适配器。它们不是额外模型HTTP出口，也不是金额授权绕过口。

未来依赖顺序：①供应商确认C01–C11并取得用户预算/责任批准，按每供应商/端点/模型/服务冻结合同证据；②补足供应商计量/隐藏推理及路由上界的表达和校验，复用现有domain、count/账本/审批，不以文本source代认证；③先绑定计数自身费用并持久预留，再验证完整报文上界，持久化generation reservation/intent；④接每个出口，先落原billing证据与结算ACK后才释放回复/工具/下一轮；⑤接取消、原ID只读恢复与人工核销，保留Unknown责任；⑥仅在具体新合同/实现差异存在后作相应最小验证与独审，另行授权真实调用。禁止预先增加验证器或重新跑基础接入。

四态回执的最小共同内容（未来要求，不声称现有全具备）：R预留=批准版本/合同SHA/报价有效时点+target/model/service+operation/request/attempt/round+完整报文SHA+计数责任与计数原回执+真实ceiling证明+预留金额及ledger前后head；S结算=原R关联+供应商/网关/响应ID映射+完整互斥usage（含隐藏推理定义）+actual model/service/价带+账单金额/来源/最终性+去重标识+持久FeeAck；C取消=原R+cancel intent/本地送达与远端ACK及截止时点+最终收费/退款账单，缺收费证明则Unknown不释放；Q恢复=原R/原intent/journal/head+只读原请求查询回执+迟到账单与去重核销+剩余责任，绝不重新生成。

| 出口→实际源码 | 现在缺的合同事实 | 合同到位后模块及先后依赖 | R/S/C/Q各态的出口特定回执 |
| --- | --- | --- | --- |
| E1 API工人及多供应商副管理→ApiClient::send_json_captured，api_generation.rs:572–604；async_api.rs:204–256，deputy_management_host.rs:64、137–149 | C01–C11，特别A6隐藏推理/缓存/网关路由与上界 | domain计量→api_currency/host→ApiCallJournal/scoped→ApiClient/gateway→api_observations→resolution；副管理也必须接同一经济责任，不另造客户端 | R逐轮工人/管理原请求；S原response与网关ID、billable_usage及FeeAck；C API oneshot只属本地，补供应商收费回执（副管理当前无外部取消信号传入）；Q原completion/journal与账单，不POST |
| E2 API工人独立计数→api_generation/scoped.rs:21–22、125–155 /responses/input_tokens | C02/C03/C06计数收费和真实输入上界；仅官方OpenAI支持 | ManagementCountPrice/计数合同→api_currency_host Count→count journal→count_scoped→generation R | R计数本身最高责任和body SHA；S原count原usage/费用与证据；C取消也要交回原count结果；Q按原count查账或人工核销，禁止重数 |
| E3 OpenAI主副管理生成→openai/evidence.rs:139–146、scoped_pricing/transport.rs:122–132，经openai.rs:185–201 POST /responses | C01–C11，显式服务/价带/隐藏推理/管理取消 | domain→management_preflight_host/scoped pricing→原management journal→OpenAiClient→currency observation→decision应用 | R绑定Plan或批准revision/管理purpose；S先账单后应用决定；C当前管理方法无工人oneshot参数，需单独取消及费用责任回执；Q原operation/decision与账本，不重做决定 |
| E4 OpenAI管理独立计数→scoped_pricing/transport.rs:140起，management_preflight_host.rs:218–378 | C03/C06计数自身收费/计量可靠性；计数免费unknown | count条款→begin_management_count:284→计数→finish:343原回执持久化/346–349重校验→generation预留 | R count最高费用/body证据；S count原回执含收费；C失败/断连转Unknown保留；Q原operation核销与继续责任，retire不等于免单 |
| E5 Codex app-server→codex.rs:633–655启动、814–816 turn/start | C01–C11，订阅/用量收费与多次内部尝试、interrupt费用、可证明上限 | Codex经济能力/计量合同→Host金额门/批准→codex_execution DispatchIntent→CodexClient→经济回执→取消恢复桥 | R thread/attempt/clientUserMessageId与会话范围上界；S远端终态加账单（终态本身不够）；C turn/interrupt ACK加最终收费；Q新readonly连接读原thread/attempt，缺失不重发 |
| E6 OMP外部进程→omp_process.rs:217–272；omp_transport/worker.rs:224–245 prompt | C01–C11，OMP内部供应商收费和会话计费、abort及上界 | 供应商+OMP版本计量契约→Host/omp_execution→WorkerProcess原intent→RPC→经济账本→恢复 | R进程/版本/provider/model与原prompt SHA；S原WorkerEvent/response加供应商账单；C AbortIntent+abort ACK+收费；Q原OmpJournal/terminal/账单，不prompt，保留原恢复失败 |
| E7 Qoder桌面→DesktopTrial.cs:435 Invoke，经desktop_sessions.rs:765 TaskSend | C01–C11，桌面Token Plan/订阅单位及可观察账单、隐藏路由/硬上界 | 先可执行经济合同/用户预算→桌面Host授权及单次permit→native发送→只读关联账单→Stop/TaskRead核销 | R已绑定窗口/session/历史指纹及review digest、经济上界；S回复相关性加独立账单；C唯一Stop Invoke/观察+收费；Q TaskRead原attempt与账单，不能重构发送permit |
| E8 WorkBuddy桌面→DesktopTrial.cs:434、335–340 ClickWorkBuddySend | C01–C11，同桌面合同但不得继承Qoder计价/取消语义 | WorkBuddy独立条款→共用desktop_background/desktop_execution→native点击→经济观察→desktop_stop_host | R原会话/目标/批准与WorkBuddy独立额度；S原回复/收费；C StopPrepare/Stop/StopRead及收费最终性；Q原TaskRead/历史锚点/账单，不再点击发送 |
| E9 非生成发现/检查→provider_models.rs:98–123 GET models；openai.rs:233–234；core/main.rs:36–66 Codex检查连接（没有turn/start） | C05/C06/C10，发现/连接是否收费也unknown | 确认免费或独立收费上界→连接/发现边界→若收费复用R/S/C/Q；不得借检查发生成 | R有确认免费的引用或独立费用责任；S发现响应及费用证明；C本地停止不当退款；Q只认原连接/查询与收费，不把modelCalls=0当零费用证明 |

E1固定ApiTarget映射在api_worker.rs:21–54，覆盖OpenAI Responses、Anthropic、DeepSeek、Moonshot、A6API、Grok与Qwen三个地域；每一供应商/地域合同独立确认。其他厂商原生CLI无实现/协议事实，不虚构已存在出口，338:41、73所述条件保持。HTTP发现、进程启动和桌面委托也须在经济责任边界登记，不能仅接两套Rust HTTP就宣称全出口完成。

现有账本不是空白：currency_budget.rs:434–470保留Unknown且仅可证明未发送的Reserved可释放；1001–1002明确cancel/HTTP/timeout不是未发送证明；527–544结算成功才释放，Unpriced阻塞且保留。api_observations:236–249要求费用确认才放工具/下一轮。以上可复用，但本轮没有开通生产金额，也没有验证供应商保证。

## 历史失败完整保留

- 328 workspace原1105 passed / 1 failed / 274 ignored，exit101。原stdout:147为143 passed，1400为962 passed/1 failed/274 ignored，1105是合计；1393–1394保留OMP directory pin unavailable；receipt:20 exit101。之后精确单项通过不是根因修复。
- 324原86 passed / 1 failed / 30 ignored，exit101；core_affected.stdout:125–133 terminal.accepted断言失败，receipt:23 exit101。
- 341首次 package/result.json:19–20 failed/exitCode1，installer.stderr:13 Unreviewed compiler or bundler routing environment；reviewed-route/package/result.json:19–20 failed/exitCode1，stderr:5 Rust is unavailable。installer.*是历史命名，不能称本轮执行安装器。后续候选包通过不抹去两次exit1。
- 336/337 result.json:7均rootCauseKnown=false；336 review:5、9无新因果证据，337 review:5、7原现场未保存且无新确定根因。原328/324失败继续保留。本轮不重跑也不归因为经济合同问题。

## 收口与下一动作

价格、计量、隐藏推理包含关系、缓存/路由费、计数硬上界、晚回/取消/恢复核销、逐笔账单和余额时效、责任与止损选择均无法在无合同/无外部事实下确定。供应商确认条款与官方材料后，由用户批准预算、责任和止损；随后按E2/E4计数责任→E1/E3生成→E5/E6→E7/E8及E9边界逐出口实施，条件不足的出口保持拒绝，不自行调整范围。

独立审阅状态由同目录独立审阅文件与result.json记载。清单准备完成不能关闭P3-MONEY；正式27门槛/13配置及凭证结束清理任务继续继承。unexpectedChanges=[]仅表示本轮未发现或制造范围外改动，不声称审计了并发会话所有活动。
