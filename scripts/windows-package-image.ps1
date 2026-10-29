# Tauri 2.12.1 patches this unique marker for NSIS and restores the build input
# afterwards. Derive the exact unsigned payload, then compare every byte by hash.
function Get-NsisImageBytes([string]$Path) {
    if((Get-AuthenticodeSignature -LiteralPath $Path).Status -ne 'NotSigned') {
        throw 'Signed image preparation requires a signing-aware packaging path; do not alter it'
    }
    $image=[IO.File]::ReadAllBytes($Path)
    $text=[Text.Encoding]::ASCII.GetString($image)
    $marker='TAURI_BUNDLE_TYPE_VAR_'
    $position=$text.IndexOf($marker,[StringComparison]::Ordinal)
    if($position -lt 0 -or $text.IndexOf($marker,$position+1,[StringComparison]::Ordinal) -ge 0) { throw 'Bundle marker changed or is ambiguous; re-review the Tauri integration' }
    $offset=$position+$marker.Length
    $kind=$text.Substring($offset,3)
    if($kind -ne 'UNK' -and $kind -ne 'NSS'){throw 'Unexpected bundle type'}
    [Text.Encoding]::ASCII.GetBytes('NSS').CopyTo($image,$offset)
    return ,$image
}
function Get-NsisImageHash([string]$Path) {
    $sha=[Security.Cryptography.SHA256]::Create()
    try{return [BitConverter]::ToString($sha.ComputeHash((Get-NsisImageBytes $Path))).Replace('-','').ToLowerInvariant()}finally{$sha.Dispose()}
}
