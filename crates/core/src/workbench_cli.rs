//! User CLI over the same authenticated IPC as the desktop. No workflow owner.
use cerulean_core::ipc::{Client, UserOperation};
use cerulean_domain::{
    credentials::{CredentialProvider, ProviderEndpoint},
    execution::Project,
    project_setup::ProjectSetupDraft,
    secret::SecretInput,
    Command, Request, Snapshot, PROTOCOL_VERSION,
};
use serde::{de::DeserializeOwned, Deserialize};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs::File, io::Read, path::PathBuf};

const HELP: &str = r#"CeruleanLonging 工作台 CLI
cerulean-runtime.exe --workbench --data-dir <核心数据目录> <命令> [选项]

service start | service stop            启动/连接同一核心；有未决任务时停止被拒绝
service review-preserved-stop
service stop-preserving --review-digest <原审阅摘要>  保存未决记录并关闭；不取消或重发任务
external-chief prepare --project <项目ID> --setup-digest <配置摘要> --session <当前Codex会话标识>
external-chief read --operation <原操作ID>
external-chief submit --operation <原操作ID> --session <原会话标识> --file <完整决策JSON>
                                       只提出方案，不代替用户批准；失败先读取原操作，不重发

verification-recheck review-disposition --project <项目ID> --operation <原复验ID> --reason <原因>
verification-recheck retire --file <原处置审阅result JSON> --event-operation <独立处置ID>
                                       处置仅结束原复验批准，不验收产物；回执未知用show查原事件，不重新提交
verification-recheck review --file <RecheckRequest JSON>
verification-recheck approve --file <原审阅result JSON> --approval-operation <独立批准ID> --operation <单次复验ID>
verification-recheck show --project <项目ID> --operation <原复验ID>
verification-recheck start --project <项目ID> --operation <已批准复验ID>
verification-recheck cancel --project <项目ID> --operation <原复验ID>
verification-recheck recover --project <项目ID> --operation <原复验ID>
                                       review仅审阅；approve仅批准并暂停后续调度；start才执行一次本地检查
                                       show核对原批准/启动状态；recover仅补记原结果，取消或通信失败不得重跑
data-format review-management-source --from <冷备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format promote-management-source --from <冷备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format review-rejected-usage --from <冷备份目录> --manifest-digest <摘要> [--hold-digest <原锁定摘要>]
data-format review-verification-recheck --from <冷备份目录> --manifest-digest <摘要> [--hold-digest <原锁定摘要>]
data-format promote-verification-recheck --from <冷备份目录> --manifest-digest <摘要> --review-digest <摘要> [--hold-digest <原锁定摘要>]
data-format promote-rejected-usage --from <冷备份目录> --manifest-digest <摘要> --review-digest <摘要> [--hold-digest <原锁定摘要>]
data-format review-relay --from <冷备份目录> --manifest-digest <摘要>
data-format promote-relay --from <冷备份目录> --manifest-digest <摘要> --review-digest <摘要>
data-format review-experiments --from <冷备份目录> --manifest-digest <摘要>
data-format promote-experiments --from <冷备份目录> --manifest-digest <摘要> --review-digest <摘要>
                                       仅追加当前实验保护；须停止核心，原数据与备份逐字节一致；不自动执行
team-import review-retention --from <冷备份> --manifest-digest <摘要> --to <新决定目录> --reason <保留原因>
team-import retain --from <同一冷备份> --manifest-digest <摘要> --to <同一新目录> --reason <同一原因> --review-digest <摘要>
team-import inspect-retention --from <同一冷备份> --manifest-digest <摘要> --to <原决定目录> --reason <同一原因> --review-digest <摘要>
                                       停止核心后核对失败导入并记录保留决定；原目录继续只读，不重新导入或执行
evidence-archive create --index <冻结索引> --index-sha256 <另存摘要> --to <源根之外的新目录>
evidence-archive verify --from <归档目录> --manifest-sha256 <另存清单摘要>
                                       离线操作；create以--data-dir为源证据根，verify不访问--data-dir；不连接核心、不执行载荷
api-generation list [--offset <偏移>] [--revision <原清单版本>]
api-generation review --file <生成费用输入JSON>
api-generation confirm --file <同一输入JSON> --review-digest <已审阅摘要>
api-generation show --project <项目ID> --request <原API请求ID> --review-digest <原确认摘要>
api-count list [--offset <偏移>] [--revision <原清单版本>]
api-count review --file <计数费用输入JSON>
api-count confirm --file <同一输入JSON> --review-digest <已审阅摘要>
api-count show --project <项目ID> --request <原API请求ID> --review-digest <原确认摘要>
backup create --to <新目录>             停止核心后备份数据；不包含外部项目
backup verify --from <备份目录>         核对完整清单与文件摘要
backup restore --from <备份目录> --manifest-digest <已审阅摘要>
                                       同用户恢复至原位置；目标目录必须不存在
upgrade review                         审阅升级阻塞项和状态摘要
upgrade prepare --review-digest <摘要>  持久锁定并等待原核心退出；不安装新版本
upgrade status                         离线读取锁定记录
upgrade backup --hold-digest <摘要> --to <新备份目录>
upgrade release --hold-digest <摘要>    明确解除锁定；保留历史，不自动启动
recovery review --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
recovery prepare --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
recovery review-previous --from <备份目录> --manifest-digest <摘要> --hold-digest <当前锁定摘要> --backup-hold-digest <备份原锁定摘要>
recovery prepare-previous --from <备份目录> --manifest-digest <摘要> --hold-digest <当前锁定摘要> --backup-hold-digest <备份原锁定摘要> --review-digest <摘要>
recovery status                        只读核对同一恢复事务；不自动重发
recovery diagnose                      只读检查残缺准备；给出处置入口，不清理或解锁
recovery inspect --review-digest <摘要> 只读核对活动事务与归档；历史成功不代表新事务完成
recovery apply --review-digest <摘要>   保留原目录并恢复；失败先读取status
recovery finish --review-digest <摘要>  核对恢复并归档事务；原hold仍需明确解除
recovery abandon --review-digest <摘要> 仅放弃尚未尝试移动的准备；保留全部证据
maintenance status [--guard-digest <摘要>] 只读核对程序更新占用及其释放记录
maintenance reserve --transaction <目录> --intent-digest <摘要> --hold-digest <摘要>
maintenance check --guard-digest <摘要> 核对占用及原数据；不启动或更新程序
maintenance release --guard-digest <摘要> --plan-digest <摘要>
                                       独立核对提交/回滚终态后归档占用
maintenance abandon --guard-digest <摘要> 仅放弃尚无执行事件、原数据/程序未变的准备
data-format status                     离线核对活动数据格式；不启动核心
data-format review --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format migrate --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format review-reader --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format promote-reader --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format review-currency --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format promote-currency --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format review-preparation --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format promote-preparation --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format review-counting --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format promote-counting --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format review-count-resolution --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format promote-count-resolution --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format review-scoped-pricing --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format review-scoped-count --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format review-scoped-execution --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format review-api-currency --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format review-api-fee-resolution --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要>
data-format promote-scoped-pricing --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format promote-scoped-count --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format promote-scoped-execution --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format promote-api-currency --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
data-format promote-api-fee-resolution --from <备份目录> --manifest-digest <摘要> --hold-digest <摘要> --review-digest <摘要>
                                       显式迁移容器1到2；保留事件字节和升级锁定
status                                 原核心完整快照
workspace overview                     独立工作区概览，不依赖完整快照
workspace plans|projects [--offset <序号>] [--revision <原页版本>]
workspace revision-reviews --project <项目ID> [--offset <序号>] [--revision <查询版本>]
workspace revision-review --project <项目ID> --revision <查询版本>
workspace unsent-assignments --project <项目ID> [--offset <序号>] [--revision <查询版本>]
workspace unsent-assignment --project <项目ID> --task <任务ID> --attempt <尝试ID> --revision <查询版本>
workspace management-record-reviews [--offset <序号>] [--revision <查询版本>]
workspace management-record-review --relative-path <原相对路径> --revision <查询版本>
workspace api-publication-reviews [--offset <序号>] [--revision <查询版本>]
workspace api-publication-review --relative-path <原相对路径> --revision <查询版本>
workspace codex-cancellations --project <项目ID> [--offset <序号>] [--revision <查询版本>]
workspace codex-cancellation --project <项目ID> --attempt <尝试ID> --revision <查询版本>
workspace plan --id <计划ID> --revision <原列表版本>  只读完整计划
workspace tasks --project <项目ID> [--offset <序号>] [--revision <原任务页版本>]
workspace task --project <项目ID> --task <任务ID> --revision <原任务页版本>
workspace task-evidence --project <项目ID> --task <任务ID> --revision <原任务页版本> [--offset <序号>]
workspace execution-review --project <项目ID> [--digest <原执行方案摘要>]
workspace jobs --project <项目ID> [--offset <偏移>] [--revision <查询版本>]
workspace job --project <项目ID> --kind <codex|omp|api|verification> --id <原尝试或验收操作ID> --revision <查询版本>
workspace recoveries --project <项目ID> [--offset <偏移>] [--revision <查询版本>]
workspace codex-startups --project <项目ID> [--offset <偏移>] [--revision <查询版本>]
workspace codex-startup --project <项目ID> --attempt <原尝试ID> --revision <查询版本>
workspace management-decisions --project <项目ID> [--offset <偏移>] [--revision <查询版本>]
workspace management-decision --project <项目ID> --operation <管理操作ID> --revision <查询版本>
workspace management-gateway-evidence --project <项目ID> --operation <管理操作ID> --revision <原管理记录版本>
  网关留证只读；版本绑定原管理记录，旁路为本次读取观察，不代表账单核销。
workspace project-runs --project <项目ID> [--offset <偏移>] [--revision <查询版本>]
workspace project-run --project <项目ID> --run <原运行ID> --revision <查询版本>
workspace recovery --project <项目ID> --kind <managementRecord|startupClosure|startupReservation|ompStop> --id <原恢复记录ID或路径> --revision <查询版本>
workspace worker-observation --project <项目ID> --attempt <原尝试ID> [--revision <查询版本>]
workspace worker-observation-events --project <项目ID> --attempt <原尝试ID> --revision <查询版本> [--offset <偏移>]
workspace worker-observation-incidents --project <项目ID> --attempt <原尝试ID> --revision <查询版本> [--offset <偏移>]
workspace verification-result --project <项目ID> --operation <验收操作ID> [--revision <查询版本>]
workspace verification-result-artifacts --project <项目ID> --operation <验收操作ID> --revision <查询版本> [--offset <偏移>]
workspace verification-result-stages --project <项目ID> --operation <验收操作ID> --revision <查询版本> [--offset <偏移>]
providers list                         全部接入与凭证状态（不返回密钥）
providers check --provider openai       检查 OpenAI 官方模型目录
providers check --file endpoint.json    检查其他供应商的已支持官方端点
credentials set --provider <供应商>     从标准输入读取 Key，结尾可有一次换行
credentials remove --provider <供应商>
project add --id <ID> --root <项目目录> [--timezone <IANA时区，默认Asia/Shanghai>]
project show --project <ID>             原方案、批准、配置、运行和任务状态
validator add --file validator.json     登记独立验收器，使用桌面相同契约
setup save --file setup.json            保存 ProjectSetupDraft，不批准或运行
setup show --project <ID>
plan request --project <ID> --setup-digest <SHA256>    请求主指挥，可能产生费用
history review --file input.json        只读审阅完整历史金额与来源
history confirm --file input.json --review-digest <摘要>  明确确认原审阅，不发送模型任务
history sources|accounted [--offset <序号>] [--revision <原页版本>]
history show --project <ID> --scope-file scope.json --review-digest <原审阅摘要>
            只读回查原确认；未找到不等于免费，不自动重发或补记
plan authorize-budget --project <ID> --setup-digest <SHA256>  仅批准规划金额许可，不发送请求
plan show --project <ID>                 审阅完整方案及 revisionDigest
plan approve --project <ID> --digest <revisionDigest>
run start|resume --project <ID> --revision-digest <SHA256> --setup-digest <SHA256>
run pause --run <运行ID>                 暂停后续调度，继续收集已发送任务
task recover --adapter codex|omp|api|desktop --project <ID> --attempt <原尝试ID>
task cancel --adapter codex|omp|api|desktop --project <ID> --attempt <ID> --reason <原因>
            Codex/desktop 另需 --evidence-digest <当前停止审阅摘要>
management list [--project <ID>] [--offset <序号>] [--revision <摘要>]
management show --operation <ID>
management answer --operation <ID> --evidence-digest <摘要> --answer <补充说明>
management discard --operation <ID> --evidence-digest <摘要> --reason <原因>
verification show --project <ID> --operation <ID>
verification recover --project <ID> --operation <ID> --evidence-digest <摘要>
verification cancel --project <ID> --operation <ID>

供应商：openai（GPT/主副指挥）、anthropic（Claude）、deepseek、qwen、kimi、grok、a6api（A6API中转）。
模型、工具、产物、预算和执行器由 setup 配置，完整方案仍须用户确认。
每条命令只发送一次；通信失败不自动重发。后台任务请用 status 查询。
除 service start 外不自动启动核心。不自动选择批准摘要或替用户确认方案。
退出码：0 请求成功；2 参数/输入错误；3 核心拒绝；4 通信失败；5 输出失败。
标准输出为 JSON；错误为标准错误 JSON。--help 不连接服务。
"#;

struct Options(BTreeMap<String, String>);
impl Options {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut options = BTreeMap::new();
        if !args.len().is_multiple_of(2) {
            return Err("每个选项必须有一个值；请使用 --help".into());
        }
        for pair in args.as_chunks::<2>().0 {
            if !pair[0].starts_with("--") || pair[1].is_empty() {
                return Err("选项格式无效".into());
            }
            if options.insert(pair[0].clone(), pair[1].clone()).is_some() {
                return Err("不能重复指定选项".into());
            }
        }
        Ok(Self(options))
    }
    fn take(&mut self, key: &str) -> Result<String, String> {
        self.0
            .remove(key)
            .ok_or_else(|| format!("缺少必需选项 {key}"))
    }
    fn optional(&mut self, key: &str) -> Option<String> {
        self.0.remove(key)
    }
    fn done(self) -> Result<(), String> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err("含不支持的选项；输入未发送".into())
        }
    }
}
enum View {
    Full,
    Providers,
    Project(String),
    VerificationRecheckDispositionReview,
    VerificationRecheckReview,
    VerificationRecheckStatus,
}
enum Action {
    EvidenceArchiveCreate(PathBuf, String, PathBuf),
    EvidenceArchiveVerify(PathBuf, String),
    ExternalChief(cerulean_domain::management_ingress::ExternalChiefOperation),
    ManagementSourceReview(PathBuf, String, String),
    ManagementSourcePromote(PathBuf, String, String, String),
    RejectedUsageReview(PathBuf, String, Option<String>),
    RejectedUsagePromote(PathBuf, String, String, Option<String>),
    VerificationRecheckReview(PathBuf, String, Option<String>),
    VerificationRecheckPromote(PathBuf, String, String, Option<String>),
    RelayReview(PathBuf, String),
    RelayPromote(PathBuf, String, String),
    ExperimentsReview(PathBuf, String),
    ExperimentsPromote(PathBuf, String, String),
    ImportRetentionReview(PathBuf, String, PathBuf, String),
    ImportRetain(PathBuf, String, PathBuf, String, String),
    ImportRetentionInspect(PathBuf, String, PathBuf, String, String),
    Workspace(cerulean_domain::workspace::WorkspaceQuery),
    MaintenanceStatus(Option<String>),
    MaintenanceReserve(PathBuf, String, String),
    MaintenanceCheck(String),
    MaintenanceRelease(String, Option<String>),
    RecoveryReview(PathBuf, String, String),
    RecoveryPrepare(PathBuf, String, String, String),
    RecoveryPreviousReview(PathBuf, String, String, String),
    RecoveryPreviousPrepare(PathBuf, String, String, String, String),
    RecoveryStatus,
    RecoveryDiagnose,
    RecoveryInspect(String),
    RecoveryApply(String),
    RecoveryFinish(String, bool),
    DataFormatStatus,
    DataFormatReview(PathBuf, String, String),
    DataFormatMigrate(PathBuf, String, String, String),
    ReaderReview(PathBuf, String, String),
    ReaderPromote(PathBuf, String, String, String),
    CurrencyReview(PathBuf, String, String),
    CurrencyPromote(PathBuf, String, String, String),
    PreparationReview(PathBuf, String, String),
    PreparationPromote(PathBuf, String, String, String),
    CountingReview(PathBuf, String, String),
    CountingPromote(PathBuf, String, String, String),
    CountResolutionReview(PathBuf, String, String),
    CountResolutionPromote(PathBuf, String, String, String),
    ScopedPricingReview(PathBuf, String, String),
    ScopedCountReview(PathBuf, String, String),
    ScopedExecutionReview(PathBuf, String, String),
    ApiCurrencyReview(PathBuf, String, String),
    ApiFeeResolutionReview(PathBuf, String, String),
    ScopedPricingPromote(PathBuf, String, String, String),
    ScopedCountPromote(PathBuf, String, String, String),
    ScopedExecutionPromote(PathBuf, String, String, String),
    ApiCurrencyPromote(PathBuf, String, String, String),
    ApiFeeResolutionPromote(PathBuf, String, String, String),
    PreservedShutdownReview,
    PreservedShutdown(String),
    UpgradeReview,
    UpgradePrepare(String),
    UpgradeStatus,
    UpgradeBackup(PathBuf, String),
    UpgradeRelease(String),
    BackupCreate(PathBuf),
    BackupVerify(PathBuf),
    BackupRestore(PathBuf, String),
    Start,
    Stop,
    Request(Box<Command>),
    SetKey(CredentialProvider),
}
struct Invocation {
    directory: PathBuf,
    action: Action,
    view: View,
}
fn json_file<T: DeserializeOwned>(path: &str) -> Result<T, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "无法打开输入 JSON 文件")?
        .take(60 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取输入 JSON 文件")?;
    if bytes.len() > 60 * 1024 {
        return Err("输入 JSON 超过 60 KiB".into());
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
    // Do not print deserializer errors: they may quote unknown input values.
    serde_json::from_slice(bytes).map_err(|_| "JSON 不符合命令契约；请核对字段、类型及版本".into())
}
fn provider(name: &str) -> Result<CredentialProvider, String> {
    serde_json::from_value(json!(name)).map_err(|_| "不支持的供应商标识".into())
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Validator {
    id: String,
    name: String,
    executable: String,
    #[serde(default)]
    inputs: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
}
fn parse(args: &[String]) -> Result<Invocation, String> {
    if args.len() < 3 || args[0] != "--data-dir" || args[1].is_empty() {
        return Err("请先指定 --data-dir <核心数据目录>；使用 --help 查看命令".into());
    }
    let directory = PathBuf::from(&args[1]);
    let family = args[2].as_str();
    let (verb, tail) = if family == "status" {
        ("", &args[3..])
    } else {
        (args.get(3).ok_or("缺少子命令")?.as_str(), &args[4..])
    };
    let mut o = Options::parse(tail)?;
    let mut view = View::Full;
    let action = match (family, verb) {
        ("evidence-archive", "create") => Action::EvidenceArchiveCreate(
            o.take("--index")?.into(),
            o.take("--index-sha256")?,
            o.take("--to")?.into(),
        ),
        ("evidence-archive", "verify") => {
            Action::EvidenceArchiveVerify(o.take("--from")?.into(), o.take("--manifest-sha256")?)
        }
        ("team-import", "review-retention") => Action::ImportRetentionReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--to")?.into(),
            o.take("--reason")?,
        ),
        ("team-import", "retain") => Action::ImportRetain(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--to")?.into(),
            o.take("--reason")?,
            o.take("--review-digest")?,
        ),
        ("team-import", "inspect-retention") => Action::ImportRetentionInspect(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--to")?.into(),
            o.take("--reason")?,
            o.take("--review-digest")?,
        ),
        ("external-chief", "prepare") => Action::ExternalChief(
            cerulean_domain::management_ingress::ExternalChiefOperation::PreparePlan {
                project_id: o.take("--project")?,
                setup_digest: o.take("--setup-digest")?,
                session_id: o.take("--session")?,
            },
        ),
        ("external-chief", "read") => Action::ExternalChief(
            cerulean_domain::management_ingress::ExternalChiefOperation::Read {
                operation_id: o.take("--operation")?,
            },
        ),
        ("external-chief", "submit") => Action::ExternalChief(
            cerulean_domain::management_ingress::ExternalChiefOperation::Submit {
                operation_id: o.take("--operation")?,
                session_id: o.take("--session")?,
                decision: json_file(&o.take("--file")?)?,
            },
        ),
        ("data-format", "review-verification-recheck") => Action::VerificationRecheckReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.optional("--hold-digest"),
        ),
        ("data-format", "promote-verification-recheck") => Action::VerificationRecheckPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--review-digest")?,
            o.optional("--hold-digest"),
        ),
        ("data-format", "review-rejected-usage") => Action::RejectedUsageReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.optional("--hold-digest"),
        ),
        ("data-format", "promote-rejected-usage") => Action::RejectedUsagePromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--review-digest")?,
            o.optional("--hold-digest"),
        ),
        ("data-format", "review-relay") => {
            Action::RelayReview(o.take("--from")?.into(), o.take("--manifest-digest")?)
        }
        ("data-format", "promote-relay") => Action::RelayPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "review-experiments") => {
            Action::ExperimentsReview(o.take("--from")?.into(), o.take("--manifest-digest")?)
        }
        ("data-format", "promote-experiments") => Action::ExperimentsPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "review-management-source") => Action::ManagementSourceReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "promote-management-source") => Action::ManagementSourcePromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("workspace", "overview") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::Overview)
        }
        ("workspace", "plans" | "projects") => {
            let offset = o
                .optional("--offset")
                .map(|v| v.parse::<u32>().map_err(|_| "工作区页偏移无效"))
                .transpose()?
                .unwrap_or(0);
            let revision = o.optional("--revision");
            Action::Workspace(if verb == "plans" {
                cerulean_domain::workspace::WorkspaceQuery::Plans { offset, revision }
            } else {
                cerulean_domain::workspace::WorkspaceQuery::Projects { offset, revision }
            })
        }
        ("workspace", "revision-reviews") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::RevisionReviews {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|v| v.parse::<u32>().map_err(|_| "修订审阅页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
        ),
        ("workspace", "revision-review") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::RevisionReview {
                project_id: o.take("--project")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "codex-cancellations") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::CodexCancellations {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|v| v.parse::<u32>().map_err(|_| "停止审阅页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
        ),
        ("workspace", "codex-cancellation") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::CodexCancellation {
                project_id: o.take("--project")?,
                attempt_id: o.take("--attempt")?,
                revision: o.take("--revision")?,
            },
        ),
        ("workspace", "unsent-assignments") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::UnsentAssignments {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|v| v.parse::<u32>().map_err(|_| "未派发尝试页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
        ),
        ("workspace", "management-record-reviews") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::ManagementRecordReviews {
                offset: o
                    .optional("--offset")
                    .map(|v| v.parse::<u32>().map_err(|_| "管理记录复核页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
        ),
        ("workspace", "management-record-review") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::ManagementRecordReview {
                relative_path: o.take("--relative-path")?,
                revision: o.take("--revision")?,
            },
        ),
        ("workspace", "api-publication-reviews") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::ApiPublicationReviews {
                offset: o
                    .optional("--offset")
                    .map(|v| v.parse::<u32>().map_err(|_| "API发布审阅页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
        ),
        ("workspace", "api-publication-review") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::ApiPublicationReview {
                relative_path: o.take("--relative-path")?,
                revision: o.take("--revision")?,
            },
        ),
        ("workspace", "unsent-assignment") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::UnsentAssignment {
                project_id: o.take("--project")?,
                task_id: o.take("--task")?,
                attempt_id: o.take("--attempt")?,
                revision: o.take("--revision")?,
            },
        ),
        ("workspace", "plan") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::Plan {
                id: o.take("--id")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "history") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::History {
                project_id: o.optional("--project"),
                offset: o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>().map_err(|_| "历史页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            })
        }
        ("workspace", "history-entry") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::HistoryEntry {
                project_id: o.optional("--project"),
                sequence: o
                    .take("--sequence")?
                    .parse::<u32>()
                    .map_err(|_| "历史序号无效")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "configurations") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::Configurations {
                offset: o
                    .optional("--offset")
                    .map(|v| v.parse::<u32>().map_err(|_| "配置页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            })
        }
        ("workspace", "configuration") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::Configuration {
                project_id: o.take("--project")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "tasks") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::Tasks {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>().map_err(|_| "任务页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            })
        }
        ("workspace", "task") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::Task {
                project_id: o.take("--project")?,
                task_id: o.take("--task")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "task-evidence") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::TaskEvidence {
                project_id: o.take("--project")?,
                task_id: o.take("--task")?,
                revision: o.take("--revision")?,
                offset: o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>().map_err(|_| "证据页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
            })
        }
        ("workspace", "execution-review") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::ExecutionReview {
                project_id: o.take("--project")?,
                digest: o.optional("--digest"),
            },
        ),
        ("workspace", "jobs") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::Jobs {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>().map_err(|_| "作业页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            })
        }
        ("workspace", "codex-startups") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::CodexStartups {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>().map_err(|_| "启动页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            })
        }
        ("workspace", "codex-startup") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::CodexStartup {
                project_id: o.take("--project")?,
                attempt_id: o.take("--attempt")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "management-decisions") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::ManagementDecisions {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>().map_err(|_| "管理结果页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
        ),
        ("workspace", "management-decision") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::ManagementDecision {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
                revision: o.take("--revision")?,
            },
        ),
        ("workspace", "management-gateway-evidence") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::ManagementGatewayEvidence {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
                revision: o.take("--revision")?,
            },
        ),
        ("workspace", "project-runs") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::ProjectRuns {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>().map_err(|_| "项目运行页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            })
        }
        ("workspace", "project-run") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::ProjectRun {
                project_id: o.take("--project")?,
                run_id: o.take("--run")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "job") => {
            use cerulean_domain::workspace::{WorkspaceJobKind, WorkspaceQuery};
            let kind = match o.take("--kind")?.as_str() {
                "codex" => WorkspaceJobKind::Codex,
                "omp" => WorkspaceJobKind::Omp,
                "api" => WorkspaceJobKind::Api,
                "verification" => WorkspaceJobKind::Verification,
                _ => return Err("作业类型必须是codex/omp/api/verification".into()),
            };
            Action::Workspace(WorkspaceQuery::Job {
                project_id: o.take("--project")?,
                kind,
                id: o.take("--id")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "recoveries") => {
            Action::Workspace(cerulean_domain::workspace::WorkspaceQuery::Recovery {
                project_id: o.take("--project")?,
                offset: o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>().map_err(|_| "恢复页偏移无效"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            })
        }
        ("workspace", "recovery") => {
            use cerulean_domain::workspace::{WorkspaceQuery, WorkspaceRecoveryKind};
            let kind =
                match o.take("--kind")?.as_str() {
                    "managementRecord" => WorkspaceRecoveryKind::ManagementRecord,
                    "startupClosure" => WorkspaceRecoveryKind::StartupClosure,
                    "startupReservation" => WorkspaceRecoveryKind::StartupReservation,
                    "ompStop" => WorkspaceRecoveryKind::OmpStop,
                    _ => return Err(
                        "恢复类型必须是managementRecord/startupClosure/startupReservation/ompStop"
                            .into(),
                    ),
                };
            Action::Workspace(WorkspaceQuery::RecoveryDetail {
                project_id: o.take("--project")?,
                kind,
                id: o.take("--id")?,
                revision: o.take("--revision")?,
            })
        }
        ("workspace", "worker-observation") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::WorkerObservation {
                project_id: o.take("--project")?,
                attempt_id: o.take("--attempt")?,
                revision: o.optional("--revision"),
            },
        ),
        ("workspace", "worker-observation-events" | "worker-observation-incidents") => {
            use cerulean_domain::workspace::WorkspaceQuery;
            let project_id = o.take("--project")?;
            let attempt_id = o.take("--attempt")?;
            let revision = o.take("--revision")?;
            let offset = o
                .optional("--offset")
                .map(|s| s.parse::<u32>().map_err(|_| "观察页偏移无效"))
                .transpose()?
                .unwrap_or(0);
            Action::Workspace(if verb == "worker-observation-events" {
                WorkspaceQuery::WorkerObservationEvents {
                    project_id,
                    attempt_id,
                    offset,
                    revision,
                }
            } else {
                WorkspaceQuery::WorkerObservationIncidents {
                    project_id,
                    attempt_id,
                    offset,
                    revision,
                }
            })
        }
        ("workspace", "verification-result") => Action::Workspace(
            cerulean_domain::workspace::WorkspaceQuery::VerificationResult {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
                revision: o.optional("--revision"),
            },
        ),
        ("workspace", "verification-result-artifacts" | "verification-result-stages") => {
            use cerulean_domain::workspace::WorkspaceQuery;
            let project_id = o.take("--project")?;
            let operation_id = o.take("--operation")?;
            let revision = o.take("--revision")?;
            let offset = o
                .optional("--offset")
                .map(|value| value.parse::<u32>().map_err(|_| "验收结果页偏移无效"))
                .transpose()?
                .unwrap_or(0);
            Action::Workspace(if verb == "verification-result-artifacts" {
                WorkspaceQuery::VerificationResultArtifacts {
                    project_id,
                    operation_id,
                    offset,
                    revision,
                }
            } else {
                WorkspaceQuery::VerificationResultStages {
                    project_id,
                    operation_id,
                    offset,
                    revision,
                }
            })
        }
        ("maintenance", "status") => Action::MaintenanceStatus(o.optional("--guard-digest")),
        ("maintenance", "reserve") => Action::MaintenanceReserve(
            o.take("--transaction")?.into(),
            o.take("--intent-digest")?,
            o.take("--hold-digest")?,
        ),
        ("maintenance", "check") => Action::MaintenanceCheck(o.take("--guard-digest")?),
        ("maintenance", "release") => {
            Action::MaintenanceRelease(o.take("--guard-digest")?, Some(o.take("--plan-digest")?))
        }
        ("maintenance", "abandon") => Action::MaintenanceRelease(o.take("--guard-digest")?, None),
        ("recovery", "review") => Action::RecoveryReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("recovery", "review-previous") => Action::RecoveryPreviousReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--backup-hold-digest")?,
        ),
        ("recovery", "prepare-previous") => Action::RecoveryPreviousPrepare(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--backup-hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("recovery", "prepare") => Action::RecoveryPrepare(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("recovery", "status") => Action::RecoveryStatus,
        ("recovery", "diagnose") => Action::RecoveryDiagnose,
        ("recovery", "inspect") => Action::RecoveryInspect(o.take("--review-digest")?),
        ("recovery", "apply") => Action::RecoveryApply(o.take("--review-digest")?),
        ("recovery", "finish") => Action::RecoveryFinish(o.take("--review-digest")?, false),
        ("recovery", "abandon") => Action::RecoveryFinish(o.take("--review-digest")?, true),
        ("data-format", "status") => Action::DataFormatStatus,
        ("data-format", "review-reader") => Action::ReaderReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "review-preparation") => Action::PreparationReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "promote-preparation") => Action::PreparationPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "review-counting") => Action::CountingReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "promote-counting") => Action::CountingPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "review-count-resolution") => Action::CountResolutionReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "promote-count-resolution") => Action::CountResolutionPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "review-scoped-pricing") => Action::ScopedPricingReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "promote-scoped-pricing") => Action::ScopedPricingPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "review-scoped-count") => Action::ScopedCountReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "review-scoped-execution") => Action::ScopedExecutionReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "review-api-currency") => Action::ApiCurrencyReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "review-api-fee-resolution") => Action::ApiFeeResolutionReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "promote-scoped-count") => Action::ScopedCountPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "promote-scoped-execution") => Action::ScopedExecutionPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "promote-api-currency") => Action::ApiCurrencyPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "promote-api-fee-resolution") => Action::ApiFeeResolutionPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "review-currency") => Action::CurrencyReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "promote-currency") => Action::CurrencyPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "promote-reader") => Action::ReaderPromote(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("data-format", "review") => Action::DataFormatReview(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
        ),
        ("data-format", "migrate") => Action::DataFormatMigrate(
            o.take("--from")?.into(),
            o.take("--manifest-digest")?,
            o.take("--hold-digest")?,
            o.take("--review-digest")?,
        ),
        ("upgrade", "review") => Action::UpgradeReview,
        ("upgrade", "prepare") => Action::UpgradePrepare(o.take("--review-digest")?),
        ("upgrade", "status") => Action::UpgradeStatus,
        ("upgrade", "backup") => {
            Action::UpgradeBackup(o.take("--to")?.into(), o.take("--hold-digest")?)
        }
        ("upgrade", "release") => Action::UpgradeRelease(o.take("--hold-digest")?),
        ("backup", "create") => Action::BackupCreate(o.take("--to")?.into()),
        ("backup", "verify") => Action::BackupVerify(o.take("--from")?.into()),
        ("backup", "restore") => {
            Action::BackupRestore(o.take("--from")?.into(), o.take("--manifest-digest")?)
        }
        ("service", "start") => Action::Start,
        ("service", "stop") => Action::Stop,
        ("service", "review-preserved-stop") => Action::PreservedShutdownReview,
        ("service", "stop-preserving") => Action::PreservedShutdown(o.take("--review-digest")?),
        ("credentials", "set") => Action::SetKey(provider(&o.take("--provider")?)?),
        _ => Action::Request(Box::new(match (family, verb) {
            ("status", "") => Command::Snapshot,
            ("providers", "list") => {
                view = View::Providers;
                Command::Snapshot
            }
            ("providers", "check") => {
                if let Some(name) = o.optional("--provider") {
                    if name != "openai" {
                        return Err("其他供应商请用 --file 指定 ProviderEndpoint".into());
                    }
                    Command::CheckOpenAi
                } else {
                    Command::CheckProviderModels {
                        target: json_file::<ProviderEndpoint>(&o.take("--file")?)?,
                    }
                }
            }
            ("credentials", "remove") => Command::RemoveProviderKey {
                provider: provider(&o.take("--provider")?)?,
            },
            ("project", "add") => Command::RegisterProject {
                project: Project {
                    id: o.take("--id")?,
                    root: o.take("--root")?,
                    timezone: o
                        .optional("--timezone")
                        .unwrap_or_else(|| cerulean_domain::timezone::DEFAULT.into()),
                },
            },
            ("project", "show") | ("setup", "show") | ("plan", "show") => {
                view = View::Project(o.take("--project")?);
                Command::Snapshot
            }
            ("validator", "add") => {
                let v: Validator = json_file(&o.take("--file")?)?;
                Command::RegisterValidator {
                    id: v.id,
                    name: v.name,
                    executable: v.executable,
                    inputs: v.inputs,
                    environment: v.environment,
                }
            }
            ("setup", "save") => Command::ConfigureProject {
                draft: Box::new(json_file::<ProjectSetupDraft>(&o.take("--file")?)?),
            },
            ("plan", "request") => Command::RequestChiefPlan {
                project_id: o.take("--project")?,
                setup_digest: o.take("--setup-digest")?,
            },
            ("history", "review") => Command::ReviewHistoricalCharge {
                input: json_file(&o.take("--file")?)?,
            },
            ("api-generation", "list") => Command::ReadApiGenerations {
                offset: o
                    .optional("--offset")
                    .map(|v| v.parse::<u32>())
                    .transpose()
                    .map_err(|_| "API生成偏移无效")?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
            ("api-generation", "review") => Command::ReviewApiGenerationResolution {
                input: json_file(&o.take("--file")?)?,
            },
            ("api-generation", "confirm") => Command::ConfirmApiGenerationResolution {
                input: json_file(&o.take("--file")?)?,
                review_digest: o.take("--review-digest")?,
            },
            ("api-generation", "show") => Command::ReadApiGenerationConfirmation {
                project_id: o.take("--project")?,
                request_id: o.take("--request")?,
                review_digest: o.take("--review-digest")?,
            },
            ("api-count", "list") => Command::ReadApiCounts {
                offset: o
                    .optional("--offset")
                    .map(|v| v.parse::<u32>())
                    .transpose()
                    .map_err(|_| "API计数偏移无效")?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
            ("api-count", "review") => Command::ReviewApiCountResolution {
                input: json_file(&o.take("--file")?)?,
            },
            ("api-count", "confirm") => Command::ConfirmApiCountResolution {
                input: json_file(&o.take("--file")?)?,
                review_digest: o.take("--review-digest")?,
            },
            ("api-count", "show") => Command::ReadApiCountConfirmation {
                project_id: o.take("--project")?,
                request_id: o.take("--request")?,
                review_digest: o.take("--review-digest")?,
            },
            ("history", "sources" | "accounted") => {
                let offset = o
                    .optional("--offset")
                    .map(|s| s.parse::<u32>())
                    .transpose()
                    .map_err(|_| "历史页偏移必须是非负整数")?
                    .unwrap_or(0);
                let revision = o.optional("--revision");
                if verb == "sources" {
                    Command::ReadHistoricalSources { offset, revision }
                } else {
                    Command::ReadHistoricalAccounting { offset, revision }
                }
            }
            ("history", "show") => Command::ReadHistoricalConfirmation {
                project_id: o.take("--project")?,
                scope: json_file(&o.take("--scope-file")?)?,
                review_digest: o.take("--review-digest")?,
            },
            ("history", "confirm") => Command::ConfirmHistoricalCharge {
                input: json_file(&o.take("--file")?)?,
                review_digest: o.take("--review-digest")?,
            },
            ("plan", "authorize-budget") => Command::AuthorizePlanningBudget {
                project_id: o.take("--project")?,
                setup_digest: o.take("--setup-digest")?,
            },
            ("plan", "approve") => Command::ApproveRevision {
                project_id: o.take("--project")?,
                digest: o.take("--digest")?,
            },
            ("run", "start" | "resume") => Command::StartProjectRun {
                project_id: o.take("--project")?,
                revision_digest: o.take("--revision-digest")?,
                setup_digest: o.take("--setup-digest")?,
            },
            ("run", "pause") => Command::PauseProjectRun {
                run_id: o.take("--run")?,
            },
            ("task", "recover" | "cancel") => {
                let adapter = o.take("--adapter")?;
                let project_id = o.take("--project")?;
                let attempt_id = o.take("--attempt")?;
                if verb == "recover" {
                    match adapter.as_str() {
                        "codex" => Command::ReconcileCodex {
                            project_id,
                            attempt_id,
                        },
                        "omp" => Command::RecoverOmp {
                            project_id,
                            attempt_id,
                        },
                        "api" => Command::RecoverApi {
                            project_id,
                            attempt_id,
                        },
                        "desktop" => Command::ReadDesktopTask {
                            project_id,
                            attempt_id,
                            reader_sha256: o.optional("--reader-sha256"),
                        },
                        _ => return Err("不支持的执行器；使用 codex/omp/api/desktop".into()),
                    }
                } else {
                    let reason = o.take("--reason")?;
                    match adapter.as_str() {
                        "codex" => Command::CancelCodex {
                            project_id,
                            attempt_id,
                            reason,
                            evidence_digest: o.take("--evidence-digest")?,
                        },
                        "omp" => Command::CancelOmp {
                            project_id,
                            attempt_id,
                            reason,
                        },
                        "api" => Command::CancelApi {
                            project_id,
                            attempt_id,
                            reason,
                        },
                        "desktop" => Command::CancelDesktopTask {
                            project_id,
                            attempt_id,
                            reason,
                            review_digest: o.take("--evidence-digest")?,
                        },
                        _ => return Err("不支持的执行器；使用 codex/omp/api/desktop".into()),
                    }
                }
            }
            ("management", "list") => Command::ReadManagementPage {
                project_id: o.optional("--project"),
                offset: o
                    .optional("--offset")
                    .map(|n| n.parse::<u32>().map_err(|_| "offset 必须为非负整数"))
                    .transpose()?
                    .unwrap_or(0),
                revision: o.optional("--revision"),
            },
            ("management", "show") => Command::ReadManagementDetail {
                operation_id: o.take("--operation")?,
            },
            ("management", "answer") => Command::AnswerManagementWait {
                operation_id: o.take("--operation")?,
                evidence_digest: o.take("--evidence-digest")?,
                answer: o.take("--answer")?,
            },
            ("management", "discard") => Command::DiscardManagementDecision {
                operation_id: o.take("--operation")?,
                evidence_digest: o.take("--evidence-digest")?,
                reason: o.take("--reason")?,
            },
            ("verification", "show") => Command::ReadVerificationDetail {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
            },
            ("verification-recheck", "review-disposition") => {
                view = View::VerificationRecheckDispositionReview;
                Command::ReviewVerificationRecheckDisposition {
                    project_id: o.take("--project")?,
                    operation_id: o.take("--operation")?,
                    reason: o.take("--reason")?,
                }
            }
            ("verification-recheck", "retire") => Command::RetireVerificationRecheck {
                review: Box::new(json_file(&o.take("--file")?)?),
                event_operation_id: o.take("--event-operation")?,
            },
            ("verification-recheck", "review") => {
                view = View::VerificationRecheckReview;
                Command::ReviewVerificationRecheck {
                    input: Box::new(json_file(&o.take("--file")?)?),
                }
            }
            ("verification-recheck", "approve") => Command::ApproveVerificationRecheck {
                review: Box::new(json_file(&o.take("--file")?)?),
                approval_operation_id: o.take("--approval-operation")?,
                operation_id: o.take("--operation")?,
            },
            ("verification-recheck", "show") => {
                view = View::VerificationRecheckStatus;
                Command::ReadVerificationRecheck {
                    project_id: o.take("--project")?,
                    operation_id: o.take("--operation")?,
                }
            }
            ("verification-recheck", "start") => Command::StartVerificationRecheck {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
            },
            ("verification-recheck", "cancel") => Command::CancelVerificationRecheck {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
            },
            ("verification-recheck", "recover") => Command::RecoverVerificationRecheck {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
            },
            ("verification", "recover") => Command::RecoverVerification {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
                evidence_digest: o.take("--evidence-digest")?,
            },
            ("verification", "cancel") => Command::CancelVerification {
                project_id: o.take("--project")?,
                operation_id: o.take("--operation")?,
            },
            _ => return Err("未知命令；请使用 --help".into()),
        })),
    };
    o.done()?;
    Ok(Invocation {
        directory,
        action,
        view,
    })
}
fn project_view(snapshot: &Snapshot, id: &str) -> Result<Value, String> {
    let workflow = snapshot
        .workflows
        .iter()
        .find(|w| w.project.id == id)
        .ok_or("项目不存在")?;
    Ok(
        json!({"workflow":workflow,"setup":snapshot.project_setups.iter().find(|s|s.setup.project_id==id),
        "runs":snapshot.project_runs.iter().filter(|r|r.project_id==id).collect::<Vec<_>>(),
        "codexJobs":snapshot.codex_jobs.iter().filter(|j|j.project_id==id).collect::<Vec<_>>(),
        "ompJobs":snapshot.omp_jobs.iter().filter(|j|j.project_id==id).collect::<Vec<_>>(),
        "apiJobs":snapshot.api_jobs.iter().filter(|j|j.project_id==id).collect::<Vec<_>>(),
        "verificationJobs":snapshot.verification_jobs.iter().filter(|j|j.project_id==id).collect::<Vec<_>>() }),
    )
}
fn error(code: u8, kind: &str, message: &str) -> u8 {
    eprintln!(
        "{}",
        json!({"ok":false,"error":{"code":kind,"message":message}})
    );
    code
}
fn key_from_input(input: &mut impl Read) -> Result<SecretInput, String> {
    let mut bytes = zeroize::Zeroizing::new(Vec::new());
    input
        .take(515)
        .read_to_end(&mut bytes)
        .map_err(|_| "无法读取密钥标准输入")?;
    if bytes.len() > 514 {
        return Err("密钥输入过长".into());
    }
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| "密钥必须是有效 ASCII")?;
    SecretInput::new(text.to_owned())
}
pub(super) fn run(args: &[String]) -> u8 {
    // Windows gives the main thread a 1 MiB stack. The bounded full-snapshot
    // decoder plus retained-process async wait exceeds that in debug builds.
    // Use a fixed stack on an owned, joined thread; never detach or retry calls.
    let args = args.to_vec();
    let thread = match std::thread::Builder::new()
        .name("workbench-cli".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || run_client(&args))
    {
        Ok(thread) => thread,
        Err(_) => return error(4, "runtimeUnavailable", "无法启动本机 CLI 线程；未发送请求"),
    };
    thread.join().unwrap_or_else(|_| {
        error(
            4,
            "resultUnknown",
            "CLI 线程异常；先查询原操作记录，不自动重发",
        )
    })
}
fn run_client(args: &[String]) -> u8 {
    if args == ["--help"] || args.is_empty() {
        println!("{HELP}");
        return 0;
    }
    let invocation = match parse(args) {
        Ok(i) => i,
        Err(e) => return error(2, "invalidInput", &e),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(_) => return error(4, "runtimeUnavailable", "无法启动本机 IPC 客户端"),
    };
    // Full snapshots make this future large in debug builds. Keep it off the
    // Windows main stack, including the nested retained-process stop wait.
    runtime.block_on(Box::pin(execute(invocation)))
}
async fn execute(i: Invocation) -> u8 {
    let maintenance = match &i.action {
        Action::MaintenanceStatus(expected) => Some(
            cerulean_core::maintenance_interlock::status(&i.directory, expected.as_deref())
                .map(|v| json!(v)),
        ),
        Action::MaintenanceReserve(transaction, intent, hold) => Some(
            cerulean_core::maintenance_interlock::reserve(&i.directory, transaction, intent, hold)
                .map(|v| json!(v)),
        ),
        Action::MaintenanceCheck(expected) => Some(
            cerulean_core::maintenance_interlock::check(&i.directory, expected).map(|v| json!(v)),
        ),
        Action::MaintenanceRelease(expected, plan) => Some(
            cerulean_core::maintenance_interlock::release(&i.directory, expected, plan.as_deref())
                .map(|v| json!(v)),
        ),
        Action::RecoveryReview(from, manifest, hold) => Some(
            cerulean_core::backup_recovery::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::RecoveryPreviousReview(from, manifest, hold, previous) => Some(
            cerulean_core::backup_recovery::review_previous(
                &i.directory,
                from,
                manifest,
                hold,
                previous,
            )
            .map(|v| json!(v)),
        ),
        Action::RecoveryPreviousPrepare(from, manifest, hold, previous, review) => Some(
            cerulean_core::backup_recovery::prepare_previous(
                &i.directory,
                from,
                manifest,
                hold,
                previous,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::RecoveryPrepare(from, manifest, hold, review) => Some(
            cerulean_core::backup_recovery::prepare(&i.directory, from, manifest, hold, review)
                .map(|v| json!(v)),
        ),
        Action::RecoveryStatus => {
            Some(cerulean_core::backup_recovery::status(&i.directory).map(|v| json!(v)))
        }
        Action::RecoveryDiagnose => {
            Some(cerulean_core::backup_recovery::diagnose(&i.directory).map(|v| json!(v)))
        }
        Action::RecoveryInspect(review) => {
            Some(cerulean_core::backup_recovery::inspect(&i.directory, review).map(|v| json!(v)))
        }
        Action::RecoveryApply(review) => {
            Some(cerulean_core::backup_recovery::apply(&i.directory, review).map(|v| json!(v)))
        }
        Action::RecoveryFinish(review, abandon) => Some(
            cerulean_core::backup_recovery::finish(&i.directory, review, *abandon)
                .map(|v| json!(v)),
        ),
        Action::DataFormatStatus => {
            Some(cerulean_core::data_format::status(&i.directory).map(|v| json!(v)))
        }
        Action::ReaderReview(from, manifest, hold) => Some(
            cerulean_core::reader_generation::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::CurrencyReview(from, manifest, hold) => Some(
            cerulean_core::currency_guard::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::PreparationReview(from, manifest, hold) => Some(
            cerulean_core::currency_execution_guard::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::PreparationPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_execution_guard::promote(
                &i.directory,
                from,
                manifest,
                hold,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::CountingReview(from, manifest, hold) => Some(
            cerulean_core::currency_count_guard::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::CountingPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_count_guard::promote(
                &i.directory,
                from,
                manifest,
                hold,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::CountResolutionReview(from, manifest, hold) => Some(
            cerulean_core::currency_count_resolution_guard::review(
                &i.directory,
                from,
                manifest,
                hold,
            )
            .map(|v| json!(v)),
        ),
        Action::CountResolutionPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_count_resolution_guard::promote(
                &i.directory,
                from,
                manifest,
                hold,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::ScopedPricingReview(from, manifest, hold) => Some(
            cerulean_core::currency_scoped_guard::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::ScopedPricingPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_scoped_guard::promote(
                &i.directory,
                from,
                manifest,
                hold,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::ScopedCountReview(from, manifest, hold) => Some(
            cerulean_core::currency_scoped_count_guard::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::ScopedExecutionReview(from, manifest, hold) => Some(
            cerulean_core::currency_scoped_execution_guard::review(
                &i.directory,
                from,
                manifest,
                hold,
            )
            .map(|v| json!(v)),
        ),
        Action::ApiCurrencyReview(from, manifest, hold) => Some(
            cerulean_core::currency_api_guard::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::ApiFeeResolutionReview(from, manifest, hold) => Some(
            cerulean_core::currency_api_resolution_guard::review(
                &i.directory,
                from,
                manifest,
                hold,
            )
            .map(|v| json!(v)),
        ),
        Action::VerificationRecheckReview(from, manifest, hold) => Some(
            match hold {
                Some(hold) => cerulean_core::verification_recheck_guard::review_held(
                    &i.directory,
                    from,
                    manifest,
                    hold,
                ),
                None => {
                    cerulean_core::verification_recheck_guard::review(&i.directory, from, manifest)
                }
            }
            .map(|v| json!(v)),
        ),
        Action::VerificationRecheckPromote(from, manifest, review, hold) => Some(
            match hold {
                Some(hold) => cerulean_core::verification_recheck_guard::promote_held(
                    &i.directory,
                    from,
                    manifest,
                    hold,
                    review,
                ),
                None => cerulean_core::verification_recheck_guard::promote(
                    &i.directory,
                    from,
                    manifest,
                    review,
                ),
            }
            .map(|v| json!(v)),
        ),
        Action::RejectedUsageReview(from, manifest, hold) => Some(
            match hold {
                Some(hold) => cerulean_core::rejected_usage_guard::review_held(
                    &i.directory,
                    from,
                    manifest,
                    hold,
                ),
                None => cerulean_core::rejected_usage_guard::review(&i.directory, from, manifest),
            }
            .map(|v| json!(v)),
        ),
        Action::RejectedUsagePromote(from, manifest, review, hold) => Some(
            match hold {
                Some(hold) => cerulean_core::rejected_usage_guard::promote_held(
                    &i.directory,
                    from,
                    manifest,
                    hold,
                    review,
                ),
                None => cerulean_core::rejected_usage_guard::promote(
                    &i.directory,
                    from,
                    manifest,
                    review,
                ),
            }
            .map(|v| json!(v)),
        ),
        Action::RelayReview(from, manifest) => {
            Some(cerulean_core::relay_guard::review(&i.directory, from, manifest).map(|v| json!(v)))
        }
        Action::RelayPromote(from, manifest, review) => Some(
            cerulean_core::relay_guard::promote(&i.directory, from, manifest, review)
                .map(|v| json!(v)),
        ),
        Action::ExperimentsReview(from, manifest) => Some(
            cerulean_core::experiment_guard::review(&i.directory, from, manifest).map(|v| json!(v)),
        ),
        Action::EvidenceArchiveCreate(index, expected, to) => Some(
            cerulean_core::portable_evidence::create(&i.directory, index, expected, to)
                .map(|v| json!(v)),
        ),
        Action::EvidenceArchiveVerify(from, expected) => {
            Some(cerulean_core::portable_evidence::verify(from, expected).map(|v| json!(v)))
        }
        Action::ImportRetentionReview(from, manifest, to, reason) => Some(
            cerulean_core::team_import_retention::review(&i.directory, from, manifest, to, reason)
                .map(|v| json!(v)),
        ),
        Action::ImportRetain(from, manifest, to, reason, review) => Some(
            cerulean_core::team_import_retention::retain(
                &i.directory,
                from,
                manifest,
                to,
                reason,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::ImportRetentionInspect(from, manifest, to, reason, review) => Some(
            cerulean_core::team_import_retention::inspect(
                &i.directory,
                from,
                manifest,
                to,
                reason,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::ExperimentsPromote(from, manifest, review) => Some(
            cerulean_core::experiment_guard::promote(&i.directory, from, manifest, review)
                .map(|v| json!(v)),
        ),
        Action::ManagementSourceReview(from, manifest, hold) => Some(
            cerulean_core::management_source_guard::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::ManagementSourcePromote(from, manifest, hold, review) => Some(
            cerulean_core::management_source_guard::promote(
                &i.directory,
                from,
                manifest,
                hold,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::ScopedCountPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_scoped_count_guard::promote(
                &i.directory,
                from,
                manifest,
                hold,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::ScopedExecutionPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_scoped_execution_guard::promote(
                &i.directory,
                from,
                manifest,
                hold,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::ApiCurrencyPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_api_guard::promote(&i.directory, from, manifest, hold, review)
                .map(|v| json!(v)),
        ),
        Action::ApiFeeResolutionPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_api_resolution_guard::promote(
                &i.directory,
                from,
                manifest,
                hold,
                review,
            )
            .map(|v| json!(v)),
        ),
        Action::CurrencyPromote(from, manifest, hold, review) => Some(
            cerulean_core::currency_guard::promote(&i.directory, from, manifest, hold, review)
                .map(|v| json!(v)),
        ),
        Action::ReaderPromote(from, manifest, hold, review) => Some(
            cerulean_core::reader_generation::promote(&i.directory, from, manifest, hold, review)
                .map(|v| json!(v)),
        ),
        Action::DataFormatReview(from, manifest, hold) => Some(
            cerulean_core::data_format::review(&i.directory, from, manifest, hold)
                .map(|v| json!(v)),
        ),
        Action::DataFormatMigrate(from, manifest, hold, review) => Some(
            cerulean_core::data_format::migrate(&i.directory, from, manifest, hold, review)
                .map(|v| json!(v)),
        ),
        Action::UpgradeStatus => {
            Some(cerulean_core::upgrade::status(&i.directory).map(|v| json!(v)))
        }
        Action::UpgradeRelease(digest) => {
            Some(cerulean_core::upgrade::release(&i.directory, digest).map(|v| json!(v)))
        }
        Action::UpgradeBackup(to, digest) => {
            Some(cerulean_core::backup::create_held(&i.directory, to, digest).map(|v| json!(v)))
        }
        Action::BackupCreate(to) => {
            Some(cerulean_core::backup::create(&i.directory, to).map(|v| json!(v)))
        }
        Action::BackupVerify(from) => Some(cerulean_core::backup::verify(from).map(|v| json!(v))),
        Action::BackupRestore(from, digest) => {
            Some(cerulean_core::backup::restore(from, &i.directory, digest).map(|v| json!(v)))
        }
        _ => None,
    };
    if let Some(result) = maintenance {
        return match result {
            Ok(review) => {
                use std::io::Write;
                let mut output = std::io::stdout().lock();
                if serde_json::to_writer(&mut output, &json!({"ok":true,"result":review})).is_err()
                    || output
                        .write_all(b"\n")
                        .and_then(|_| output.flush())
                        .is_err()
                {
                    error(
                        5,
                        "outputFailed",
                        "维护操作已完成但输出失败；先检查目标目录，勿自动重试",
                    )
                } else {
                    0
                }
            }
            Err(e) => error(
                3,
                if matches!(
                    i.action,
                    Action::EvidenceArchiveCreate(..) | Action::EvidenceArchiveVerify(..)
                ) {
                    "archiveRejected"
                } else {
                    "backupRejected"
                },
                &e,
            ),
        };
    }
    let id = format!(
        "cli-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_micros()
    );
    let start = matches!(i.action, Action::Start);
    let wait_stop = matches!(
        i.action,
        Action::UpgradePrepare(_) | Action::PreservedShutdown(_)
    );
    let operation = match i.action {
        Action::ExternalChief(operation) => UserOperation::ExternalChief {
            id: id.clone(),
            protocol_version: PROTOCOL_VERSION,
            operation,
        },
        Action::Workspace(query) => UserOperation::Workspace {
            id: id.clone(),
            protocol_version: PROTOCOL_VERSION,
            query,
        },
        Action::PreservedShutdownReview => UserOperation::ReviewPreservedShutdown {
            id: id.clone(),
            protocol_version: PROTOCOL_VERSION,
        },
        Action::PreservedShutdown(review_digest) => UserOperation::PreserveShutdown {
            id: id.clone(),
            protocol_version: PROTOCOL_VERSION,
            review_digest,
        },
        Action::UpgradeReview => UserOperation::ReviewUpgrade {
            id: id.clone(),
            protocol_version: PROTOCOL_VERSION,
        },
        Action::UpgradePrepare(review_digest) => UserOperation::PrepareUpgrade {
            id: id.clone(),
            protocol_version: PROTOCOL_VERSION,
            review_digest,
        },
        Action::Stop => UserOperation::Shutdown {
            id: id.clone(),
            protocol_version: PROTOCOL_VERSION,
        },
        action => {
            let command = match action {
                Action::Request(command) => *command,
                Action::Start => Command::Snapshot,
                Action::SetKey(provider) => {
                    let key = match key_from_input(&mut std::io::stdin().lock()) {
                        Ok(k) => k,
                        Err(e) => return error(2, "invalidKey", &e),
                    };
                    Command::SetProviderKey { provider, key }
                }
                Action::Stop
                | Action::EvidenceArchiveCreate(_, _, _)
                | Action::EvidenceArchiveVerify(_, _)
                | Action::ExternalChief(_)
                | Action::ManagementSourceReview(_, _, _)
                | Action::ManagementSourcePromote(_, _, _, _)
                | Action::RelayReview(_, _)
                | Action::RelayPromote(_, _, _)
                | Action::RejectedUsageReview(_, _, _)
                | Action::RejectedUsagePromote(_, _, _, _)
                | Action::VerificationRecheckReview(_, _, _)
                | Action::VerificationRecheckPromote(_, _, _, _)
                | Action::ExperimentsReview(_, _)
                | Action::ExperimentsPromote(_, _, _)
                | Action::ImportRetentionReview(_, _, _, _)
                | Action::ImportRetain(_, _, _, _, _)
                | Action::ImportRetentionInspect(_, _, _, _, _)
                | Action::Workspace(_)
                | Action::MaintenanceStatus(_)
                | Action::MaintenanceReserve(_, _, _)
                | Action::MaintenanceCheck(_)
                | Action::MaintenanceRelease(_, _)
                | Action::RecoveryReview(_, _, _)
                | Action::RecoveryPreviousReview(_, _, _, _)
                | Action::RecoveryPreviousPrepare(_, _, _, _, _)
                | Action::RecoveryPrepare(_, _, _, _)
                | Action::RecoveryStatus
                | Action::RecoveryDiagnose
                | Action::RecoveryInspect(_)
                | Action::RecoveryApply(_)
                | Action::RecoveryFinish(_, _)
                | Action::DataFormatStatus
                | Action::ReaderReview(_, _, _)
                | Action::ReaderPromote(_, _, _, _)
                | Action::CurrencyReview(_, _, _)
                | Action::CurrencyPromote(_, _, _, _)
                | Action::PreparationReview(_, _, _)
                | Action::PreparationPromote(_, _, _, _)
                | Action::CountingReview(_, _, _)
                | Action::CountingPromote(_, _, _, _)
                | Action::CountResolutionReview(_, _, _)
                | Action::CountResolutionPromote(_, _, _, _)
                | Action::ScopedPricingReview(_, _, _)
                | Action::ScopedCountReview(_, _, _)
                | Action::ScopedExecutionReview(_, _, _)
                | Action::ApiCurrencyReview(_, _, _)
                | Action::ApiFeeResolutionReview(_, _, _)
                | Action::ScopedPricingPromote(_, _, _, _)
                | Action::ScopedCountPromote(_, _, _, _)
                | Action::ScopedExecutionPromote(_, _, _, _)
                | Action::ApiCurrencyPromote(_, _, _, _)
                | Action::ApiFeeResolutionPromote(_, _, _, _)
                | Action::DataFormatReview(_, _, _)
                | Action::DataFormatMigrate(_, _, _, _)
                | Action::PreservedShutdownReview
                | Action::PreservedShutdown(_)
                | Action::UpgradeReview
                | Action::UpgradePrepare(_)
                | Action::UpgradeStatus
                | Action::UpgradeBackup(_, _)
                | Action::UpgradeRelease(_) => unreachable!(),
                Action::BackupCreate(_) | Action::BackupVerify(_) | Action::BackupRestore(_, _) => {
                    unreachable!()
                }
            };
            let historical = command.is_historical();
            let request = Request {
                id: id.clone(),
                protocol_version: PROTOCOL_VERSION,
                command,
            };
            if historical {
                UserOperation::Historical { request }
            } else if request.command.is_count_resolution() {
                UserOperation::CountResolution { request }
            } else if request.command.supports_receipt() {
                UserOperation::Command { request }
            } else {
                UserOperation::Request { request }
            }
        }
    };
    let wire = match serde_json::to_vec(&operation) {
        Ok(b) => zeroize::Zeroizing::new(b),
        Err(_) => return error(2, "invalidRequest", "请求无法编码"),
    };
    if wire.len() > 65536 {
        return error(2, "requestTooLarge", "请求超过 IPC 上限，未发送");
    }
    drop(wire);
    let connection = if start {
        let executable = match std::env::current_exe() {
            Ok(p) => p,
            Err(_) => return error(4, "runtimeUnavailable", "无法定位当前核心程序"),
        };
        Client::connect_or_start(&i.directory, &executable).await
    } else {
        Client::connect(&i.directory).await
    };
    let client = match connection {
        Ok(c) => c,
        Err(details) => {
            let connection = serde_json::from_str::<Value>(&details).unwrap_or_else(
                |_| json!({"stage":"connection","win32Error":null,"message":details}),
            );
            eprintln!(
                "{}",
                json!({"ok":false,"error":{
                    "code":"connectionFailed",
                    "message":"未发送请求：无法连接此目录的核心。请检查目录、版本与权限，或执行 service start",
                    "connection":connection
                }})
            );
            return 4;
        }
    };
    let server_pid = client.server_pid;
    let reply = match if wait_stop {
        Box::pin(client.call_and_wait_stop(operation)).await
    } else {
        Box::pin(client.call(operation)).await
    } {
        Ok(r) => r,
        Err(reason) => {
            return error(
                4,
                "resultUnknown",
                if reason == "service request timed out; not resent" {
                    "等待核心响应超时，结果可能已入账；未重发。请先查询 status 和原任务记录"
                } else {
                    "通信中断，结果可能已入账；未重发。请先查询 status 和原任务记录"
                },
            )
        }
    };
    if let Some(e) = reply.response.error {
        return error(3, &e.code, &e.message);
    }
    let historical_result = reply.historical.is_some();
    let count_resolution_result = reply.count_resolution.is_some();
    let workspace_result = reply.workspace.is_some();
    let external_chief_result = reply.external_chief.is_some();
    let result = if let Some(external) = reply.external_chief {
        json!(external)
    } else if let Some(workspace) = reply.workspace {
        json!(workspace)
    } else if let Some(historical) = reply.historical {
        json!(historical)
    } else if let Some(count) = reply.count_resolution {
        json!(count)
    } else {
        match (&i.view, reply.response.result) {
            (View::Full, snapshot) => json!(snapshot),
            (View::Providers, Some(s)) => {
                json!({"adapters":s.adapters,"credentials":s.provider_credentials,"connections":s.provider_connections,"openai":s.openai_connection})
            }
            (View::Project(project), Some(s)) => match project_view(&s, project) {
                Ok(v) => v,
                Err(e) => return error(3, "projectNotFound", &e),
            },
            (View::VerificationRecheckDispositionReview, Some(s)) => {
                match s.verification_recheck_disposition_review {
                    Some(review) => json!(review),
                    None => return error(4, "invalidResponse", "核心未返回原处置审阅；未重发"),
                }
            }
            (View::VerificationRecheckReview, Some(s)) => match s.verification_recheck_review {
                Some(review) => json!(review),
                None => return error(4, "invalidResponse", "核心未返回复验审阅；未重发"),
            },
            (View::VerificationRecheckStatus, Some(s)) => match s.verification_recheck_status {
                Some(status) => json!(status),
                None => return error(4, "invalidResponse", "核心未返回原复验状态；未重发"),
            },
            _ => return error(4, "invalidResponse", "核心未返回快照；未重发"),
        }
    };
    let value = json!({"ok":true,"requestId":id,"serverPid":server_pid,"stopping":reply.stopping,"result":result,"commandReceipt":reply.command_receipt,"externalChiefResult":external_chief_result,"workspaceResult":workspace_result,"historicalResult":historical_result,"countResolutionResult":count_resolution_result,"preservationReview":reply.preservation_review,"upgradeReview":reply.upgrade_review,"upgradeHold":reply.upgrade_hold});
    use std::io::Write;
    let mut output = std::io::stdout().lock();
    if serde_json::to_writer(&mut output, &value).is_err()
        || output
            .write_all(b"\n")
            .and_then(|_| output.flush())
            .is_err()
    {
        return error(
            5,
            "outputFailed",
            "请求已响应，但输出失败；请查询状态，勿据此自动重发",
        );
    }
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn verification_recheck_cli_keeps_review_approval_and_launch_separate() {
        use cerulean_domain::verification_recheck::{RecheckRequest, RecheckReview};
        let temp = tempfile::tempdir().unwrap();
        let input = temp.path().join("request.json");
        let review_path = temp.path().join("review.json");
        let request = RecheckRequest {
            project_id: "project".into(),
            original_operation_id: "failed".into(),
            replacement_checks: vec![],
            reason: "CLI parsing fixture; not execution authority".into(),
        };
        std::fs::write(&input, serde_json::to_vec(&request).unwrap()).unwrap();
        let review = RecheckReview {
            schema_version: 1,
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            original_operation_id: "failed".into(),
            revision_digest: "r".repeat(64),
            manifest_digest: "m".repeat(64),
            original_report_digest: "o".repeat(64),
            original_checks_digest: "c".repeat(64),
            original_source_snapshot: cerulean_domain::execution::SourceSnapshot {
                operation_id: "failed".into(),
                manifest_sha256: "s".repeat(64),
            },
            ledger_head: "h".repeat(64),
            replacement_checks: vec![],
            reason: request.reason.clone(),
        };
        std::fs::write(&review_path, serde_json::to_vec(&review).unwrap()).unwrap();
        let make = |verb: &str, path: &std::path::Path, extra: &[&str]| {
            let mut args: Vec<String> = [
                "--data-dir",
                "fixture",
                "verification-recheck",
                verb,
                "--file",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect();
            args.push(path.to_string_lossy().into());
            args.extend(extra.iter().map(|s| (*s).to_owned()));
            parse(&args)
        };
        let parsed = make("review", &input, &[]).unwrap();
        assert!(matches!(parsed.view, View::VerificationRecheckReview));
        assert!(
            matches!(parsed.action, Action::Request(c) if matches!(c.as_ref(), Command::ReviewVerificationRecheck { input } if input.as_ref() == &request))
        );
        let parsed = make(
            "approve",
            &review_path,
            &[
                "--approval-operation",
                "approve",
                "--operation",
                "new-check",
            ],
        )
        .unwrap();
        let Action::Request(command) = parsed.action else {
            panic!("user command expected")
        };
        assert!(command.supports_receipt());
        assert!(
            matches!(*command, Command::ApproveVerificationRecheck { review: got, approval_operation_id, operation_id }
            if *got == review && approval_operation_id == "approve" && operation_id == "new-check")
        );
        assert!(make("approve", &review_path, &["--operation", "new-check"]).is_err());
        assert!(make(
            "approve",
            &review_path,
            &["--approval-operation", "approve"]
        )
        .is_err());
        assert!(make(
            "start",
            &input,
            &["--project", "project", "--operation", "new-check"]
        )
        .is_err());
        assert!(make("review", &input, &["--run", "true"]).is_err());
        assert!(make("review", &review_path, &[]).is_err());
        assert!(make(
            "approve",
            &input,
            &[
                "--approval-operation",
                "approve",
                "--operation",
                "new-check"
            ]
        )
        .is_err());
    }

    #[test]
    fn verification_recheck_cli_queries_and_actions_require_exact_original_identity() {
        let reviewed = invocation("verification-recheck review-disposition --project p --operation original --reason retained").unwrap();
        assert!(matches!(
            reviewed.view,
            View::VerificationRecheckDispositionReview
        ));
        let Action::Request(c) = reviewed.action else {
            panic!("request");
        };
        assert!(!c.supports_receipt());
        for line in [
            "verification-recheck review-disposition --project p --operation original",
            "verification-recheck review-disposition --project p --operation original --reason r --force true",
            "verification-recheck retire --event-operation dispose",
        ] { assert!(invocation(line).is_err()); }
        for (verb, wire_type) in [
            ("show", "readVerificationRecheck"),
            ("start", "startVerificationRecheck"),
            ("cancel", "cancelVerificationRecheck"),
            ("recover", "recoverVerificationRecheck"),
        ] {
            let parsed = invocation(&format!(
                "verification-recheck {verb} --project p --operation original"
            ))
            .unwrap();
            if verb == "show" {
                assert!(matches!(parsed.view, View::VerificationRecheckStatus));
            }
            let Action::Request(command) = parsed.action else {
                panic!("user command expected")
            };
            assert_eq!(command.supports_receipt(), verb != "show");
            assert_eq!(
                serde_json::to_value(command).unwrap(),
                json!({"type":wire_type, "projectId":"p", "operationId":"original"})
            );
            for suffix in [
                "",
                " --project p",
                " --operation original",
                " --project p --operation original --retry true",
                " --project p --project p --operation original",
            ] {
                assert!(invocation(&format!("verification-recheck {verb}{suffix}")).is_err());
            }
        }
    }

    #[test]
    fn verification_recheck_protection_cli_keeps_cold_and_held_authority_distinct() {
        for hold in [None, Some("hold")] {
            let suffix = hold
                .map(|h| format!(" --hold-digest {h}"))
                .unwrap_or_default();
            let Action::VerificationRecheckReview(from, manifest, got_hold) = invocation(
                &format!("data-format review-verification-recheck --from cold --manifest-digest manifest{suffix}")
            ).unwrap().action else { panic!("expected read-only protection review"); };
            assert_eq!(from, PathBuf::from("cold"));
            assert_eq!(manifest, "manifest");
            assert_eq!(got_hold.as_deref(), hold);
            let Action::VerificationRecheckPromote(from, manifest, digest, got_hold) = invocation(
                &format!("data-format promote-verification-recheck --from cold --manifest-digest manifest --review-digest review{suffix}")
            ).unwrap().action else { panic!("expected explicit protection promotion"); };
            assert_eq!(from, PathBuf::from("cold"));
            assert_eq!(manifest, "manifest");
            assert_eq!(digest, "review");
            assert_eq!(got_hold.as_deref(), hold);
        }
        for line in [
            "data-format review-verification-recheck --from cold",
            "data-format review-verification-recheck --manifest-digest manifest",
            "data-format promote-verification-recheck --from cold --manifest-digest manifest",
            "data-format review-verification-recheck --from cold --manifest-digest manifest --review-digest unexpected",
            "data-format promote-verification-recheck --from cold --manifest-digest manifest --review-digest review --force",
        ] { assert!(invocation(line).is_err(),"{line}"); }
    }
    #[test]
    fn import_retention_cli_requires_original_selection_and_explicit_reason() {
        assert!(matches!(invocation("team-import review-retention --from backup --manifest-digest m --to receipt --reason preserve").unwrap().action,Action::ImportRetentionReview(_,_,_,_)));
        for action in ["retain", "inspect-retention"] {
            assert!(invocation(&format!("team-import {action} --from backup --manifest-digest m --to receipt --reason preserve")).is_err());
            assert!(invocation(&format!("team-import {action} --from backup --manifest-digest m --to receipt --review-digest r")).is_err());
            assert!(invocation(&format!("team-import {action} --from backup --manifest-digest m --to receipt --reason preserve --review-digest r")).is_ok());
        }
    }
    #[test]
    fn timezone_cli_defaults_and_explicit_iana_name_reach_same_host_command() {
        for (line, expected) in [
            ("project add --id p --root project", "Asia/Shanghai"),
            (
                "project add --id p --root project --timezone America/New_York",
                "America/New_York",
            ),
        ] {
            let Action::Request(command) = invocation(line).unwrap().action else {
                panic!("project registration command expected");
            };
            let Command::RegisterProject { project } = *command else {
                panic!("project registration command expected");
            };
            assert_eq!(project.timezone, expected);
        }
    }
    #[test]
    fn preserved_shutdown_cli_requires_original_review_digest() {
        assert!(matches!(
            invocation("service review-preserved-stop").unwrap().action,
            Action::PreservedShutdownReview
        ));
        assert!(
            matches!(invocation("service stop-preserving --review-digest original").unwrap().action, Action::PreservedShutdown(d) if d == "original")
        );
        for input in [
            "service stop-preserving",
            "service stop-preserving --force true",
            "service review-preserved-stop --approve true",
        ] {
            assert!(invocation(input).is_err());
        }
    }
    #[test]
    fn external_chief_cli_requires_bound_configuration_session_and_full_decision() {
        assert!(matches!(
            invocation("external-chief prepare --project p --setup-digest d --session current")
                .unwrap()
                .action,
            Action::ExternalChief(_)
        ));
        assert!(matches!(
            invocation("external-chief read --operation original")
                .unwrap()
                .action,
            Action::ExternalChief(_)
        ));
        for command in [
            "external-chief prepare --project p --setup-digest d",
            "external-chief read --operation original --approve true",
            "external-chief submit --operation original --session current",
            "external-chief resend --operation original",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
        assert!(matches!(invocation("data-format review-management-source --from archive --manifest-digest m --hold-digest h").unwrap().action,Action::ManagementSourceReview(_,_,_)));
        assert!(matches!(invocation("data-format promote-management-source --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action,Action::ManagementSourcePromote(_,_,_,_)));
    }
    #[test]
    fn workspace_pages_cli_parses_readonly_queries_and_requires_detail_revision() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace overview").unwrap().action,
            Action::Workspace(WorkspaceQuery::Overview)
        ));
        assert!(matches!(
            invocation("workspace plans --offset 20 --revision original")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::Plans {
                offset: 20,
                revision: Some(_)
            })
        ));
        assert!(matches!(
            invocation("workspace projects").unwrap().action,
            Action::Workspace(WorkspaceQuery::Projects {
                offset: 0,
                revision: None
            })
        ));
        assert!(matches!(
            invocation("workspace plan --id chosen --revision original")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::Plan { .. })
        ));
        for command in [
            "workspace plan --id chosen",
            "workspace overview --approve true",
            "workspace plans --offset -1",
            "workspace create",
            "workspace execution-review",
            "workspace jobs",
            "workspace jobs --project p --offset -1",
            "workspace job --project p --kind other --id a --revision r",
            "workspace job --project p --kind omp --id a",
            "workspace execution-review --project p --approve true",
            "workspace tasks",
            "workspace task --project p --task t",
            "workspace task-evidence --project p --task t --revision r --offset -1",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
        assert!(matches!(
            invocation("workspace execution-review --project p --digest original")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::ExecutionReview {
                digest: Some(_),
                ..
            })
        ));
        assert!(matches!(
            invocation("workspace jobs --project p --offset 7 --revision r")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::Jobs { offset: 7, .. })
        ));
        for kind in ["codex", "omp", "api", "verification"] {
            assert!(matches!(
                invocation(&format!(
                    "workspace job --project p --kind {kind} --id a --revision r"
                ))
                .unwrap()
                .action,
                Action::Workspace(WorkspaceQuery::Job { .. })
            ));
        }
        assert!(matches!(
            invocation("workspace tasks --project p --offset 3 --revision original")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::Tasks { offset: 3, .. })
        ));
        assert!(matches!(
            invocation("workspace task --project p --task t --revision original")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::Task { .. })
        ));
        assert!(matches!(
            invocation("workspace task-evidence --project p --task t --revision original")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::TaskEvidence { offset: 0, .. })
        ));
    }
    #[test]
    fn workspace_recovery_cli_parses_bounded_pages_and_original_detail_ids() {
        use cerulean_domain::workspace::{WorkspaceQuery, WorkspaceRecoveryKind};
        assert!(matches!(
            invocation("workspace recoveries --project p").unwrap().action,
            Action::Workspace(WorkspaceQuery::Recovery {
                project_id,
                offset: 0,
                revision: None,
            }) if project_id == "p"
        ));
        assert!(matches!(
            invocation("workspace recoveries --project p --offset 7 --revision original")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::Recovery {
                project_id,
                offset: 7,
                revision: Some(revision),
            }) if project_id == "p" && revision == "original"
        ));
        for (kind, expected, id) in [
            (
                "managementRecord",
                WorkspaceRecoveryKind::ManagementRecord,
                "management/record.json",
            ),
            (
                "startupClosure",
                WorkspaceRecoveryKind::StartupClosure,
                "startup/closure.json",
            ),
            (
                "startupReservation",
                WorkspaceRecoveryKind::StartupReservation,
                "omp:attempt-1",
            ),
            (
                "ompStop",
                WorkspaceRecoveryKind::OmpStop,
                "D:\\jobs\\pending.json",
            ),
        ] {
            assert!(matches!(
                invocation(&format!(
                    "workspace recovery --project p --kind {kind} --id {id} --revision original"
                ))
                .unwrap()
                .action,
                Action::Workspace(WorkspaceQuery::RecoveryDetail {
                    project_id,
                    kind,
                    id: parsed_id,
                    revision,
                }) if project_id == "p" && kind == expected && parsed_id == id && revision == "original"
            ));
        }
    }
    #[test]
    fn workspace_recovery_cli_requires_scope_and_revision_and_rejects_invalid_options() {
        for command in [
            "workspace recoveries",
            "workspace recoveries --project p --offset -1",
            "workspace recoveries --project p --offset nope",
            "workspace recoveries --project p --offset 4294967296",
            "workspace recoveries --project p --approve true",
            "workspace recovery --kind ompStop --id a --revision r",
            "workspace recovery --project p --id a --revision r",
            "workspace recovery --project p --kind other --id a --revision r",
            "workspace recovery --project p --kind api --id a --revision r",
            "workspace recovery --project p --kind ompStop --revision r",
            "workspace recovery --project p --kind ompStop --id a",
            "workspace recovery --project p --kind ompStop --id a --revision r --offset 0",
            "workspace recovery --project p --kind ompStop --id a --revision r --approve true",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
    }
    #[test]
    fn workspace_observation_cli_parses_summary_and_versioned_pages() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace worker-observation --project p --attempt a")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::WorkerObservation {
                project_id,
                attempt_id,
                revision: None,
            }) if project_id == "p" && attempt_id == "a"
        ));
        assert!(matches!(
            invocation("workspace worker-observation --project p --attempt a --revision original")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::WorkerObservation {
                project_id,
                attempt_id,
                revision: Some(revision),
            }) if project_id == "p" && attempt_id == "a" && revision == "original"
        ));
        for (verb, incidents) in [
            ("worker-observation-events", false),
            ("worker-observation-incidents", true),
        ] {
            for (options, expected_offset) in [
                ("", 0),
                (" --offset 7", 7),
                (" --offset 4294967295", u32::MAX),
            ] {
                let action = invocation(&format!(
                    "workspace {verb} --project p --attempt a --revision original{options}"
                ))
                .unwrap()
                .action;
                let (project_id, attempt_id, offset, revision) = match action {
                    Action::Workspace(WorkspaceQuery::WorkerObservationEvents {
                        project_id,
                        attempt_id,
                        offset,
                        revision,
                    }) if !incidents => (project_id, attempt_id, offset, revision),
                    Action::Workspace(WorkspaceQuery::WorkerObservationIncidents {
                        project_id,
                        attempt_id,
                        offset,
                        revision,
                    }) if incidents => (project_id, attempt_id, offset, revision),
                    _ => panic!("unexpected query for {verb}"),
                };
                assert_eq!(project_id, "p");
                assert_eq!(attempt_id, "a");
                assert_eq!(offset, expected_offset);
                assert_eq!(revision, "original");
            }
        }
    }
    #[test]
    fn workspace_observation_cli_requires_scope_and_page_revision_and_rejects_unknown_options() {
        for verb in [
            "worker-observation",
            "worker-observation-events",
            "worker-observation-incidents",
        ] {
            for options in [
                "--attempt a --revision r",
                "--project p --revision r",
                "--project p --attempt a --revision r --unknown true",
                "--project p --attempt a --revision r --approve true",
                "--project p --attempt a --revision r --principal core",
                "--project p --attempt a --revision r --revision other",
                "--project p --attempt a --revision",
            ] {
                let command = format!("workspace {verb} {options}");
                assert!(invocation(&command).is_err(), "{command}");
            }
        }
        assert!(
            invocation("workspace worker-observation --project p --attempt a --offset 0").is_err()
        );
        for verb in ["worker-observation-events", "worker-observation-incidents"] {
            let missing_revision = format!("workspace {verb} --project p --attempt a");
            assert_eq!(
                invocation(&missing_revision).err().unwrap(),
                "缺少必需选项 --revision"
            );
            for offset in ["-1", "nope", "4294967296"] {
                let command = format!(
                    "workspace {verb} --project p --attempt a --revision r --offset {offset}"
                );
                assert!(invocation(&command).is_err(), "{command}");
            }
        }
    }
    #[test]
    fn workspace_codex_startups_cli_parses_bounded_page_and_versioned_detail() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace codex-startups --project project-a").unwrap().action,
            Action::Workspace(WorkspaceQuery::CodexStartups {
                project_id,
                offset: 0,
                revision: None,
            }) if project_id == "project-a"
        ));
        assert!(matches!(
            invocation("workspace codex-startups --project project-a --offset 17 --revision revision-7")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::CodexStartups {
                project_id,
                offset: 17,
                revision: Some(revision),
            }) if project_id == "project-a" && revision == "revision-7"
        ));
        assert!(matches!(
            invocation("workspace codex-startup --project project-a --attempt attempt-9 --revision revision-7")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::CodexStartup {
                project_id,
                attempt_id,
                revision,
            }) if project_id == "project-a"
                && attempt_id == "attempt-9"
                && revision == "revision-7"
        ));
    }

    #[test]
    fn workspace_codex_startups_cli_rejects_missing_revision_invalid_offset_and_unknown_options() {
        for command in [
            "workspace codex-startups",
            "workspace codex-startups --project p --offset -1",
            "workspace codex-startups --project p --offset nope",
            "workspace codex-startups --project p --offset 4294967296",
            "workspace codex-startups --project p --unknown value",
            "workspace codex-startups --project p --approve true",
            "workspace codex-startup --project p --attempt a",
            "workspace codex-startup --project p --revision r",
            "workspace codex-startup --attempt a --revision r",
            "workspace codex-startup --project p --attempt a --revision r --offset 0",
            "workspace codex-startup --project p --attempt a --revision r --unknown value",
            "workspace codex-startup --project p --attempt a --revision r --revision other",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
        assert_eq!(
            invocation("workspace codex-startup --project p --attempt a")
                .err()
                .unwrap(),
            "缺少必需选项 --revision"
        );
    }

    #[test]
    fn workspace_codex_startups_cli_help_lists_readonly_queries() {
        assert!(HELP.contains("workspace codex-startups --project"));
        assert!(HELP.contains("workspace codex-startup --project"));
    }

    #[test]
    fn workspace_management_decisions_cli_parses_bounded_page_and_versioned_detail() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace management-decisions --project project-a").unwrap().action,
            Action::Workspace(WorkspaceQuery::ManagementDecisions {
                project_id,
                offset: 0,
                revision: None,
            }) if project_id == "project-a"
        ));
        assert!(matches!(
            invocation("workspace management-decisions --project project-a --offset 17 --revision revision-7")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::ManagementDecisions {
                project_id,
                offset: 17,
                revision: Some(revision),
            }) if project_id == "project-a" && revision == "revision-7"
        ));
        assert!(matches!(
            invocation("workspace management-decision --project project-a --operation operation-9 --revision revision-7")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::ManagementDecision {
                project_id,
                operation_id,
                revision,
            }) if project_id == "project-a"
                && operation_id == "operation-9"
                && revision == "revision-7"
        ));
    }

    #[test]
    fn workspace_management_decisions_cli_serializes_original_query_identity() {
        for (command, expected) in [
            (
                "workspace management-decisions --project project-a",
                json!({"type":"managementDecisions","projectId":"project-a","offset":0,"revision":null}),
            ),
            (
                "workspace management-decisions --project project-a --offset 17 --revision revision-7",
                json!({"type":"managementDecisions","projectId":"project-a","offset":17,"revision":"revision-7"}),
            ),
            (
                "workspace management-decision --project project-a --operation operation-9 --revision revision-7",
                json!({"type":"managementDecision","projectId":"project-a","operationId":"operation-9","revision":"revision-7"}),
            ),
        ] {
            let Action::Workspace(query) = invocation(command).unwrap().action else {
                panic!("expected workspace query: {command}");
            };
            assert_eq!(serde_json::to_value(query).unwrap(), expected, "{command}");
        }
    }

    #[test]
    fn workspace_management_decisions_cli_rejects_missing_revision_invalid_offset_and_unknown_options(
    ) {
        for command in [
            "workspace management-decisions",
            "workspace management-decisions --project p --offset -1",
            "workspace management-decisions --project p --offset nope",
            "workspace management-decisions --project p --offset 4294967296",
            "workspace management-decisions --project p --offset",
            "workspace management-decisions --project p --unknown value",
            "workspace management-decisions --project p --approve true",
            "workspace management-decisions --project p --operation op",
            "workspace management-decisions --project p --revision r --revision other",
            "workspace management-decision --project p --operation op",
            "workspace management-decision --project p --revision r",
            "workspace management-decision --operation op --revision r",
            "workspace management-decision --project p --operation op --revision",
            "workspace management-decision --project p --operation op --revision r --offset 0",
            "workspace management-decision --project p --operation op --revision r --unknown value",
            "workspace management-decision --project p --operation op --revision r --approve true",
            "workspace management-decision --project p --operation op --revision r --revision other",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
        assert_eq!(
            invocation("workspace management-decision --project p --operation op")
                .err()
                .unwrap(),
            "缺少必需选项 --revision"
        );
    }

    #[test]
    fn workspace_management_decisions_cli_help_lists_readonly_queries() {
        assert!(HELP.contains("workspace management-decisions --project"));
        assert!(HELP.contains("workspace management-decision --project"));
    }

    #[test]
    fn workspace_management_gateway_329_cli_preserves_original_identity_and_requires_revision() {
        let command = "workspace management-gateway-evidence --project project-a --operation operation-9 --revision revision-7";
        let Action::Workspace(query) = invocation(command).unwrap().action else {
            panic!("expected independent workspace query");
        };
        assert_eq!(
            serde_json::to_value(query).unwrap(),
            json!({"type":"managementGatewayEvidence","projectId":"project-a","operationId":"operation-9","revision":"revision-7"})
        );
        for command in [
            "workspace management-gateway-evidence --operation op --revision r",
            "workspace management-gateway-evidence --project p --revision r",
            "workspace management-gateway-evidence --project p --operation op",
            "workspace management-gateway-evidence --project p --operation op --revision r --revision other",
            "workspace management-gateway-evidence --project p --operation op --revision r --offset 0",
            "workspace management-gateway-evidence --project p --operation op --revision r --approve true",
            "workspace management-gateway-evidence --project p --operation op --revision r --unknown value",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
        assert!(HELP.contains("workspace management-gateway-evidence --project"));
        assert!(HELP.contains("旁路为本次读取观察"));
    }

    #[test]
    fn workspace_project_runs_cli_parses_bounded_page_and_versioned_detail() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace project-runs --project project-a").unwrap().action,
            Action::Workspace(WorkspaceQuery::ProjectRuns {
                project_id,
                offset: 0,
                revision: None,
            }) if project_id == "project-a"
        ));
        assert!(matches!(
            invocation("workspace project-runs --project project-a --offset 17 --revision revision-7")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::ProjectRuns {
                project_id,
                offset: 17,
                revision: Some(revision),
            }) if project_id == "project-a" && revision == "revision-7"
        ));
        assert!(matches!(
            invocation("workspace project-run --project project-a --run run-9 --revision revision-7")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::ProjectRun {
                project_id,
                run_id,
                revision,
            }) if project_id == "project-a"
                && run_id == "run-9"
                && revision == "revision-7"
        ));
        assert!(matches!(
            invocation("workspace project-runs --project project-a --offset 4294967295")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::ProjectRuns {
                offset: u32::MAX,
                ..
            })
        ));
    }

    #[test]
    fn workspace_project_runs_cli_serializes_original_query_identity() {
        for (command, expected) in [
            (
                "workspace project-runs --project project-a",
                json!({"type":"projectRuns","projectId":"project-a","offset":0,"revision":null}),
            ),
            (
                "workspace project-runs --project project-a --offset 17 --revision revision-7",
                json!({"type":"projectRuns","projectId":"project-a","offset":17,"revision":"revision-7"}),
            ),
            (
                "workspace project-run --project project-a --run run-9 --revision revision-7",
                json!({"type":"projectRun","projectId":"project-a","runId":"run-9","revision":"revision-7"}),
            ),
        ] {
            let Action::Workspace(query) = invocation(command).unwrap().action else {
                panic!("expected workspace query: {command}");
            };
            assert_eq!(serde_json::to_value(query).unwrap(), expected, "{command}");
        }
    }

    #[test]
    fn workspace_project_runs_cli_rejects_missing_invalid_extra_and_duplicate_options() {
        for command in [
            "workspace project-runs",
            "workspace project-runs --project",
            "workspace project-runs --project p --offset -1",
            "workspace project-runs --project p --offset nope",
            "workspace project-runs --project p --offset 4294967296",
            "workspace project-runs --project p --offset",
            "workspace project-runs --project p --revision",
            "workspace project-runs --project p --unknown value",
            "workspace project-runs --project p --approve true",
            "workspace project-runs --project p --run run",
            "workspace project-runs --project p extra",
            "workspace project-runs --project p --project other",
            "workspace project-runs --project p --offset 0 --offset 1",
            "workspace project-runs --project p --revision r --revision other",
            "workspace project-run",
            "workspace project-run --project p --run run",
            "workspace project-run --project p --revision r",
            "workspace project-run --run run --revision r",
            "workspace project-run --project p --run run --revision",
            "workspace project-run --project p --run run --revision r --offset 0",
            "workspace project-run --project p --run run --revision r --unknown value",
            "workspace project-run --project p --run run --revision r --approve true",
            "workspace project-run --project p --run run --revision r extra",
            "workspace project-run --project p --run run --revision r --project other",
            "workspace project-run --project p --run run --revision r --run other",
            "workspace project-run --project p --run run --revision r --revision other",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
        assert_eq!(
            invocation("workspace project-run --project p --run run")
                .err()
                .unwrap(),
            "缺少必需选项 --revision"
        );
        assert_eq!(
            invocation("workspace project-runs --project p --offset nope")
                .err()
                .unwrap(),
            "项目运行页偏移无效"
        );
    }

    #[test]
    fn workspace_project_runs_cli_help_lists_readonly_queries() {
        assert!(HELP.contains(
            "workspace project-runs --project <项目ID> [--offset <偏移>] [--revision <查询版本>]"
        ));
        assert!(HELP.contains(
            "workspace project-run --project <项目ID> --run <原运行ID> --revision <查询版本>"
        ));
    }

    use super::*;
    #[test]
    fn workspace_verification_result_cli_parses_metadata_and_versioned_pages() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace verification-result --project project-a --operation op-42")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::VerificationResult {
                project_id,
                operation_id,
                revision: None,
            }) if project_id == "project-a" && operation_id == "op-42"
        ));
        assert!(matches!(
            invocation("workspace verification-result --project project-a --operation op-42 --revision revision-7")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::VerificationResult {
                project_id,
                operation_id,
                revision: Some(revision),
            }) if project_id == "project-a" && operation_id == "op-42" && revision == "revision-7"
        ));

        for (verb, is_stages) in [
            ("verification-result-artifacts", false),
            ("verification-result-stages", true),
        ] {
            for (options, expected_offset) in [("", 0), (" --offset 17", 17)] {
                let action = invocation(&format!(
                    "workspace {verb} --project project-a --operation op-42 --revision revision-7{options}"
                ))
                .unwrap()
                .action;
                match (action, is_stages) {
                    (
                        Action::Workspace(WorkspaceQuery::VerificationResultArtifacts {
                            project_id,
                            operation_id,
                            offset,
                            revision,
                        }),
                        false,
                    ) => {
                        assert_eq!(project_id, "project-a");
                        assert_eq!(operation_id, "op-42");
                        assert_eq!(offset, expected_offset);
                        assert_eq!(revision, "revision-7");
                    }
                    (
                        Action::Workspace(WorkspaceQuery::VerificationResultStages {
                            project_id,
                            operation_id,
                            offset,
                            revision,
                        }),
                        true,
                    ) => {
                        assert_eq!(project_id, "project-a");
                        assert_eq!(operation_id, "op-42");
                        assert_eq!(offset, expected_offset);
                        assert_eq!(revision, "revision-7");
                    }
                    _ => panic!("unexpected query for {verb}"),
                }
            }
        }
    }
    #[test]
    fn workspace_verification_result_cli_rejects_missing_invalid_and_extra_arguments() {
        for command in [
            "workspace verification-result",
            "workspace verification-result --project p",
            "workspace verification-result --operation op",
            "workspace verification-result --project p --operation op --revision",
            "workspace verification-result --project p --operation op --unknown value",
            "workspace verification-result --project p --operation op --revision r --offset 0",
            "workspace verification-result-artifacts --project p --operation op",
            "workspace verification-result-artifacts --project p --revision r",
            "workspace verification-result-artifacts --operation op --revision r",
            "workspace verification-result-artifacts --project p --operation op --revision r --offset -1",
            "workspace verification-result-artifacts --project p --operation op --revision r --offset nope",
            "workspace verification-result-artifacts --project p --operation op --revision r --offset 4294967296",
            "workspace verification-result-artifacts --project p --operation op --revision r --unknown value",
            "workspace verification-result-stages --project p --operation op",
            "workspace verification-result-stages --project p --revision r",
            "workspace verification-result-stages --operation op --revision r",
            "workspace verification-result-stages --project p --operation op --revision r --offset -1",
            "workspace verification-result-stages --project p --operation op --revision r --offset nope",
            "workspace verification-result-stages --project p --operation op --revision r --offset 4294967296",
            "workspace verification-result-stages --project p --operation op --revision r --unknown value",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
    }
    #[test]
    fn workspace_revision_reviews_cli_parses_and_serializes_page_query() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace revision-reviews --project project-a")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::RevisionReviews {
                project_id,
                offset: 0,
                revision: None,
            }) if project_id == "project-a"
        ));
        let parsed = invocation(
            "workspace revision-reviews --project project-a --offset 7 --revision original",
        )
        .unwrap();
        let Action::Workspace(query) = parsed.action else {
            panic!("expected workspace query");
        };
        assert!(matches!(
            &query,
            WorkspaceQuery::RevisionReviews { project_id, offset: 7, revision: Some(revision) }
                if project_id == "project-a" && revision == "original"
        ));
        assert_eq!(
            serde_json::to_value(query).unwrap(),
            json!({
                "type": "revisionReviews",
                "projectId": "project-a",
                "offset": 7,
                "revision": "original",
            })
        );
        let Action::Workspace(query) =
            invocation("workspace revision-review --project project-a --revision original")
                .unwrap()
                .action
        else {
            panic!("expected workspace query");
        };
        assert_eq!(
            serde_json::to_value(query).unwrap(),
            json!({
                "type": "revisionReview",
                "projectId": "project-a",
                "revision": "original",
            })
        );
    }

    #[test]
    fn workspace_revision_reviews_cli_parses_detail_and_rejects_missing_unknown_invalid_duplicate()
    {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace revision-review --project p --revision r")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::RevisionReview { project_id, revision })
                if project_id == "p" && revision == "r"
        ));
        for command in [
            "workspace revision-reviews",
            "workspace revision-reviews --offset 1",
            "workspace revision-reviews --project p --offset -1",
            "workspace revision-reviews --project p --offset nope",
            "workspace revision-reviews --project p --offset 4294967296",
            "workspace revision-reviews --project p --unknown x",
            "workspace revision-reviews --project p --project q",
            "workspace revision-review --project p",
            "workspace revision-review --revision r",
            "workspace revision-review --project p --revision r --revision s",
            "workspace revision-review --project p --unknown x --revision r",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
    }

    #[test]
    fn workspace_revision_reviews_cli_help_mentions_both_commands() {
        assert!(HELP.contains("workspace revision-reviews --project"));
        assert!(HELP.contains("workspace revision-review --project"));
    }

    #[test]
    fn workspace_unsent_assignments_cli_parses_page_query() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace unsent-assignments --project project-a")
                .unwrap()
                .action,
            Action::Workspace(WorkspaceQuery::UnsentAssignments {
                project_id, offset: 0, revision: None,
            }) if project_id == "project-a"
        ));
        let Action::Workspace(query) = invocation(
            "workspace unsent-assignments --project project-a --offset 7 --revision original",
        )
        .unwrap()
        .action
        else {
            panic!("expected workspace query");
        };
        assert_eq!(
            serde_json::to_value(query).unwrap(),
            json!({"type":"unsentAssignments","projectId":"project-a","offset":7,"revision":"original"})
        );
    }

    #[test]
    fn workspace_unsent_assignment_cli_parses_detail_and_rejects_invalid_arguments() {
        let Action::Workspace(query) =
            invocation("workspace unsent-assignment --project p --task t --attempt a --revision r")
                .unwrap()
                .action
        else {
            panic!("expected workspace query");
        };
        assert_eq!(
            serde_json::to_value(query).unwrap(),
            json!({"type":"unsentAssignment","projectId":"p","taskId":"t","attemptId":"a","revision":"r"})
        );
        for command in [
            "workspace unsent-assignments",
            "workspace unsent-assignments --offset 1",
            "workspace unsent-assignments --project p --offset -1",
            "workspace unsent-assignments --project p --offset nope",
            "workspace unsent-assignments --project p --offset 4294967296",
            "workspace unsent-assignments --project p --unknown x",
            "workspace unsent-assignments --project p --project q",
            "workspace unsent-assignment --project p --task t --attempt a",
            "workspace unsent-assignment --project p --task t --revision r",
            "workspace unsent-assignment --project p --task t --attempt a --revision r --revision x",
            "workspace unsent-assignment --project p --task t --attempt a --revision r --unknown x",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
    }

    #[test]
    fn workspace_unsent_assignment_cli_help_lists_queries() {
        assert!(HELP.contains("workspace unsent-assignments --project"));
        assert!(HELP.contains("workspace unsent-assignment --project"));
    }

    #[test]
    fn workspace_codex_cancellation_cli_page_and_detail() {
        for (command, expected) in [
            (
                "workspace codex-cancellations --project p",
                json!({"type":"codexCancellations","projectId":"p","offset":0,"revision":null}),
            ),
            (
                "workspace codex-cancellations --project p --offset 7 --revision r",
                json!({"type":"codexCancellations","projectId":"p","offset":7,"revision":"r"}),
            ),
            (
                "workspace codex-cancellation --project p --attempt a --revision r",
                json!({"type":"codexCancellation","projectId":"p","attemptId":"a","revision":"r"}),
            ),
        ] {
            let Action::Workspace(query) = invocation(command).unwrap().action else {
                panic!("query")
            };
            assert_eq!(serde_json::to_value(query).unwrap(), expected);
        }
    }

    #[test]
    fn workspace_codex_cancellation_cli_invalid_arguments() {
        for command in [
            "workspace codex-cancellations",
            "workspace codex-cancellations --project p --offset -1",
            "workspace codex-cancellations --project p --offset nope",
            "workspace codex-cancellations --project p --offset 4294967296",
            "workspace codex-cancellations --project p --unknown x",
            "workspace codex-cancellations --project p --project q",
            "workspace codex-cancellation --project p --attempt a",
            "workspace codex-cancellation --project p --revision r",
            "workspace codex-cancellation --project p --attempt a --revision r --revision s",
            "workspace codex-cancellation --project p --attempt a --revision r --unknown x",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
    }

    #[test]
    fn workspace_codex_cancellation_cli_help() {
        assert!(HELP.contains("workspace codex-cancellations --project"));
        assert!(HELP.contains("workspace codex-cancellation --project"));
    }

    #[test]
    fn workspace_management_record_reviews_cli_serializes_global_pages_without_project() {
        for (command, expected) in [
            (
                "workspace management-record-reviews",
                json!({"type":"managementRecordReviews","offset":0,"revision":null}),
            ),
            (
                "workspace management-record-reviews --offset 7",
                json!({"type":"managementRecordReviews","offset":7,"revision":null}),
            ),
            (
                "workspace management-record-reviews --revision original",
                json!({"type":"managementRecordReviews","offset":0,"revision":"original"}),
            ),
            (
                "workspace management-record-reviews --offset 4294967295 --revision original",
                json!({"type":"managementRecordReviews","offset":4294967295u32,"revision":"original"}),
            ),
        ] {
            let Action::Workspace(query) = invocation(command).unwrap().action else {
                panic!("expected workspace query");
            };
            assert_eq!(serde_json::to_value(query).unwrap(), expected, "{command}");
        }
    }

    #[test]
    fn workspace_management_record_reviews_cli_preserves_original_detail_relative_path() {
        for relative_path in [
            "api-publication/reviews/original.json",
            r"api-publication\reviews\Original review.json",
            "api-publication/reviews/./原始审阅.json",
        ] {
            let args = [
                "--data-dir",
                "fixture",
                "workspace",
                "management-record-review",
                "--relative-path",
                relative_path,
                "--revision",
                "original",
            ]
            .map(str::to_owned);
            let Action::Workspace(query) = parse(&args).unwrap().action else {
                panic!("expected workspace query");
            };
            assert_eq!(
                serde_json::to_value(query).unwrap(),
                json!({
                    "type": "managementRecordReview",
                    "relativePath": relative_path,
                    "revision": "original",
                })
            );
        }
    }

    #[test]
    fn workspace_management_record_reviews_cli_rejects_missing_duplicate_unknown_project_and_offset_arguments(
    ) {
        for command in [
            "workspace management-record-reviews --offset",
            "workspace management-record-reviews --revision",
            "workspace management-record-reviews --offset -1",
            "workspace management-record-reviews --offset nope",
            "workspace management-record-reviews --offset 4294967296",
            "workspace management-record-reviews --offset 0 --offset 1",
            "workspace management-record-reviews --revision r --revision s",
            "workspace management-record-reviews --unknown x",
            "workspace management-record-reviews --project p",
            "workspace management-record-reviews --relative-path review.json",
            "workspace management-record-review",
            "workspace management-record-review --relative-path review.json",
            "workspace management-record-review --revision r",
            "workspace management-record-review --relative-path review.json --revision",
            "workspace management-record-review --revision r --relative-path",
            "workspace management-record-review --relative-path review.json --revision r --relative-path other.json",
            "workspace management-record-review --relative-path review.json --revision r --revision s",
            "workspace management-record-review --relative-path review.json --revision r --unknown x",
            "workspace management-record-review --relative-path review.json --revision r --project p",
            "workspace management-record-review --relative-path review.json --revision r --offset 0",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
    }

    #[test]
    fn workspace_management_record_reviews_cli_help_lists_global_queries() {
        assert!(HELP.contains(
            "workspace management-record-reviews [--offset <序号>] [--revision <查询版本>]"
        ));
        assert!(HELP.contains(
            "workspace management-record-review --relative-path <原相对路径> --revision <查询版本>"
        ));
    }

    #[test]
    fn workspace_api_publication_reviews_cli_serializes_global_pages_without_project() {
        for (command, expected) in [
            (
                "workspace api-publication-reviews",
                json!({"type":"apiPublicationReviews","offset":0,"revision":null}),
            ),
            (
                "workspace api-publication-reviews --offset 7",
                json!({"type":"apiPublicationReviews","offset":7,"revision":null}),
            ),
            (
                "workspace api-publication-reviews --revision original",
                json!({"type":"apiPublicationReviews","offset":0,"revision":"original"}),
            ),
            (
                "workspace api-publication-reviews --offset 4294967295 --revision original",
                json!({"type":"apiPublicationReviews","offset":4294967295u32,"revision":"original"}),
            ),
        ] {
            let Action::Workspace(query) = invocation(command).unwrap().action else {
                panic!("expected workspace query");
            };
            assert_eq!(serde_json::to_value(query).unwrap(), expected, "{command}");
        }
    }

    #[test]
    fn workspace_api_publication_reviews_cli_preserves_original_detail_relative_path() {
        for relative_path in [
            "api-publication/reviews/original.json",
            r"api-publication\reviews\Original review.json",
            "api-publication/reviews/./原始审阅.json",
        ] {
            let args = [
                "--data-dir",
                "fixture",
                "workspace",
                "api-publication-review",
                "--relative-path",
                relative_path,
                "--revision",
                "original",
            ]
            .map(str::to_owned);
            let Action::Workspace(query) = parse(&args).unwrap().action else {
                panic!("expected workspace query");
            };
            assert_eq!(
                serde_json::to_value(query).unwrap(),
                json!({
                    "type": "apiPublicationReview",
                    "relativePath": relative_path,
                    "revision": "original",
                })
            );
        }
    }

    #[test]
    fn workspace_api_publication_reviews_cli_rejects_missing_duplicate_unknown_project_and_offset_arguments(
    ) {
        for command in [
            "workspace api-publication-reviews --offset",
            "workspace api-publication-reviews --revision",
            "workspace api-publication-reviews --offset -1",
            "workspace api-publication-reviews --offset nope",
            "workspace api-publication-reviews --offset 4294967296",
            "workspace api-publication-reviews --offset 0 --offset 1",
            "workspace api-publication-reviews --revision r --revision s",
            "workspace api-publication-reviews --unknown x",
            "workspace api-publication-reviews --project p",
            "workspace api-publication-reviews --relative-path review.json",
            "workspace api-publication-review",
            "workspace api-publication-review --relative-path review.json",
            "workspace api-publication-review --revision r",
            "workspace api-publication-review --relative-path review.json --revision",
            "workspace api-publication-review --revision r --relative-path",
            "workspace api-publication-review --relative-path review.json --revision r --relative-path other.json",
            "workspace api-publication-review --relative-path review.json --revision r --revision s",
            "workspace api-publication-review --relative-path review.json --revision r --unknown x",
            "workspace api-publication-review --relative-path review.json --revision r --project p",
            "workspace api-publication-review --relative-path review.json --revision r --offset 0",
        ] {
            assert!(invocation(command).is_err(), "{command}");
        }
    }

    #[test]
    fn workspace_api_publication_reviews_cli_help_lists_global_queries() {
        assert!(HELP.contains(
            "workspace api-publication-reviews [--offset <序号>] [--revision <查询版本>]"
        ));
        assert!(HELP.contains(
            "workspace api-publication-review --relative-path <原相对路径> --revision <查询版本>"
        ));
    }

    fn invocation(line: &str) -> Result<Invocation, String> {
        let args = format!("--data-dir fixture {line}")
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        parse(&args)
    }
    #[test]
    fn historical_product_cli_requires_explicit_file_and_confirm_digest() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("input.json");
        std::fs::write(&file,serde_json::to_vec(&json!({"projectId":"p","scope":{"kind":"management","operationId":"old","originalProjectId":null},"amountUsdNanos":0,"billingReference":"zero-fee statement","explanation":"explicit verification"})).unwrap()).unwrap();
        let make = |verb: &str, extra: Vec<String>| {
            let mut args = vec![
                "--data-dir".into(),
                "fixture".into(),
                "history".into(),
                verb.into(),
                "--file".into(),
                file.to_string_lossy().into(),
            ];
            args.extend(extra);
            parse(&args)
        };
        assert!(
            matches!(make("review",vec![]).unwrap().action,Action::Request(c) if matches!(*c,Command::ReviewHistoricalCharge {..}))
        );
        assert!(make("confirm", vec![]).is_err());
        assert!(
            matches!(make("confirm",vec!["--review-digest".into(),"chosen".into()]).unwrap().action,Action::Request(c) if matches!(*c,Command::ConfirmHistoricalCharge {ref review_digest,..} if review_digest=="chosen"))
        );
    }
    #[test]
    fn historical_paging_cli_queries_do_not_construct_confirmation_commands() {
        assert!(
            matches!(invocation("history sources").unwrap().action,Action::Request(c) if matches!(*c,Command::ReadHistoricalSources{offset:0,revision:None}))
        );
        assert!(
            matches!(invocation("history accounted --offset 20 --revision original").unwrap().action,Action::Request(c) if matches!(*c,Command::ReadHistoricalAccounting{offset:20,revision:Some(_)}))
        );
        assert!(invocation("history sources --offset -1").is_err());
        assert!(invocation("history show --project p").is_err());
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("scope.json");
        std::fs::write(
            &file,
            br#"{"kind":"management","operationId":"old","originalProjectId":null}"#,
        )
        .unwrap();
        let args = vec![
            "--data-dir".into(),
            "fixture".into(),
            "history".into(),
            "show".into(),
            "--project".into(),
            "p".into(),
            "--scope-file".into(),
            file.to_string_lossy().into(),
            "--review-digest".into(),
            "original".into(),
        ];
        assert!(
            matches!(parse(&args).unwrap().action,Action::Request(c) if matches!(*c,Command::ReadHistoricalConfirmation{..}))
        );
    }
    #[test]
    fn currency_promotion_requires_original_hold_backup_and_review() {
        assert!(
            invocation("data-format review-currency --from archive --manifest-digest m").is_err()
        );
        assert!(invocation(
            "data-format promote-currency --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-currency --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::CurrencyReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-currency --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::CurrencyPromote(_, _, _, _)));
    }
    #[test]
    fn preparation_cli_requires_original_hold_backup_and_review() {
        assert!(
            invocation("data-format review-preparation --from archive --manifest-digest m")
                .is_err()
        );
        assert!(invocation(
            "data-format promote-preparation --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-preparation --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::PreparationReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-preparation --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::PreparationPromote(_, _, _, _)));
    }
    #[test]
    fn counting_cli_requires_original_hold_backup_and_review() {
        assert!(
            invocation("data-format review-counting --from archive --manifest-digest m").is_err()
        );
        assert!(invocation(
            "data-format promote-counting --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-counting --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::CountingReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-counting --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::CountingPromote(_, _, _, _)));
    }
    #[test]
    fn count_resolution_cli_requires_original_hold_backup_and_review() {
        assert!(invocation(
            "data-format review-count-resolution --from archive --manifest-digest m"
        )
        .is_err());
        assert!(invocation(
            "data-format promote-count-resolution --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-count-resolution --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::CountResolutionReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-count-resolution --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::CountResolutionPromote(_, _, _, _)));
    }
    #[test]
    fn scoped_pricing_cli_requires_original_hold_backup_and_review() {
        assert!(
            invocation("data-format review-scoped-pricing --from archive --manifest-digest m")
                .is_err()
        );
        assert!(invocation(
            "data-format promote-scoped-pricing --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-scoped-pricing --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::ScopedPricingReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-scoped-pricing --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::ScopedPricingPromote(_, _, _, _)));
    }
    #[test]
    fn scoped_count_cli_requires_original_hold_backup_and_review() {
        assert!(
            invocation("data-format review-scoped-count --from archive --manifest-digest m")
                .is_err()
        );
        assert!(invocation(
            "data-format promote-scoped-count --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-scoped-count --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::ScopedCountReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-scoped-count --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::ScopedCountPromote(_, _, _, _)));
    }
    #[test]
    fn scoped_execution_cli_requires_original_hold_backup_and_review() {
        assert!(invocation(
            "data-format review-scoped-execution --from archive --manifest-digest m"
        )
        .is_err());
        assert!(invocation(
            "data-format promote-scoped-execution --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-scoped-execution --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::ScopedExecutionReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-scoped-execution --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::ScopedExecutionPromote(_, _, _, _)));
    }
    #[test]
    fn api_currency_cli_requires_original_hold_backup_and_review() {
        assert!(
            invocation("data-format review-api-currency --from archive --manifest-digest m")
                .is_err()
        );
        assert!(invocation(
            "data-format promote-api-currency --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-api-currency --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::ApiCurrencyReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-api-currency --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::ApiCurrencyPromote(_, _, _, _)));
    }
    #[test]
    fn api_fee_resolution_cli_requires_original_hold_backup_and_review() {
        assert!(invocation(
            "data-format review-api-fee-resolution --from archive --manifest-digest m"
        )
        .is_err());
        assert!(invocation(
            "data-format promote-api-fee-resolution --from archive --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(
            invocation(
                "data-format review-api-fee-resolution --from archive --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::ApiFeeResolutionReview(_, _, _)
        ));
        assert!(matches!(invocation("data-format promote-api-fee-resolution --from archive --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::ApiFeeResolutionPromote(_, _, _, _)));
    }
    #[test]
    fn workspace_history_cli_requires_original_detail_version() {
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(matches!(
            invocation("workspace history").unwrap().action,
            Action::Workspace(WorkspaceQuery::History {
                project_id: None,
                offset: 0,
                revision: None
            })
        ));
        assert!(
            matches!(invocation("workspace history --project p --offset 20 --revision original").unwrap().action, Action::Workspace(WorkspaceQuery::History {project_id: Some(p), offset: 20, revision: Some(r)}) if p == "p" && r == "original")
        );
        assert!(
            matches!(invocation("workspace history-entry --sequence 22 --revision original").unwrap().action, Action::Workspace(WorkspaceQuery::HistoryEntry {project_id: None, sequence: 22, revision}) if revision == "original")
        );
        for command in [
            "workspace history --offset -1",
            "workspace history-entry --sequence 22",
            "workspace history-entry --revision original",
            "workspace history --unknown true",
        ] {
            assert!(invocation(command).is_err());
        }
    }

    #[test]
    fn planning_budget_cli_requires_explicit_configuration_and_does_not_request_plan() {
        assert!(invocation("plan authorize-budget --project p").is_err());
        let Action::Request(command) =
            invocation("plan authorize-budget --project p --setup-digest chosen")
                .unwrap()
                .action
        else {
            panic!()
        };
        assert!(
            matches!(*command,Command::AuthorizePlanningBudget{project_id,setup_digest} if project_id=="p"&&setup_digest=="chosen")
        );
    }
    #[test]
    fn approval_and_run_require_explicit_reviewed_digests() {
        for command in [
            "plan approve --project p",
            "run start --project p --revision-digest abc",
            "task cancel --adapter codex --project p --attempt a --reason stop",
        ] {
            assert!(invocation(command).is_err());
        }
        let Action::Request(c) = invocation("plan approve --project p --digest chosen")
            .unwrap()
            .action
        else {
            panic!()
        };
        assert!(
            matches!(*c,Command::ApproveRevision{project_id,digest} if project_id=="p"&&digest=="chosen")
        );
        let Action::Request(c) =
            invocation("run resume --project p --revision-digest original --setup-digest setup")
                .unwrap()
                .action
        else {
            panic!()
        };
        assert!(
            matches!(*c,Command::StartProjectRun{revision_digest,..} if revision_digest=="original")
        );
    }
    #[test]
    fn duplicate_unknown_and_role_flags_fail_before_transport() {
        for command in [
            "status --principal core",
            "status --yes true",
            "plan approve --project p --digest a --digest b",
            "task recover --adapter worker --project p --attempt a",
            "credentials set --provider openai --key secret-sentinel",
            "management list --offset -1",
        ] {
            let err = invocation(command).err().unwrap();
            assert!(!err.contains("secret-sentinel"));
        }
    }
    #[test]
    fn maintenance_reservation_requires_original_digests_and_explicit_release_evidence() {
        assert!(matches!(
            invocation("maintenance status").unwrap().action,
            Action::MaintenanceStatus(None)
        ));
        assert!(matches!(
            invocation("maintenance status --guard-digest g")
                .unwrap()
                .action,
            Action::MaintenanceStatus(Some(_))
        ));
        assert!(invocation("maintenance reserve --transaction tx --intent-digest i").is_err());
        assert!(matches!(
            invocation("maintenance reserve --transaction tx --intent-digest i --hold-digest h")
                .unwrap()
                .action,
            Action::MaintenanceReserve(_, _, _)
        ));
        assert!(invocation("maintenance release --guard-digest g").is_err());
        assert!(matches!(
            invocation("maintenance release --guard-digest g --plan-digest p")
                .unwrap()
                .action,
            Action::MaintenanceRelease(_, Some(_))
        ));
        assert!(matches!(
            invocation("maintenance abandon --guard-digest g")
                .unwrap()
                .action,
            Action::MaintenanceRelease(_, None)
        ));
        assert!(invocation("maintenance abandon --guard-digest g --force true").is_err());
    }
    #[test]
    fn recovery_commands_require_review_and_do_not_connect_to_core() {
        assert!(invocation(
            "recovery review-previous --from saved --manifest-digest m --hold-digest current"
        )
        .is_err());
        assert!(matches!(invocation("recovery review-previous --from saved --manifest-digest m --hold-digest current --backup-hold-digest old").unwrap().action, Action::RecoveryPreviousReview(_, _, _, _)));
        assert!(invocation("recovery prepare-previous --from saved --manifest-digest m --hold-digest current --backup-hold-digest old").is_err());
        assert!(matches!(invocation("recovery prepare-previous --from saved --manifest-digest m --hold-digest current --backup-hold-digest old --review-digest r").unwrap().action, Action::RecoveryPreviousPrepare(_, _, _, _, _)));
        assert!(invocation("recovery prepare-previous --from saved --manifest-digest m --hold-digest current --backup-hold-digest old --review-digest r --force true").is_err());
        assert!(matches!(
            invocation("recovery diagnose").unwrap().action,
            Action::RecoveryDiagnose
        ));
        assert!(invocation("recovery diagnose --force true").is_err());
        assert!(invocation("recovery inspect").is_err());
        assert!(matches!(
            invocation("recovery inspect --review-digest r")
                .unwrap()
                .action,
            Action::RecoveryInspect(_)
        ));
        assert!(matches!(
            invocation("recovery status").unwrap().action,
            Action::RecoveryStatus
        ));
        assert!(
            invocation("recovery prepare --from saved --manifest-digest m --hold-digest h")
                .is_err()
        );
        assert!(matches!(invocation("recovery prepare --from saved --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::RecoveryPrepare(_, _, _, _)));
        assert!(invocation("recovery apply").is_err());
        assert!(matches!(
            invocation("recovery finish --review-digest r")
                .unwrap()
                .action,
            Action::RecoveryFinish(_, false)
        ));
        assert!(matches!(
            invocation("recovery abandon --review-digest r")
                .unwrap()
                .action,
            Action::RecoveryFinish(_, true)
        ));
        assert!(invocation("recovery apply --review-digest r --force true").is_err());
    }
    #[test]
    fn data_migration_requires_backup_hold_and_review_without_starting_core() {
        assert!(matches!(
            invocation("data-format status").unwrap().action,
            Action::DataFormatStatus
        ));
        assert!(
            invocation("data-format migrate --from saved --manifest-digest m --hold-digest h")
                .is_err()
        );
        assert!(matches!(invocation("data-format migrate --from saved --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::DataFormatMigrate(_, _, _, _)));
        assert!(invocation("data-format migrate --from saved --manifest-digest m --hold-digest h --review-digest r --force true").is_err());
        assert!(matches!(
            invocation(
                "data-format review-reader --from saved --manifest-digest m --hold-digest h"
            )
            .unwrap()
            .action,
            Action::ReaderReview(_, _, _)
        ));
        assert!(invocation(
            "data-format promote-reader --from saved --manifest-digest m --hold-digest h"
        )
        .is_err());
        assert!(matches!(invocation("data-format promote-reader --from saved --manifest-digest m --hold-digest h --review-digest r").unwrap().action, Action::ReaderPromote(_, _, _, _)));
        assert!(invocation("data-format promote-reader --from saved --manifest-digest m --hold-digest h --review-digest r --force true").is_err());
    }
    #[test]
    fn keys_accept_one_line_ending_and_reject_embedded_controls() {
        for suffix in ["", "\n", "\r\n"] {
            let text = format!("synthetic-not-a-real-key{suffix}");
            let k = key_from_input(&mut text.as_bytes()).unwrap();
            assert_eq!(k.expose(), "synthetic-not-a-real-key");
        }
        for text in [
            "secret\nsecond-secret",
            "synthetic-not-a-real-key\n\n",
            " ",
            &"x".repeat(515),
        ] {
            assert!(key_from_input(&mut text.as_bytes()).is_err());
        }
    }
    #[test]
    fn all_first_batch_credentials_use_the_existing_vault_command() {
        for p in ["openai", "anthropic", "deepseek", "qwen", "kimi", "grok"] {
            assert!(matches!(
                invocation(&format!("credentials set --provider {p}"))
                    .unwrap()
                    .action,
                Action::SetKey(_)
            ));
        }
    }

    #[test]
    fn api_generation_product_cli_requires_file_digest_and_original_lookup_identity() {
        assert!(
            matches!(invocation("api-generation list").unwrap().action,Action::Request(c) if matches!(*c,Command::ReadApiGenerations {offset:0,revision:None}))
        );
        assert!(invocation("api-generation list --offset -1").is_err());
        assert!(invocation("api-generation show --project p --request r").is_err());
        assert!(
            matches!(invocation("api-generation show --project p --request r --review-digest d").unwrap().action,Action::Request(c) if matches!(*c,Command::ReadApiGenerationConfirmation {..}))
        );
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("input.json");
        std::fs::write(&file,serde_json::to_vec(&json!({"projectId":"p","requestId":"r","amountUsdNanos":0,"billingReference":"explicit zero","explanation":"user reviewed"})).unwrap()).unwrap();
        let make = |verb: &str, extra: Vec<String>| {
            let mut args = vec![
                "--data-dir".into(),
                "fixture".into(),
                "api-generation".into(),
                verb.into(),
                "--file".into(),
                file.to_string_lossy().into(),
            ];
            args.extend(extra);
            parse(&args)
        };
        assert!(
            matches!(make("review",vec![]).unwrap().action,Action::Request(c) if matches!(*c,Command::ReviewApiGenerationResolution {..}))
        );
        assert!(make("confirm", vec![]).is_err());
        assert!(
            matches!(make("confirm",vec!["--review-digest".into(),"chosen".into()]).unwrap().action,Action::Request(c) if matches!(*c,Command::ConfirmApiGenerationResolution {ref review_digest,..} if review_digest=="chosen"))
        );
    }
    #[test]
    fn evidence_archive_cli_is_explicit_and_requires_external_expected_digests() {
        assert!(matches!(invocation("evidence-archive create --index input.json --index-sha256 selected --to destination").unwrap().action, Action::EvidenceArchiveCreate(_,_,_)));
        assert!(matches!(
            invocation("evidence-archive verify --from archive --manifest-sha256 selected")
                .unwrap()
                .action,
            Action::EvidenceArchiveVerify(_, _)
        ));
        for command in [
            "evidence-archive create --index input.json --to destination",
            "evidence-archive verify --from archive",
            "evidence-archive verify --from archive --manifest-sha256 selected --execute yes",
        ] {
            assert!(invocation(command).is_err());
        }
    }
    #[test]
    fn api_count_product_cli_requires_file_digest_and_original_lookup_identity() {
        assert!(
            matches!(invocation("api-count list").unwrap().action,Action::Request(c) if matches!(*c,Command::ReadApiCounts {offset:0,revision:None}))
        );
        assert!(invocation("api-count list --offset -1").is_err());
        assert!(invocation("api-count show --project p --request r").is_err());
        assert!(
            matches!(invocation("api-count show --project p --request r --review-digest d").unwrap().action,Action::Request(c) if matches!(*c,Command::ReadApiCountConfirmation {..}))
        );
        let t = tempfile::tempdir().unwrap();
        let file = t.path().join("input.json");
        std::fs::write(&file,serde_json::to_vec(&json!({"projectId":"p","requestId":"r","amountUsdNanos":0,"billingReference":"explicit zero","explanation":"user reviewed"})).unwrap()).unwrap();
        let make = |verb: &str, extra: Vec<String>| {
            let mut args = vec![
                "--data-dir".into(),
                "fixture".into(),
                "api-count".into(),
                verb.into(),
                "--file".into(),
                file.to_string_lossy().into(),
            ];
            args.extend(extra);
            parse(&args)
        };
        assert!(
            matches!(make("review",vec![]).unwrap().action,Action::Request(c) if matches!(*c,Command::ReviewApiCountResolution {..}))
        );
        assert!(make("confirm", vec![]).is_err());
        assert!(
            matches!(make("confirm",vec!["--review-digest".into(),"chosen".into()]).unwrap().action,Action::Request(c) if matches!(*c,Command::ConfirmApiCountResolution {ref review_digest,..} if review_digest=="chosen"))
        );
    }
}
