param([Parameter(Mandatory=$true)][string]$Session,[Parameter(Mandatory=$true)][ValidatePattern('^[a-f0-9]{64}$')][string]$SessionDigest,[switch]$Describe,[switch]$DataMaintenance,[switch]$ImportRetention)
if($ImportRetention){$DataMaintenance=[switch]$true}
$ErrorActionPreference='Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -ReferencedAssemblies System.Windows.Forms -TypeDefinition @'
public class CeruleanMaintenanceForm : System.Windows.Forms.Form {
    public bool PreviewMode;
    protected override bool ShowWithoutActivation { get { return PreviewMode; } }
}
public sealed class CeruleanMaintenanceOperation {
    public System.Diagnostics.Process Process;
    public System.Threading.Tasks.Task Completion;
    static async System.Threading.Tasks.Task Capture(System.IO.StreamReader source, System.IO.StreamWriter target) {
        try {
            var buffer = new char[4096];
            int count;
            while ((count = await source.ReadAsync(buffer, 0, buffer.Length).ConfigureAwait(false)) != 0)
                await target.WriteAsync(buffer, 0, count).ConfigureAwait(false);
            await target.FlushAsync().ConfigureAwait(false);
        } finally { target.Dispose(); }
    }
    public static CeruleanMaintenanceOperation Start(string file, string arguments, string stdout, string stderr) {
        System.IO.StreamWriter output = null, error = null;
        var process = new System.Diagnostics.Process();
        bool started = false;
        try {
            output = new System.IO.StreamWriter(new System.IO.FileStream(stdout, System.IO.FileMode.CreateNew,
                System.IO.FileAccess.Write, System.IO.FileShare.Read), new System.Text.UTF8Encoding(false));
            error = new System.IO.StreamWriter(new System.IO.FileStream(stderr, System.IO.FileMode.CreateNew,
                System.IO.FileAccess.Write, System.IO.FileShare.Read), new System.Text.UTF8Encoding(false));
            process.StartInfo = new System.Diagnostics.ProcessStartInfo(file, arguments) {
                UseShellExecute = false, CreateNoWindow = true,
                RedirectStandardOutput = true, RedirectStandardError = true,
                StandardOutputEncoding = new System.Text.UTF8Encoding(false),
                StandardErrorEncoding = new System.Text.UTF8Encoding(false)
            };
            if (!process.Start()) throw new System.Exception("Maintenance process did not start");
            started = true;
            var capturedOutput = Capture(process.StandardOutput, output);
            var capturedError = Capture(process.StandardError, error);
            var exited = System.Threading.Tasks.Task.Run(() => process.WaitForExit());
            return new CeruleanMaintenanceOperation {
                Process = process,
                Completion = System.Threading.Tasks.Task.WhenAll(exited, capturedOutput, capturedError)
            };
        } catch {
            // Never kill or automatically repeat a process after a start whose outcome is unknown.
            if (!started) { if (output != null) output.Dispose(); if (error != null) error.Dispose(); process.Dispose(); }
            throw;
        }
    }
}
'@
[Windows.Forms.Application]::EnableVisualStyles()
function NormalPath([string]$path){
    $value=[IO.Path]::GetFullPath($path).TrimEnd('\')
    if($value.StartsWith('\\?\UNC\')){return '\\'+$value.Substring(8)}
    if($value.StartsWith('\\?\')){return $value.Substring(4)}
    return $value
}
$directory=NormalPath $PSScriptRoot
$pins=[Collections.Generic.List[IDisposable]]::new()
$operation=$null;$lastReview=$null;$lastHold=$null;$sourceInfo=$null;$candidateInfo=$null
$lastFormatReview=$null;$lastFormatStatus=$null
$lastRecoveryReview=$null
$lastReaderReview=$null;$lastRecoveryMode=$null
$lastScopedPricingReview=$null
$lastScopedCountReview=$null
$lastImportReview=$null
function Digest([string]$path){return (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()}
function ReadBounded([string]$path){if((Get-Item -LiteralPath $path).Length -gt 4194304){throw '维护记录超过显示上限，请审阅原文件'};return [IO.File]::ReadAllText($path,[Text.Encoding]::UTF8)}
function QuoteArgument([string]$value){
    if($value -match '["\x00-\x1f]'){throw '路径或参数含不支持的字符'}
    # Every variable value is quoted; double trailing backslashes for Windows argv.
    return '"'+[regex]::Replace($value,'(\\+)$','$1$1')+'"'
}
try{
    $pins.Add([IO.File]::Open($Session,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read))
    if((NormalPath (Split-Path -Parent $Session)) -ine $directory -or (Digest $Session) -cne $SessionDigest){throw '维护会话身份或摘要不符'}
    $config=(ReadBounded $Session) | ConvertFrom-Json
    if($config.schema -ne 1 -or $config.ownerSid -cne [Security.Principal.WindowsIdentity]::GetCurrent().User.Value -or (NormalPath $config.directory) -ine $directory){throw '维护会话用户或目录不符'}
    $required=@('manage-windows-update.ps1','test-windows-package.ps1','maintenance-window.ps1','cerulean-runtime.exe')
    if(@($config.artifacts).Count -ne $required.Count){throw '维护工具清单不完整'}
    foreach($name in $required){
        $entries=@($config.artifacts | Where-Object {$_.path -ceq $name})
        if($entries.Count -ne 1){throw '维护工具名称重复或缺失'}
        $path=Join-Path $directory $name
        $pins.Add([IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read))
        if((Get-Item -LiteralPath $path).Length -ne $entries[0].bytes -or (Digest $path) -cne $entries[0].sha256){throw '维护工具已改变，未启动任何升级操作'}
    }
    $helper=Get-Content -LiteralPath (Join-Path $directory 'test-windows-package.ps1') -Raw -Encoding UTF8
    $typeSource=[regex]::Match($helper,"(?s)Add-Type -TypeDefinition @'\r?\n(.*?)\r?\n'@").Groups[1].Value
    if(-not $typeSource){throw '维护句柄组件缺失'}
    Add-Type -TypeDefinition $typeSource
    $pins.Add([CeruleanPackagePin]::Open($directory,$true))
    $pins.Add([CeruleanPackagePin]::Open($Session,$false))
    if((Digest $Session) -cne $SessionDigest){throw '维护会话在读取后改变'}
    foreach($entry in $config.artifacts){$pins.Add([CeruleanPackagePin]::Open((Join-Path $directory $entry.path),$false));if((Digest (Join-Path $directory $entry.path)) -cne $entry.sha256){throw '维护工具在读取后改变'}}

    $form=[CeruleanMaintenanceForm]::new();$form.Text='CeruleanLonging · 升级维护';$form.Width=1050;$form.Height=850;$form.MinimumSize=[Drawing.Size]::new(900,720)
    if($DataMaintenance){$form.Text='CeruleanLonging · 数据迁移与恢复'}
    if($ImportRetention){$form.Text='CeruleanLonging · 导入处置'}
    $form.StartPosition='CenterScreen';$form.Font=[Drawing.Font]::new('Microsoft YaHei UI',10)
    $layout=[Windows.Forms.TableLayoutPanel]::new();$layout.Dock='Fill';$layout.AutoScroll=$true;$layout.Padding=[Windows.Forms.Padding]::new(18);$layout.ColumnCount=2
    $layout.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new([Windows.Forms.SizeType]::Absolute,155)) | Out-Null
    $layout.ColumnStyles.Add([Windows.Forms.ColumnStyle]::new([Windows.Forms.SizeType]::Percent,100)) | Out-Null
    $form.Controls.Add($layout);$row=0;$actionButtons=[Collections.Generic.List[object]]::new()
    function Row([string]$label,$control){
        $caption=[Windows.Forms.Label]::new();$caption.Text=$label;$caption.AutoSize=$true;$caption.Margin=[Windows.Forms.Padding]::new(0,8,10,8)
        $control.Dock='Fill';$control.Margin=[Windows.Forms.Padding]::new(0,5,0,5)
        $layout.Controls.Add($caption,0,$script:row);$layout.Controls.Add($control,1,$script:row);$script:row++
    }
    function Field([string]$label,[string]$value='',[bool]$readOnly=$false){$box=[Windows.Forms.TextBox]::new();$box.Text=$value;$box.ReadOnly=$readOnly;$box.AccessibleName=$label;Row $label $box;return $box}
    function Buttons([string]$label,$spec){
        $panel=[Windows.Forms.FlowLayoutPanel]::new();$panel.AutoSize=$true;$panel.WrapContents=$true
        foreach($item in $spec){$button=[Windows.Forms.Button]::new();$button.Text=$item[0];$button.Tag=$item[1];$button.AutoSize=$true;$button.Padding=[Windows.Forms.Padding]::new(8,3,8,3);$button.Add_Click({param($sender,$eventArgs) StartAction ([string]$sender.Tag)});$panel.Controls.Add($button);$actionButtons.Add($button)}
        Row $label $panel
    }
    $intro=[Windows.Forms.Label]::new();$intro.AutoSize=$true;$intro.MaximumSize=[Drawing.Size]::new(780,0);$intro.Text="维护工具 $($config.version)。先审阅并准备核心，再准备备份。执行更新或回滚前请关闭所有工作台窗口；本窗口可保持打开。操作不会自动启动项目。";Row '使用顺序' $intro
    if($DataMaintenance){$intro.Text='先完成程序升级与维护收尾。锁定核心并备份后，可审阅数据格式迁移、监督配置或金额预算格式升级。格式升级不会自动批准预算或启动任务。启用后旧版不能直接读取，需要恢复已审阅的旧备份。旧备份的锁定已解除时，先取得当前新锁定，再填入备份原锁定摘要。恢复须审阅、准备、保留原目录后执行并核验收尾；最后明确解除数据锁定，不自动启动项目。'}
    if($ImportRetention){$intro.Text='先关闭连接此数据目录的工作台窗口，再确认停止核心、创建冷备份并审阅保留决定。每次变更需单独确认。原目录保持只读；保留决定不恢复任务或清空记录。备份含本机加密凭证，不包含外部项目，不用于分享。'}
    $install=Field '原安装目录' (NormalPath $config.installationDirectory) $true
    $data=Field '数据目录' (NormalPath $config.dataDirectory) $true
    if(-not $ImportRetention){Buttons '核心状态' @(@('审阅升级状态','coreReview'),@('读取现有锁定','coreStatus'),@('确认锁定并停止核心','corePrepare'))}
    if($ImportRetention){
        $backup=Field '数据备份目录' (Join-Path (Split-Path -Parent $data.Text) ('CeruleanImportBackup-'+[DateTime]::Now.ToString('yyyyMMdd-HHmmss')))
        $backupDigest=Field '已审阅备份摘要'
        $retentionDirectory=Field '保留决定目录' (Join-Path (Split-Path -Parent $data.Text) ('import-retention-'+[DateTime]::Now.ToString('yyyyMMdd-HHmmss')))
        $retentionReason=Field '保留原因'
        $retentionDigest=Field '原保留审阅摘要'
        $consent=[Windows.Forms.CheckBox]::new();$consent.AutoSize=$true;$consent.Text='已审阅当前目录与结果，确认本次操作';Row '操作确认' $consent
        Buttons '停止与冷备份' @(@('正常停止原核心','importStop'),@('创建原数据冷备份','importBackup'),@('核对原冷备份','importVerify'))
        Buttons '保留决定' @(@('审阅原导入保留','importReview'),@('确认保存保留决定','importRetain'),@('核查原保留决定','importInspect'))
        function ClearImportReview {$script:lastImportReview=$null;$consent.Checked=$false}
        $inputs=@($backup,$backupDigest,$retentionDirectory,$retentionReason,$retentionDigest,$consent)
        foreach($field in @($backup,$backupDigest,$retentionDirectory,$retentionReason,$retentionDigest)){$field.Add_TextChanged({ClearImportReview})}
    }elseif($DataMaintenance){
        $backup=Field '数据备份目录' (Join-Path (Split-Path -Parent $data.Text) ('CeruleanDataBackup-'+[DateTime]::Now.ToString('yyyyMMdd-HHmmss')))
        $backupDigest=Field '已审阅备份摘要'
        $migrationDigest=Field '迁移审阅摘要' '' $true
        $readerDigest=Field '监督配置启用摘要' '' $true
        $currencyDigest=Field '预算格式启用摘要' '' $true
        $preparationDigest=Field '预算执行准备启用摘要' '' $true
        $countingDigest=Field '计数费用记录启用摘要' '' $true
        $countResolutionDigest=Field '计数核销与终止记录启用摘要' '' $true
        $scopedPricingDigest=Field '服务档位与上下文计价启用摘要' '' $true
        $scopedCountDigest=Field '分档管理计数记录启用摘要' '' $true
        $scopedExecutionDigest=Field '分档决策与费用确认启用摘要' '' $true
        $apiCurrencyDigest=Field 'API 逐轮计数与费用确认启用摘要' '' $true
        $apiFeeResolutionDigest=Field 'API 未知费用核对启用摘要' '' $true
        $recoveryHold=Field '当前数据锁定摘要'
        $backupHold=Field '旧备份原锁定摘要'
        $recoveryDigest=Field '原恢复审阅摘要'
        Buttons '备份核对' @(@('创建锁定数据备份','dataBackup'),@('核对备份文件','dataVerify'))
        $consent=[Windows.Forms.CheckBox]::new();$consent.AutoSize=$true;$consent.Text='已审阅当前状态、目录和包版本，确认执行所选变更';Row '操作确认' $consent
        Buttons '数据格式' @(@('读取当前数据格式','formatStatus'),@('审阅数据迁移','formatReview'),@('确认迁移到格式2','formatMigrate'))
        Buttons '新版监督配置' @(@('审阅启用监督配置','readerReview'),@('确认启用监督配置','readerPromote'))
        Buttons '金额预算数据格式' @(@('审阅预算格式升级','currencyReview'),@('确认启用预算格式','currencyPromote'))
        Buttons '金额预算执行准备' @(@('审阅预算执行准备','preparationReview'),@('确认启用预算执行准备','preparationPromote'))
        Buttons '计数费用记录' @(@('审阅计数费用记录','countingReview'),@('确认启用计数费用记录','countingPromote'))
        Buttons '计数核销与终止记录' @(@('审阅计数核销与终止记录','countResolutionReview'),@('确认启用计数核销与终止记录','countResolutionPromote'))
        Buttons '服务档位与上下文计价' @(@('审阅服务档位与上下文计价','scopedPricingReview'),@('确认启用服务档位与上下文计价','scopedPricingPromote'))
        Buttons '分档管理计数记录' @(@('审阅分档管理计数记录','scopedCountReview'),@('确认启用分档管理计数记录','scopedCountPromote'))
        Buttons '分档决策与费用确认' @(@('审阅分档决策与费用确认','scopedExecutionReview'),@('确认启用分档决策与费用确认','scopedExecutionPromote'))
        Buttons 'API 逐轮计数与费用确认' @(@('审阅API 逐轮计数与费用确认','apiCurrencyReview'),@('确认启用API 逐轮计数与费用确认','apiCurrencyPromote'))
        Buttons 'API 未知费用核对' @(@('审阅API 未知费用核对','apiFeeResolutionReview'),@('确认启用API 未知费用核对','apiFeeResolutionPromote'))
        Buttons '恢复审阅' @(@('审阅原位置恢复','recoveryReview'),@('读取恢复事务','recoveryStatus'),@('诊断恢复中断','recoveryDiagnose'),@('核对恢复归档','recoveryInspect'))
        Buttons '旧锁定备份' @(@('审阅旧锁定备份恢复','recoveryPreviousReview'),@('准备旧备份恢复','recoveryPreviousPrepare'))
        Buttons '恢复事务' @(@('准备恢复副本','recoveryPrepare'),@('保留原目录并恢复','recoveryApply'),@('核验并结束恢复','recoveryFinish'),@('放弃未开始恢复','recoveryAbandon'))
        Buttons '其他与完成' @(@('仅恢复到空缺原位置','dataRestore'),@('明确解除数据锁定','dataRelease'))
        function ClearDataReviews {
            $script:lastFormatReview=$null;$migrationDigest.Text=''
            $script:lastReaderReview=$null;$readerDigest.Text=''
            $script:lastCurrencyReview=$null;$currencyDigest.Text=''
            $script:lastPreparationReview=$null;$preparationDigest.Text=''
            $script:lastCountingReview=$null;$countingDigest.Text=''
            $script:lastCountResolutionReview=$null;$countResolutionDigest.Text=''
            $script:lastScopedPricingReview=$null;$scopedPricingDigest.Text=''
            $script:lastScopedCountReview=$null;$scopedCountDigest.Text=''
            $script:lastScopedExecutionReview=$null;$scopedExecutionDigest.Text=''
            $script:lastApiCurrencyReview=$null;$apiCurrencyDigest.Text=''
            $script:lastApiFeeResolutionReview=$null;$apiFeeResolutionDigest.Text=''
            $script:lastRecoveryReview=$null;$script:lastRecoveryMode=$null
            $consent.Checked=$false
        }
        $inputs=@($backup,$backupDigest,$recoveryHold,$backupHold,$recoveryDigest,$consent)
        $backup.Add_TextChanged({ClearDataReviews})
        $backupDigest.Add_TextChanged({ClearDataReviews})
        $recoveryHold.Add_TextChanged({ClearDataReviews})
        $backupHold.Add_TextChanged({$script:lastRecoveryReview=$null;$script:lastRecoveryMode=$null;$consent.Checked=$false})
        $recoveryDigest.Add_TextChanged({$consent.Checked=$false})
    }else{
    $source=Field '原安装包目录';$candidate=Field '新安装包目录'
    Buttons '安装包信息' @(@('读取原安装包','sourceInfo'),@('读取新安装包','candidateInfo'))
    $internal=[Windows.Forms.CheckBox]::new();$internal.AutoSize=$true;$internal.Text='使用已核对来源的内部未签名包（不代表正式发布）';Row '包来源' $internal
    $publisher=Field '发布者证书指纹'
    $transaction=Field '事务目录' (Join-Path (Split-Path -Parent $install.Text) ('CeruleanMaintenance-'+[DateTime]::Now.ToString('yyyyMMdd-HHmmss')))
    $planDigest=Field '原事务计划摘要'
    $intentDigest=Field '原准备意图摘要'
    $guardDigest=Field '原维护占用摘要'
    $consent=[Windows.Forms.CheckBox]::new();$consent.AutoSize=$true;$consent.Text='已审阅当前状态、目录和包版本，确认执行所选变更';Row '操作确认' $consent
    Buttons '更新与恢复' @(@('准备备份和程序','Prepare'),@('读取事务','Review'),@('执行更新','Apply'),@('恢复原程序','Rollback'),@('确认完成升级','Commit'),@('核对提交结果','Reconcile'),@('放弃未完成提交','AbandonCommit'))
    Buttons '中断收尾' @(@('读取准备意图','intentInfo'),@('核验并结束维护','FinishMaintenance'),@('放弃未完成准备','AbandonPreparation'))
    Buttons '独立数据维护' (,@('打开数据迁移与恢复','openDataMaintenance'))
    $inputs=@($source,$candidate,$publisher,$transaction,$planDigest,$intentDigest,$guardDigest,$internal,$consent)
    $source.Add_TextChanged({$script:sourceInfo=$null;$consent.Checked=$false})
    $candidate.Add_TextChanged({$script:candidateInfo=$null;$consent.Checked=$false})
    $transaction.Add_TextChanged({$planDigest.Text='';$intentDigest.Text='';$guardDigest.Text='';$consent.Checked=$false})
    foreach($field in @($planDigest,$intentDigest,$guardDigest,$publisher)){$field.Add_TextChanged({$consent.Checked=$false})}
    $internal.Add_CheckedChanged({$consent.Checked=$false})
    }
    $status=[Windows.Forms.Label]::new();$status.AutoSize=$true;$status.Text='尚未执行操作。保留本次维护目录可重新打开，日志在其 operations 文件夹中。';Row '当前状态' $status
    $output=[Windows.Forms.TextBox]::new();$output.AccessibleName='审阅结果';$output.Multiline=$true;$output.ReadOnly=$true;$output.ScrollBars='Both';$output.WordWrap=$false;$output.Height=220;Row '审阅结果' $output
    $location=Field '维护副本位置' $directory $true

    function StartAction([string]$action){
        try{
            if($null -ne $script:operation){throw '当前操作尚未结束，不会重复发送'}
            if($ImportRetention -and $action -notin @('importStop','importBackup','importVerify','importReview','importRetain','importInspect')){throw '导入处置窗口不接受其他维护动作'}
            if($action -eq 'intentInfo'){
                $path=Join-Path (NormalPath $transaction.Text) 'intent.json'
                $pin=[CeruleanPackagePin]::Open($path,$false)
                try{
                    $value=(ReadBounded $path)|ConvertFrom-Json
                    if($value.schema -notin @(1,2) -or ($value.schema -eq 2 -and (($value.controllerReaderGeneration -isnot [int] -and $value.controllerReaderGeneration -isnot [long]) -or $value.controllerReaderGeneration -notin @(3,4,5,6,7,8,9,10,11,12,13,14,15,16))) -or $value.ownerSid -cne $config.ownerSid -or (NormalPath $value.transaction) -ine (NormalPath $transaction.Text) -or (NormalPath $value.data) -ine $data.Text -or (NormalPath $value.install) -ine $install.Text){throw '原准备意图与当前维护会话身份或读取代际不符'}
                    $intentDigest.Text=Digest $path
                    $output.Text=$value|ConvertTo-Json -Depth 15
                }finally{$pin.Dispose()}
                $status.Text='已读取原准备意图；未执行放弃或重新准备。请核对原路径、程序清单和摘要。';$consent.Checked=$false;return
            }
            if($action -eq 'openDataMaintenance'){
                $start=[Diagnostics.ProcessStartInfo]::new();$start.FileName=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
                $start.Arguments=(@('-NoProfile','-ExecutionPolicy','RemoteSigned','-WindowStyle','Hidden','-File',(Join-Path $directory 'maintenance-window.ps1'),'-Session',$Session,'-SessionDigest',$SessionDigest,'-DataMaintenance') | ForEach-Object {QuoteArgument ([string]$_)}) -join ' '
                $start.UseShellExecute=$false;$start.CreateNoWindow=$true
                $child=[Diagnostics.Process]::Start($start)
                try{$opened=@{pid=$child.Id;session=$Session;sessionDigest=$SessionDigest;mode='data-maintenance';readyConfirmed=$false;automaticDataActions=0}}finally{$child.Dispose()}
                [IO.File]::WriteAllText((Join-Path $directory ('data-window-'+[Guid]::NewGuid().ToString('N')+'.json')),($opened|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
                $output.Text=$opened|ConvertTo-Json;$status.Text='已启动数据维护窗口进程，请在新窗口继续；尚未执行数据操作。';return
            }
            if($action -in @('sourceInfo','candidateInfo')){
                $folder=if($action -eq 'sourceInfo'){$source.Text}else{$candidate.Text}
                $path=Join-Path (NormalPath $folder) 'manifest.json'
                $manifestPin=[CeruleanPackagePin]::Open($path,$false)
                try{
                    $raw=ReadBounded $path;$manifest=$raw | ConvertFrom-Json
                    $info=@{directory=(NormalPath $folder);digest=(Digest $path);version=$manifest.version;target=$manifest.target;claimedChannel=$manifest.channel;claimedRelease=$manifest.releaseAccepted}
                }finally{$manifestPin.Dispose()}
                if($action -eq 'sourceInfo'){$script:sourceInfo=$info}else{$script:candidateInfo=$info}
                $output.Text=$info|ConvertTo-Json -Depth 6;$status.Text='已读取清单。执行准备时还会校验实际文件、签名与程序能力。';$consent.Checked=$false;return
            }
            $mutating=$action -notin @('importVerify','importReview','importInspect','coreReview','coreStatus','Review','formatStatus','formatReview','readerReview','currencyReview','preparationReview','countingReview','countResolutionReview','scopedPricingReview','scopedCountReview','scopedExecutionReview','apiCurrencyReview','apiFeeResolutionReview','dataVerify','recoveryReview','recoveryPreviousReview','recoveryStatus','recoveryDiagnose','recoveryInspect')
            if($mutating -and -not $consent.Checked){throw '请先审阅信息并勾选本次操作确认'}
            if($mutating){$consent.Checked=$false}
            $arguments=@();$file='';$isCore=$action.StartsWith('core')
            if($action.StartsWith('import')){
                if(-not $ImportRetention){throw '请打开独立导入处置窗口'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text)
                if($action -eq 'importStop'){$arguments+=@('service','stop');ClearImportReview}
                elseif($action -eq 'importBackup'){
                    if(-not $backup.Text){throw '请填写新冷备份目录'}
                    $arguments+=@('backup','create','--to',(NormalPath $backup.Text));ClearImportReview
                }elseif($action -eq 'importVerify'){
                    if(-not $backup.Text){throw '请填写原冷备份目录'}
                    $arguments+=@('backup','verify','--from',(NormalPath $backup.Text))
                }else{
                    if(-not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$' -or -not $retentionDirectory.Text -or -not $retentionReason.Text.Trim()){throw '请核对原冷备份、摘要、决定目录和保留原因'}
                    $verb=switch($action){'importReview'{'review-retention'}'importRetain'{'retain'}'importInspect'{'inspect-retention'}}
                    $arguments+=@('team-import',$verb,'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--to',(NormalPath $retentionDirectory.Text),'--reason',$retentionReason.Text)
                    if($action -eq 'importRetain'){
                        if(-not $script:lastImportReview -or $retentionDigest.Text -cne $script:lastImportReview.digest){throw '请先审阅本次原导入保留；输入变化后需重新审阅'}
                        $script:lastImportReview=$null
                    }
                    if($action -in @('importRetain','importInspect')){
                        if($retentionDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请提供原保留审阅摘要'}
                        $arguments+=@('--review-digest',$retentionDigest.Text)
                    }
                }
            }elseif($action.StartsWith('recovery')){
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $verb=$action.Substring(8).ToLowerInvariant()
                if($action -eq 'recoveryPreviousReview'){$verb='review-previous'}
                if($action -eq 'recoveryPreviousPrepare'){$verb='prepare-previous'}
                $arguments=@('--workbench','--data-dir',$data.Text,'recovery',$verb)
                $previous=$action -in @('recoveryPreviousReview','recoveryPreviousPrepare')
                if($action -in @('recoveryReview','recoveryPrepare','recoveryPreviousReview','recoveryPreviousPrepare')){
                    if(-not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$' -or $recoveryHold.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请核对原备份目录、备份摘要及原锁定摘要'}
                    $arguments+=@('--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$recoveryHold.Text)
                    if($previous){
                        if($backupHold.Text -cnotmatch '^[a-f0-9]{64}$' -or $backupHold.Text -ceq $recoveryHold.Text){throw '旧备份恢复需要不同的当前锁定与备份原锁定摘要'}
                        $arguments+=@('--backup-hold-digest',$backupHold.Text)
                    }
                }
                if($action -in @('recoveryPrepare','recoveryPreviousPrepare','recoveryApply','recoveryFinish','recoveryAbandon','recoveryInspect')){
                    if($recoveryDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请提供明确的原恢复审阅摘要，不自动选择事务'}
                    if($action -in @('recoveryPrepare','recoveryPreviousPrepare') -and (-not $script:lastRecoveryReview -or $script:lastRecoveryReview.digest -cne $recoveryDigest.Text -or $script:lastRecoveryMode -cne $(if($previous){'previous'}else{'current'}))){throw '请先成功审阅对应类型的恢复计划'}
                    $arguments+=@('--review-digest',$recoveryDigest.Text)
                }
                if($action -in @('recoveryReview','recoveryPreviousReview')){$script:lastRecoveryReview=$null;$script:lastRecoveryMode=$null}
                if($mutating){ClearDataReviews;$script:lastHold=$null;$script:lastFormatStatus=$null}
            }elseif($action -in @('readerReview','readerPromote')){
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'readerReview'){'review-reader'}else{'promote-reader'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'readerPromote'){
                    if(-not $script:lastReaderReview -or $script:lastReaderReview.digest -cne $readerDigest.Text){throw '请先审阅启用监督配置的当前计划'}
                    $arguments+=@('--review-digest',$script:lastReaderReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastReaderReview=$null;$readerDigest.Text=''}
            }elseif($action -in @('currencyReview','currencyPromote')){
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'currencyReview'){'review-currency'}else{'promote-currency'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'currencyPromote'){
                    if(-not $script:lastCurrencyReview -or $script:lastCurrencyReview.digest -cne $currencyDigest.Text){throw '请先审阅预算格式升级数据格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastCurrencyReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastCurrencyReview=$null;$currencyDigest.Text=''}
            }elseif($action -in @('preparationReview','preparationPromote')){
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'preparationReview'){'review-preparation'}else{'promote-preparation'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'preparationPromote'){
                    if(-not $script:lastPreparationReview -or $script:lastPreparationReview.digest -cne $preparationDigest.Text){throw '请先审阅预算执行准备格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastPreparationReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastPreparationReview=$null;$preparationDigest.Text=''}
            }elseif($action -in @('countingReview','countingPromote')){
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'countingReview'){'review-counting'}else{'promote-counting'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'countingPromote'){
                    if(-not $script:lastCountingReview -or $script:lastCountingReview.digest -cne $countingDigest.Text){throw '请先审阅计数费用记录格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastCountingReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastCountingReview=$null;$countingDigest.Text=''}
            }elseif($action -in @('countResolutionReview','countResolutionPromote')){
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'countResolutionReview'){'review-count-resolution'}else{'promote-count-resolution'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'countResolutionPromote'){
                    if(-not $script:lastCountResolutionReview -or $script:lastCountResolutionReview.digest -cne $countResolutionDigest.Text){throw '请先审阅计数核销与终止记录格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastCountResolutionReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastCountResolutionReview=$null;$countResolutionDigest.Text=''}
            }elseif($action -in @('scopedPricingReview','scopedPricingPromote')){
                # reader8 enables storage only; this path never approves a budget or releases the hold.
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'scopedPricingReview'){'review-scoped-pricing'}else{'promote-scoped-pricing'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'scopedPricingPromote'){
                    if(-not $script:lastScopedPricingReview -or $script:lastScopedPricingReview.digest -cne $scopedPricingDigest.Text){throw '请先审阅服务档位与上下文计价格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastScopedPricingReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastScopedPricingReview=$null;$scopedPricingDigest.Text=''}
            }elseif($action -in @('scopedCountReview','scopedCountPromote')){
                # reader9 enables storage only; this path never approves a budget or releases the hold.
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'scopedCountReview'){'review-scoped-count'}else{'promote-scoped-count'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'scopedCountPromote'){
                    if(-not $script:lastScopedCountReview -or $script:lastScopedCountReview.digest -cne $scopedCountDigest.Text){throw '请先审阅分档管理计数记录格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastScopedCountReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastScopedCountReview=$null;$scopedCountDigest.Text=''}
            }elseif($action -in @('scopedExecutionReview','scopedExecutionPromote')){
                # reader10 enables storage only; this path never approves a budget or releases the hold.
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'scopedExecutionReview'){'review-scoped-execution'}else{'promote-scoped-execution'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'scopedExecutionPromote'){
                    if(-not $script:lastScopedExecutionReview -or $script:lastScopedExecutionReview.digest -cne $scopedExecutionDigest.Text){throw '请先审阅分档决策与费用确认格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastScopedExecutionReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastScopedExecutionReview=$null;$scopedExecutionDigest.Text=''}
            }elseif($action -in @('apiCurrencyReview','apiCurrencyPromote')){
                # reader11 enables storage only; this path never approves a budget or releases the hold.
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'apiCurrencyReview'){'review-api-currency'}else{'promote-api-currency'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'apiCurrencyPromote'){
                    if(-not $script:lastApiCurrencyReview -or $script:lastApiCurrencyReview.digest -cne $apiCurrencyDigest.Text){throw '请先审阅API 逐轮计数与费用确认格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastApiCurrencyReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastApiCurrencyReview=$null;$apiCurrencyDigest.Text=''}
            }elseif($action -in @('apiFeeResolutionReview','apiFeeResolutionPromote')){
                # reader12 enables storage only; this path never approves a budget or releases the hold.
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                if(-not $script:lastHold.validRecord -or -not $backup.Text -or $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先读取有效核心锁定并核对冷备份摘要'}
                $file=Join-Path $directory 'cerulean-runtime.exe'
                $arguments=@('--workbench','--data-dir',$data.Text,'data-format',$(if($action -eq 'apiFeeResolutionReview'){'review-api-fee-resolution'}else{'promote-api-fee-resolution'}),'--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                if($action -eq 'apiFeeResolutionPromote'){
                    if(-not $script:lastApiFeeResolutionReview -or $script:lastApiFeeResolutionReview.digest -cne $apiFeeResolutionDigest.Text){throw '请先审阅API 未知费用核对格式的当前计划'}
                    $arguments+=@('--review-digest',$script:lastApiFeeResolutionReview.digest)
                    ClearDataReviews;$script:lastFormatStatus=$null;$script:lastHold=$null
                }else{$script:lastApiFeeResolutionReview=$null;$apiFeeResolutionDigest.Text=''}
            }elseif($action -in @('dataBackup','dataVerify','dataRestore','dataRelease','formatStatus','formatReview','formatMigrate')){
                if(-not $DataMaintenance){throw '请打开独立数据维护窗口'}
                $file=Join-Path $directory 'cerulean-runtime.exe';$arguments=@('--workbench','--data-dir',$data.Text)
                if($action -in @('dataBackup','formatReview','formatMigrate','dataRelease') -and -not $script:lastHold.validRecord){throw '请先读取有效的现有核心锁定'}
                if($action -in @('dataBackup','dataVerify','dataRestore','formatReview','formatMigrate') -and -not $backup.Text){throw '请指定已审阅的数据备份目录'}
                if($action -in @('dataRestore','formatReview','formatMigrate') -and $backupDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请先核对备份文件并取得摘要'}
                switch($action){
                    'formatStatus'{$script:lastFormatStatus=$null;$arguments+=@('data-format','status')}
                    'dataBackup'{$arguments+=@('upgrade','backup','--hold-digest',$script:lastHold.holdDigest,'--to',(NormalPath $backup.Text))}
                    'dataVerify'{$arguments+=@('backup','verify','--from',(NormalPath $backup.Text))}
                    'dataRestore'{$script:lastHold=$null;$script:lastFormatStatus=$null;ClearDataReviews;$arguments+=@('backup','restore','--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text)}
                    'dataRelease'{
                        if(-not $script:lastFormatStatus){throw '请先成功读取当前数据格式，再明确解除锁定'}
                        $arguments+=@('upgrade','release','--hold-digest',$script:lastHold.holdDigest);$script:lastHold=$null;$script:lastFormatStatus=$null
                        ClearDataReviews
                    }
                    default{
                        if($action -eq 'formatMigrate'){
                            if(-not $script:lastFormatReview -or $script:lastFormatReview.digest -cne $migrationDigest.Text){throw '请先成功审阅本次数据迁移'}
                            $arguments+=@('data-format','migrate','--review-digest',$script:lastFormatReview.digest)
                            ClearDataReviews;$script:lastFormatStatus=$null
                        }else{$script:lastFormatReview=$null;$migrationDigest.Text='';$arguments+=@('data-format','review')}
                        $arguments+=@('--from',(NormalPath $backup.Text),'--manifest-digest',$backupDigest.Text,'--hold-digest',$script:lastHold.holdDigest)
                    }
                }
            }elseif($isCore){
                $file=Join-Path $directory 'cerulean-runtime.exe';$arguments=@('--workbench','--data-dir',$data.Text,'upgrade')
                if($action -eq 'coreReview'){$arguments+='review'}elseif($action -eq 'coreStatus'){$arguments+='status'}else{
                    if(-not $script:lastReview -or -not $script:lastReview.ready){throw '请先取得可升级的当前审阅'}
                    $arguments+=@('prepare','--review-digest',$script:lastReview.digest)
                    $script:lastReview=$null
                }
                if($action -in @('corePrepare','coreStatus')){$script:lastHold=$null;if($DataMaintenance){ClearDataReviews}}
            }else{
                $file=Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
                $arguments=@('-NoProfile','-ExecutionPolicy','RemoteSigned','-File',(Join-Path $directory 'manage-windows-update.ps1'),'-Action',$action,'-TransactionDirectory',(NormalPath $transaction.Text))
                if($action -eq 'Prepare'){
                    if(-not $script:sourceInfo -or -not $script:candidateInfo -or -not $script:lastHold.validRecord){throw '请先读取两份安装包信息和有效核心锁定'}
                    $arguments+=@('-InstalledDirectory',$install.Text,'-DataDirectory',$data.Text,'-HoldDigest',$script:lastHold.holdDigest,'-SourcePackage',$script:sourceInfo.directory,'-SourceManifestSha256',$script:sourceInfo.digest,'-CandidatePackage',$script:candidateInfo.directory,'-CandidateManifestSha256',$script:candidateInfo.digest)
                }elseif($action -eq 'AbandonPreparation'){
                    if($intentDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请读取并核对原准备意图摘要'}
                    $arguments+=@('-IntentDigest',$intentDigest.Text)
                }else{
                    if($planDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '请填入原事务计划摘要，不能推测或自动选择'}
                    $arguments+=@('-PlanDigest',$planDigest.Text)
                }
                if($internal.Checked){$arguments+='-AllowUnsignedInternal'}elseif($publisher.Text){if($publisher.Text -notmatch '^[a-fA-F0-9]{40}$'){throw '发布者证书指纹格式无效'};$arguments+=@('-PublisherThumbprint',$publisher.Text)}
                if($guardDigest.Text){if($guardDigest.Text -cnotmatch '^[a-f0-9]{64}$'){throw '原维护占用摘要格式无效'};$arguments+=@('-GuardDigest',$guardDigest.Text)}
            }
            $logRoot=Join-Path $directory 'operations';if(-not (Test-Path -LiteralPath $logRoot)){$null=New-Item -ItemType Directory -Path $logRoot}
            $id=[Guid]::NewGuid().ToString('N');$out=Join-Path $logRoot ($id+'.stdout');$err=Join-Path $logRoot ($id+'.stderr')
            $request=@{action=$action;at=[DateTimeOffset]::Now.ToString('o');transaction=$transaction.Text;planDigest=$planDigest.Text;intentDigest=$intentDigest.Text;guardDigest=$guardDigest.Text;phase='requested'}
            if($DataMaintenance){$request=@{action=$action;at=[DateTimeOffset]::Now.ToString('o');data=$data.Text;backup=$backup.Text;backupDigest=$backupDigest.Text;recoveryDigest=$recoveryDigest.Text;recoveryHold=$recoveryHold.Text;backupHold=$backupHold.Text;phase='requested';arguments=$arguments}}
            if($ImportRetention){$request.retentionDirectory=$retentionDirectory.Text;$request.retentionReason=$retentionReason.Text;$request.retentionDigest=$retentionDigest.Text}
            [IO.File]::WriteAllText((Join-Path $logRoot ($id+'.request.json')),($request|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
            $quoted=@($arguments | ForEach-Object {QuoteArgument ([string]$_)}) -join ' '
            $capture=[CeruleanMaintenanceOperation]::Start($file,$quoted,$out,$err)
            $process=$capture.Process
            $handle=$process.Handle
            $script:operation=@{process=$process;drain=$capture.Completion;action=$action;stdout=$out;stderr=$err;id=$id;logRoot=$logRoot;handle=$handle}
            foreach($button in $actionButtons){$button.Enabled=$false};foreach($inputField in $inputs){$inputField.Enabled=$false};$consent.Checked=$false;$status.Text='操作进行中；不会因等待时间较长而重复执行。';$timer.Start()
        }catch{$status.Text='未执行或结果需核对：'+$_.Exception.Message}
    }
    $timer=[Windows.Forms.Timer]::new();$timer.Interval=250
    $timer.Add_Tick({
        if(-not $script:operation){return}
        $op=$script:operation;$op.process.Refresh();if(-not $op.process.HasExited){return}
        # Completion includes the original process and both owned log writers.
        # An inherited child pipe can outlive its controller: retain the operation
        # and disabled controls until EOF and disposal, without blocking the UI.
        if(-not $op.drain.IsCompleted){return}
        $timer.Stop()
        try{
            $op.drain.GetAwaiter().GetResult()
            $record=@{action=$op.action;exit=$op.process.ExitCode;stdout=$op.stdout;stderr=$op.stderr;phase='finished'}
            [IO.File]::WriteAllText((Join-Path $op.logRoot ($op.id+'.finished.json')),($record|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
            $text=ReadBounded $op.stdout;$errorText=ReadBounded $op.stderr
            if($op.process.ExitCode -ne 0){$output.Text=$errorText+"`r`n"+$text;throw '操作未通过，原日志保留；先读取现有状态再决定下一步'}
            $result=$text | ConvertFrom-Json;$output.Text=$result|ConvertTo-Json -Depth 15
            if($op.action -eq 'coreReview'){$script:lastReview=$result.upgradeReview}
            if($op.action -eq 'corePrepare'){$script:lastHold=$result.upgradeHold;$script:lastReview=$null}
            if($op.action -eq 'coreStatus'){$script:lastHold=$result.result}
            if($op.action -eq 'Prepare'){$planDigest.Text=$result.planDigest;$intentDigest.Text=$result.intentDigest}
            if(-not $DataMaintenance -and $result.maintenance.guardDigest){$guardDigest.Text=$result.maintenance.guardDigest}
            if($DataMaintenance -and $op.action -in @('coreStatus','corePrepare') -and $script:lastHold.validRecord){$recoveryHold.Text=$script:lastHold.holdDigest}
            if($op.action -in @('recoveryReview','recoveryPreviousReview')){$recoveryDigest.Text=$result.result.digest;$script:lastRecoveryReview=$result.result;$script:lastRecoveryMode=if($op.action -eq 'recoveryPreviousReview'){'previous'}else{'current'}}
            if($op.action -eq 'readerReview'){$script:lastReaderReview=$result.result;$readerDigest.Text=$result.result.digest}
            if($op.action -eq 'currencyReview'){$script:lastCurrencyReview=$result.result;$currencyDigest.Text=$result.result.digest}
            if($op.action -eq 'preparationReview'){$script:lastPreparationReview=$result.result;$preparationDigest.Text=$result.result.digest}
            if($op.action -eq 'countingReview'){$script:lastCountingReview=$result.result;$countingDigest.Text=$result.result.digest}
            if($op.action -eq 'countResolutionReview'){$script:lastCountResolutionReview=$result.result;$countResolutionDigest.Text=$result.result.digest}
            if($op.action -eq 'scopedPricingReview'){$script:lastScopedPricingReview=$result.result;$scopedPricingDigest.Text=$result.result.digest}
            if($op.action -eq 'scopedCountReview'){$script:lastScopedCountReview=$result.result;$scopedCountDigest.Text=$result.result.digest}
            if($op.action -eq 'scopedExecutionReview'){$script:lastScopedExecutionReview=$result.result;$scopedExecutionDigest.Text=$result.result.digest}
            if($op.action -eq 'apiCurrencyReview'){$script:lastApiCurrencyReview=$result.result;$apiCurrencyDigest.Text=$result.result.digest}
            if($op.action -eq 'apiFeeResolutionReview'){$script:lastApiFeeResolutionReview=$result.result;$apiFeeResolutionDigest.Text=$result.result.digest}
            if($op.action -eq 'importReview'){$retentionDigest.Text=$result.result.digest;$script:lastImportReview=$result.result}
            if($op.action -in @('importBackup','importVerify')){$backupDigest.Text=$result.result.manifestDigest}
            if($op.action -in @('dataBackup','dataVerify')){$backupDigest.Text=$result.result.manifestDigest}
            if($op.action -eq 'formatReview'){$script:lastFormatReview=$result.result;$migrationDigest.Text=$result.result.digest}
            if($op.action -eq 'formatStatus'){$script:lastFormatStatus=$result.result}
            $status.Text='操作完成，请审阅结果。执行更新或回滚前，关闭所有原工作台窗口。'
            if($DataMaintenance){$status.Text='操作完成，请审阅结果。恢复事务完成后仍须核验收尾，再读取格式和锁定并明确解除。归档核查只证明历史证据，不代表当前数据未变化；不会自动启动核心。'}
            if($ImportRetention){$status.Text='原操作已返回，请核对结果。保留决定不改变原导入或解除只读；需要继续其他工作时另建独立实验。结果不确定时仅核查原决定。'}
        }catch{if($ImportRetention){$script:lastImportReview=$null};$status.Text=$_.Exception.Message}
        finally{$op.process.Dispose();$script:operation=$null;foreach($button in $actionButtons){$button.Enabled=$true};foreach($inputField in $inputs){$inputField.Enabled=$true}}
    })
    $form.Add_FormClosing({param($sender,$eventArgs) if($null -ne $script:operation){$eventArgs.Cancel=$true;$status.Text='当前操作仍在运行，请等待结果；不会关闭或重复该进程。'}})
    if($Describe){
        $gates=0
        foreach($button in $actionButtons){
            if($button.Tag -in @('importStop','importBackup','importRetain','corePrepare','Prepare','Apply','Rollback','Commit','Reconcile','AbandonCommit','FinishMaintenance','AbandonPreparation','dataBackup','dataRestore','dataRelease','formatMigrate','readerPromote','currencyPromote','preparationPromote','countingPromote','countResolutionPromote','scopedPricingPromote','scopedCountPromote','scopedExecutionPromote','apiCurrencyPromote','apiFeeResolutionPromote','recoveryPreviousPrepare','recoveryPrepare','recoveryApply','recoveryFinish','recoveryAbandon')){
                $method=$button.GetType().GetMethod('OnClick',([Reflection.BindingFlags]::Instance -bor [Reflection.BindingFlags]::NonPublic))
                $null=$method.Invoke($button,@([EventArgs]::Empty))
                if($status.Text -notlike '*勾选本次操作确认*'){throw '操作确认事件校验未通过'}
                $gates++
            }
        }
        if(Test-Path -LiteralPath (Join-Path $directory 'operations')){throw '界面结构检查触发了外部操作'}
        $status.Text='尚未执行操作。保留本次维护目录可重新打开，日志在其 operations 文件夹中。'
        $form.PreviewMode=$true;$form.ShowInTaskbar=$false;$form.Opacity=0
        $form.Show();[Windows.Forms.Application]::DoEvents();$form.PerformLayout()
        $preview=Join-Path $directory 'maintenance-preview.png'
        $bitmap=[Drawing.Bitmap]::new($form.Width,$form.Height)
        try{$form.DrawToBitmap($bitmap,[Drawing.Rectangle]::new(0,0,$form.Width,$form.Height));$bitmap.Save($preview,[Drawing.Imaging.ImageFormat]::Png)}finally{$bitmap.Dispose()}
        $form.Hide()
        @{version=$config.version;schema=1;dataMaintenance=[bool]$DataMaintenance;importRetention=[bool]$ImportRetention;actions=@($actionButtons | ForEach-Object {$_.Tag});installation=$install.Text;data=$data.Text;automaticActions=0;bundleVerified=$true;visible=$false;confirmationGateCases=$gates;preview=$preview}|ConvertTo-Json -Compress
    }else{$null=$form.ShowDialog()}
    $timer.Dispose();$form.Dispose()
}catch{
    [IO.File]::WriteAllText((Join-Path $directory 'startup-error.txt'),$_.Exception.Message,[Text.UTF8Encoding]::new($false))
    if($Describe){throw}else{[Windows.Forms.MessageBox]::Show($_.Exception.Message,'维护窗口未打开')|Out-Null}
}finally{for($i=$pins.Count-1;$i -ge 0;$i--){$pins[$i].Dispose()}}
