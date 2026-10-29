# Metadata only. A successful check does not authenticate or decrypt a credential.
function Assert-OmpRuntimeParent([string]$Path) {
    if (-not [IO.Directory]::Exists($Path)) {
        throw 'OMP runtime parent directory is missing. Create the isolated directory before Prepare; no test process was started.'
    }
}
function Assert-OmpCredentialSource([string]$Path, [string]$PreparedDirectory) {
    if ([string]::IsNullOrWhiteSpace($Path) -or -not [IO.Directory]::Exists($Path)) {
        throw 'Authorized credential source directory is missing. Supply the actual workbench data directory; no test process was started and no key was read.'
    }
    $source=[IO.Path]::GetFullPath($Path).TrimEnd('\','/')
    $isolated=[IO.Path]::GetFullPath((Join-Path $PreparedDirectory 'data')).TrimEnd('\','/')
    if ([string]::Equals($source,$isolated,[StringComparison]::OrdinalIgnoreCase)) {
        throw 'Credential source must differ from this probe isolated data directory.'
    }
    $credential=Join-Path $source 'credentials/deepseek.dpapi'
    if (-not [IO.File]::Exists($credential)) {
        throw 'The authorized source has no DeepSeek workbench credential file. Configure it through the workbench; no key was read.'
    }
    $info=Get-Item -LiteralPath $credential -Force -ErrorAction Stop
    if ($info.Length -eq 0 -or $info.Length -gt 65536) {
        throw 'DeepSeek credential file has an invalid envelope size. No key was read.'
    }
    # Reject links rather than inspecting another source selected by indirection.
    $probe=$credential
    while ($probe) {
        $item=Get-Item -LiteralPath $probe -Force -ErrorAction Stop
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Credential source contains a linked path. Use the actual explicitly authorized workbench directory.'
        }
        $probe=[IO.Path]::GetDirectoryName($probe)
    }
}
