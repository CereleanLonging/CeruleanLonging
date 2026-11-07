import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

assert(process.env.CERULEAN_CACHE_SESSION);
assert.equal(process.env.CERULEAN_BUILD_SCHEDULE_ACTIVE, '1');
const scratch = mkdtempSync(join(process.env.CERULEAN_CACHE_SESSION, 'update-reader-policy-'));
const source = readFileSync('scripts/manage-windows-update.ps1', 'utf8');
const start = source.indexOf('function ReaderGeneration(');
const end = source.indexOf('function HashBytes(', start);
assert(start >= 0 && end > start);
const definitions = source.slice(start, end);
assert(definitions.includes('function CheckDataFormat('));
const escapedRoot = scratch.replaceAll("'", "''");
const script = `
$ErrorActionPreference='Stop'
$cases=[Collections.Generic.List[string]]::new()
${definitions}
function Package([int]$generation) {
    return [pscustomobject]@{packageDirectory='D:\\synthetic-package';capabilities=[pscustomobject]@{
        version='0.1.48';dataReaderGeneration=$generation;dataFormatGuardSchema=1;
        binaryUpdateSchema=2;dataRecoverySchema=2;readableDataSchemas=@(1,2)}}
}
function Core([string]$binary,[string[]]$arguments) {
    $script:coreCalls++
    return [pscustomobject]@{result=$script:format}
}
function Reject([scriptblock]$operation,[string]$name) {
    $rejected=$false
    try { & $operation | Out-Null } catch { $rejected=$true }
    if(-not $rejected){throw ('Expected rejection: '+$name)}
    $cases.Add($name)
}
$script:coreCalls=0
$script:plan=[pscustomobject]@{data='${escapedRoot}'}
$events=Join-Path $script:plan.data 'events'
$null=New-Item -ItemType Directory -Path $events
$script:format=[pscustomobject]@{dataSchema=1;minimumReaderGeneration=1;readerGuardDigest=$null}
CheckDataFormat (Package 18) (Package 25)
$cases.Add('new reader accepts unchanged legacy data')
for($generation=3;$generation -le 25;$generation++) {
    if((ReaderGeneration (Package $generation)) -ne $generation){throw 'Reader declaration mismatch'}
}
$cases.Add('all supported explicit generations remain numeric')
foreach($generation in @(0,2,26)) {
    Reject {ReaderGeneration (Package $generation)} ('unsupported declaration '+$generation)
}
foreach($value in @('25',25.5)) {
    $package=Package 25
    $package.capabilities.dataReaderGeneration=$value
    Reject {ReaderGeneration $package} ('nonintegral declaration '+$value)
}
$package=Package 25
$package.capabilities.dataRecoverySchema=1
Reject {ReaderGeneration $package} 'inconsistent recovery capabilities'
for($generation=3;$generation -le 25;$generation++) {
    [IO.File]::WriteAllText((Join-Path $events ('00000000.reader'+$generation+'.json')),'{}')
    $script:format.minimumReaderGeneration=$generation
    $script:format.readerGuardDigest='a'*64
    CheckDataFormat (Package 25) (Package 25)
    $cases.Add('recognized marker '+$generation)
    if($generation -ge 19) {
        Reject {CheckDataFormat (Package ($generation-1)) (Package 25)} ('old source blocked for '+$generation)
        Reject {CheckDataFormat (Package 25) (Package ($generation-1))} ('old candidate blocked for '+$generation)
    }
}
$script:format.minimumReaderGeneration=24
Reject {CheckDataFormat (Package 25) (Package 25)} 'minimum declaration must match newest marker'
$script:format.minimumReaderGeneration=25
$script:format.readerGuardDigest=$null
Reject {CheckDataFormat (Package 25) (Package 25)} 'reader guard digest required'
$script:format.readerGuardDigest='a'*64
[IO.File]::WriteAllText((Join-Path $events '00000000.reader26.json'),'{}')
$before=$script:coreCalls
Reject {CheckDataFormat (Package 25) (Package 25)} 'future marker blocked before core invocation'
if($script:coreCalls -ne $before){throw 'Future marker reached core'}
[pscustomobject]@{passed=$true;cases=$cases.ToArray();coreCalls=$script:coreCalls;
    scope='production PowerShell reader gates with synthetic core declarations';modelCalls=0;userDataChanges=0}|ConvertTo-Json -Depth 4
`;
const path = join(scratch, 'test.ps1');
writeFileSync(path, '\ufeff' + script);
const result = spawnSync(join(process.env.SystemRoot, 'System32/WindowsPowerShell/v1.0/powershell.exe'),
  ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path],
  { windowsHide: true, encoding: 'utf8', timeout: 60000 });
assert.ifError(result.error);
assert.equal(result.status, 0, result.stdout + result.stderr);
const receipt = JSON.parse(result.stdout.replace(/^\uFEFF/, ''));
assert.equal(receipt.passed, true);
assert.equal(receipt.userDataChanges, 0);
console.log(JSON.stringify(receipt));
