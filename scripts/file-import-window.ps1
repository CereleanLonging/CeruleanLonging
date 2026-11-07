param([Parameter(Mandatory=$true)][string]$PlanPath)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
[Windows.Forms.Application]::EnableVisualStyles()
$plan = [IO.File]::ReadAllText($PlanPath) | ConvertFrom-Json
if ($plan.session -notmatch '^[a-f0-9]{48}$') { throw 'Invalid import session' }
$directory = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($PlanPath))
if ([IO.Path]::GetFileName($directory) -cne $plan.session) { throw 'Import directory mismatch' }
$parent = [Diagnostics.Process]::GetProcessById([int]$plan.parentPid)
$null = $parent.Handle
$script:busy = $false
$script:published = $false
$script:cancelled = $false

function Test-ImportClosing {
    $parent.Refresh()
    return $script:cancelled -or $parent.HasExited -or
        [IO.File]::Exists((Join-Path $directory 'cancel.signal')) -or
        [IO.File]::Exists((Join-Path $directory 'finish.signal'))
}

$form = New-Object Windows.Forms.Form
$form.Text = 'CeruleanLonging · 文件导入（普通权限）'
$form.ClientSize = New-Object Drawing.Size(520,260)
$form.MinimumSize = New-Object Drawing.Size(480,290)
$form.StartPosition = 'CenterScreen'
$form.Font = New-Object Drawing.Font('Microsoft YaHei UI',10)
$form.AllowDrop = $true

$layout = New-Object Windows.Forms.TableLayoutPanel
$layout.Dock = 'Fill'
$layout.Padding = New-Object Windows.Forms.Padding(20)
$layout.ColumnCount = 1
$layout.RowCount = 3
$null = $layout.RowStyles.Add((New-Object Windows.Forms.RowStyle('Percent',100)))
$null = $layout.RowStyles.Add((New-Object Windows.Forms.RowStyle('Absolute',48)))
$null = $layout.RowStyles.Add((New-Object Windows.Forms.RowStyle('Absolute',48)))
$drop = New-Object Windows.Forms.Label
$drop.Text = "将一个文件拖入这里`r`n支持 Markdown（.md）和工作流（.json）"
$drop.TextAlign = 'MiddleCenter'
$drop.Dock = 'Fill'
$drop.BorderStyle = 'FixedSingle'
$drop.AllowDrop = $true
$status = New-Object Windows.Forms.Label
$status.Text = '文件读入后，请回主窗口预览并确认。'
$status.Dock = 'Fill'
$status.TextAlign = 'MiddleLeft'
$buttons = New-Object Windows.Forms.FlowLayoutPanel
$buttons.Dock = 'Fill'
$buttons.FlowDirection = 'RightToLeft'
$cancel = New-Object Windows.Forms.Button
$cancel.Text = '取消'
$cancel.Size = New-Object Drawing.Size(100,34)
$choose = New-Object Windows.Forms.Button
$choose.Text = '选择文件'
$choose.Size = New-Object Drawing.Size(120,34)
$buttons.Controls.Add($cancel)
$buttons.Controls.Add($choose)
$layout.Controls.Add($drop,0,0)
$layout.Controls.Add($status,0,1)
$layout.Controls.Add($buttons,0,2)
$form.Controls.Add($layout)

function Import-SelectedFile([string[]]$paths) {
    if ($script:busy -or $script:published) { return }
    if ($paths.Count -ne 1 -or -not [IO.File]::Exists($paths[0])) {
        $status.Text = '请一次选择一个文件，文件夹无法导入。'
        return
    }
    $sourcePath = $paths[0]
    $extension = [IO.Path]::GetExtension($sourcePath).ToLowerInvariant()
    if ($extension -notin @('.md','.json')) {
        $status.Text = '仅支持 .md 文档和工作流 .json 文件。'
        return
    }
    $script:busy = $true
    $choose.Enabled = $false
    $status.Text = '正在读取文件…'
    $source = $null
    $target = $null
    $digest = $null
    try {
        $source = [IO.File]::Open($sourcePath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
        if ($extension -eq '.json' -and $source.Length -gt 65536) { throw '工作流模板不得超过64 KiB。' }
        $target = [IO.File]::Open((Join-Path $directory 'input.part'),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        $digest = [Security.Cryptography.SHA256]::Create()
        $buffer = New-Object byte[] 65536
        $length = [long]0
        while (($count = $source.Read($buffer,0,$buffer.Length)) -gt 0) {
            if (Test-ImportClosing) { throw '已取消文件导入。' }
            $target.Write($buffer,0,$count)
            $null = $digest.TransformBlock($buffer,0,$count,$buffer,0)
            $length += $count
            [Windows.Forms.Application]::DoEvents()
        }
        if (Test-ImportClosing) { throw '已取消文件导入。' }
        $null = $digest.TransformFinalBlock((New-Object byte[] 0),0,0)
        $hash = [BitConverter]::ToString($digest.Hash).Replace('-','').ToLowerInvariant()
        $target.Flush($true)
        $target.Dispose()
        $target = $null
        [IO.File]::Move((Join-Path $directory 'input.part'),(Join-Path $directory 'selected.bin'))
        $receipt = [ordered]@{ session=[string]$plan.session; name=[IO.Path]::GetFileName($sourcePath); bytes=$length; sha256=$hash }
        $utf8 = New-Object Text.UTF8Encoding($false)
        $receiptBytes = $utf8.GetBytes(($receipt | ConvertTo-Json -Compress))
        $receiptFile = [IO.File]::Open((Join-Path $directory 'receipt.part'),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        try { $receiptFile.Write($receiptBytes,0,$receiptBytes.Length); $receiptFile.Flush($true) }
        finally { $receiptFile.Dispose() }
        [IO.File]::Move((Join-Path $directory 'receipt.part'),(Join-Path $directory 'receipt.json'))
        $script:published = $true
        $status.Text = '正在交回主窗口，校验完成后此窗口会自动关闭。'
    } catch {
        $status.Text = $_.Exception.Message
        # A partially published handoff is closed, never reused for another file.
        if ([IO.File]::Exists((Join-Path $directory 'selected.bin'))) { $script:cancelled = $true }
    } finally {
        if ($source) { $source.Dispose() }
        if ($target) { $target.Dispose() }
        if ($digest) { $digest.Dispose() }
        if (-not $script:published) { [IO.File]::Delete((Join-Path $directory 'input.part')) }
        $script:busy = $false
        $choose.Enabled = -not $script:published
    }
}

$choose.Add_Click({
    $dialog = New-Object Windows.Forms.OpenFileDialog
    $dialog.Title = '选择要导入的 Markdown 或工作流'
    $dialog.Filter = 'Markdown 或工作流 (*.md;*.json)|*.md;*.json'
    $dialog.Multiselect = $false
    try {
        if ($dialog.ShowDialog($form) -eq [Windows.Forms.DialogResult]::OK) {
            Import-SelectedFile @($dialog.FileName)
        }
    } finally { $dialog.Dispose() }
})
$dragEnter = {
    param($sender,$eventArgs)
    if (-not $script:busy -and -not $script:published -and $eventArgs.Data.GetDataPresent([Windows.Forms.DataFormats]::FileDrop)) {
        $eventArgs.Effect = [Windows.Forms.DragDropEffects]::Copy
    } else { $eventArgs.Effect = [Windows.Forms.DragDropEffects]::None }
}
$dragDrop = {
    param($sender,$eventArgs)
    if ($eventArgs.Data.GetDataPresent([Windows.Forms.DataFormats]::FileDrop)) {
        Import-SelectedFile @($eventArgs.Data.GetData([Windows.Forms.DataFormats]::FileDrop))
    }
}
$form.Add_DragEnter($dragEnter)
$form.Add_DragDrop($dragDrop)
$drop.Add_DragEnter($dragEnter)
$drop.Add_DragDrop($dragDrop)
$cancel.Add_Click({ $script:cancelled = $true; $form.Close() })
$form.Add_FormClosing({
    param($sender,$eventArgs)
    $script:cancelled = $true
    if (-not [IO.File]::Exists((Join-Path $directory 'finish.signal'))) {
        try {
            $signal = [IO.File]::Open((Join-Path $directory 'cancel.signal'),[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::Read)
            $signal.Dispose()
        } catch [IO.IOException] { }
    }
    if ($script:busy) { $eventArgs.Cancel = $true }
})
$timer = New-Object Windows.Forms.Timer
$timer.Interval = 200
$timer.Add_Tick({ if ((Test-ImportClosing) -and -not $script:busy) { $form.Close() } })
$timer.Start()
try { [Windows.Forms.Application]::Run($form) }
finally {
    $timer.Stop()
    $timer.Dispose()
    $form.Dispose()
    # If the host exits unexpectedly, collect only this helper's fixed files.
    if ($parent.HasExited) {
        foreach ($name in @('input.part','selected.bin','receipt.part','receipt.json','cancel.signal','finish.signal','plan.json','window.ps1')) {
            try { [IO.File]::Delete((Join-Path $directory $name)) } catch { }
        }
        try { [IO.Directory]::Delete($directory,$false) } catch { }
    }
    $parent.Dispose()
}
