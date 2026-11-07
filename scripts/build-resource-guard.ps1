param([Parameter(Mandatory=$true)][string]$Configuration)
$ErrorActionPreference='Stop'
[Console]::OutputEncoding=[Text.UTF8Encoding]::new($false)
$OutputEncoding=[Console]::OutputEncoding
$config=Get-Content -LiteralPath $Configuration -Raw -Encoding UTF8|ConvertFrom-Json
Add-Type -Path (Join-Path $PSScriptRoot 'build-resource-job.cs')
$mutex=[Threading.Mutex]::new($false,('Local\CeruleanBuild-'+$config.workspaceId))
$acquired=$false
$receipt=[ordered]@{state='waiting';policy=$null;started=$false;exitCode=$null;peakJobMemoryBytes=$null}
function Save-Receipt { [IO.File]::WriteAllText($config.receipt,($receipt|ConvertTo-Json -Depth 5)) }
try {
    try{$acquired=$mutex.WaitOne(0)}catch [Threading.AbandonedMutexException]{$acquired=$true}
    if(-not $acquired){$receipt.state='deferred-concurrent-build';$receipt.exitCode=75;Save-Receipt;Write-Host '已有构建正在运行，本次暂不启动。';exit 75}
    $memory=[CeruleanBuildJob]::ReadMemory()
    $budget=[CeruleanBuildJob]::Budget($memory.AvailablePhysical,$memory.AvailablePageFile,[uint64]$config.minimumMemoryBytes)
    $receipt.availablePhysicalBytes=$memory.AvailablePhysical
    $receipt.availableCommitBytes=$memory.AvailablePageFile
    $receipt.minimumBuildMemoryBytes=$config.minimumMemoryBytes
    $receipt.admissionReferenceBytes=$config.minimumMemoryBytes+1GB
    $receipt.admissionToleranceBytes=1GB
    $receipt.requiredAvailableBytes=$config.minimumMemoryBytes
    $receipt.observedMinimumPhysicalBytes=$memory.AvailablePhysical
    $receipt.observedMinimumCommitBytes=$memory.AvailablePageFile
    if(-not $budget){$receipt.state='deferred-low-memory';$receipt.exitCode=75;Save-Receipt;Write-Host ('可用物理内存或提交空间低于 '+($receipt.requiredAvailableBytes/1GB)+' GiB，已超出授权的 1 GiB 启动差额，本次暂不启动。');exit 75}
    $receipt.policy=[CeruleanBuildJob]::Configure($budget)
    $env:CARGO_BUILD_JOBS='1'
    $env:RAYON_NUM_THREADS='2'
    $env:UV_THREADPOOL_SIZE='2'
    $env:CERULEAN_BUILD_SCHEDULE_ACTIVE='1'
    $receipt.state='running';$receipt.started=$true;Save-Receipt
    Write-Host ('按构建类型最低预算启动：'+[math]::Round($budget/1GB,2)+' GiB，系统余量 1 GiB；Cargo 单任务、最多 2 个逻辑核心、CPU 上限 20%、BelowNormal。记录本次真实峰值，不随空闲内存增加额度。')
    $child=[CeruleanBuildJob]::Start($config.program,[string[]]$config.arguments,$config.directory)
    while(-not $child.WaitForExit(1000)){
        $memory=[CeruleanBuildJob]::ReadMemory()
        $receipt.observedMinimumPhysicalBytes=[math]::Min($receipt.observedMinimumPhysicalBytes,$memory.AvailablePhysical)
        $receipt.observedMinimumCommitBytes=[math]::Min($receipt.observedMinimumCommitBytes,$memory.AvailablePageFile)
        if($memory.AvailablePhysical -lt 1GB -or $memory.AvailablePageFile -lt 1GB){
            $receipt.peakJobMemoryBytes=[CeruleanBuildJob]::PeakMemoryBytes()
            $receipt.state='stopped-low-memory';$receipt.exitCode=75;Save-Receipt
            Write-Host '系统可用物理内存或提交空间低于 1 GiB，停止本次构建进程组；不自动重试或放大额度。'
            [CeruleanBuildJob]::Stop()
        }
    }
    $child.WaitForExit()
    $receipt.peakJobMemoryBytes=[CeruleanBuildJob]::PeakMemoryBytes()
    $receipt.state=if($child.ExitCode -eq 0){'completed'}else{'failed'}
    $receipt.exitCode=$child.ExitCode;Save-Receipt
    exit $child.ExitCode
}catch{$receipt.state='guard-error';$receipt.error=$_.Exception.Message;Save-Receipt;throw}
finally{if($acquired){$mutex.ReleaseMutex()};$mutex.Dispose()}
