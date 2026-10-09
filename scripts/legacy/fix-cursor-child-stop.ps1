# Optional compatibility fix for Cursor 3.22.12 Agents-window child Stop.
# Only the exact inspected Glass bundle is supported. Close Cursor before applying.
param(
    [string]$CursorApp = (Join-Path $env:LOCALAPPDATA 'Programs/cursor/resources/app'),
    [switch]$Restore,
    [switch]$Check
)
$ErrorActionPreference = 'Stop'
$file = Join-Path $CursorApp 'out/vs/workbench/workbench.glass.main.js'
$backup = "$file.nexusor-child-stop.bak"
$expected = 'E64FCC4428C0A6E633E2F6C2229307CC40547C5638DE2A4F65AEC518E669538E'
$utf8 = [Text.UTF8Encoding]::new($false, $true)
function Hash-Bytes([byte[]]$Bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return ([BitConverter]::ToString($sha.ComputeHash($Bytes))).Replace('-', '') }
    finally { $sha.Dispose() }
}
$current = [IO.File]::ReadAllBytes($file)
$currentHash = Hash-Bytes $current
if (Test-Path $backup) {
    $original = [IO.File]::ReadAllBytes($backup)
    if ((Hash-Bytes $original) -ne $expected) { throw 'Backup hash is not the supported original; no files changed.' }
} else {
    if ($currentHash -ne $expected) { throw 'Unsupported or modified Cursor Glass bundle; no files changed.' }
    $original = $current
}
$source = $utf8.GetString($original)
$old = 'o&&t.composerService.cancelChat(o),t.composerDataService.updateComposerBubbleSetStore(t.parentHandle,t.bubbleId'
# Resolve the same materialized-workspace service used by Cursor terminal Stop.
# Cloud keeps its original path; unknown local workspaces do not fall back to a wrong host.
$new = @'
o&&(Yc(o)?t.composerService.cancelChat(o):void(async()=>{let reference;try{const collection=t.instantiationService.invokeFunction(a=>a.get(K_)),workspaces=collection.getMaterializedWorkspaceIdentifiers(),workspace=t.parentHandle.data.workspaceIdentifier??(workspaces.length===1?workspaces[0]:void 0);if(!workspace||!workspaces.some(w=>w.id===workspace.id))throw new Error("Nexusor child Stop: workspace is unresolved");reference=await collection.createWorkspaceReference(workspace,Mc.Terminal);const host=reference.object.instantiationService.invokeFunction(a=>a.get(_Ht)),result=M_i.fromBinary(await host.interruptTurn(new xRe({sessionId:o,sessionTree:!0,reason:"Subagent stopped by user",initiatedBy:mZ.USER_STOP}).toBinary()));if(!result.interrupted)throw new Error("Nexusor child Stop: host did not interrupt a running child")}catch(error){console.error("Nexusor child Stop failed",error)}finally{reference?.dispose()}})()),t.composerDataService.updateComposerBubbleSetStore(t.parentHandle,t.bubbleId
'@
if (($source.Split([string[]]@($old), [StringSplitOptions]::None).Count - 1) -ne 1) {
    throw 'Expected exactly one child Stop anchor; no files changed.'
}
$patched = $utf8.GetBytes($source.Replace($old, $new))
$patchedHash = Hash-Bytes $patched
if ($currentHash -ne $expected -and $currentHash -ne $patchedHash) {
    throw 'Cursor changed since this fix was installed; refusing to overwrite it.'
}
if ($Check) {
    [pscustomobject]@{ Supported=$true; Applied=($currentHash -eq $patchedHash); Path=$file; OriginalSHA256=$expected; PatchedSHA256=$patchedHash }
    return
}
if (Get-Process Cursor -ErrorAction SilentlyContinue) { throw 'Close every Cursor window/process before applying or restoring this fix.' }
if ($Restore) {
    if (!(Test-Path $backup)) { throw 'No backup to restore.' }
    $target = $original
} else {
    if ($currentHash -eq $patchedHash) { 'Child Stop compatibility fix is already applied.'; return }
    if (!(Test-Path $backup)) { [IO.File]::WriteAllBytes($backup, $original) }
    $target = $patched
}
# Replace atomically on the same volume; retain the verified original backup.
$temporary = "$file.nexusor-$([Guid]::NewGuid().ToString('N')).tmp"
$replaced = "$temporary.previous"
try {
    [IO.File]::WriteAllBytes($temporary, $target)
    [IO.File]::Replace($temporary, $file, $replaced)
} finally {
    if (Test-Path $temporary) { Remove-Item -LiteralPath $temporary }
    if (Test-Path $replaced) { Remove-Item -LiteralPath $replaced }
}
[pscustomobject]@{ Restored=[bool]$Restore; Path=$file; SHA256=(Hash-Bytes ([IO.File]::ReadAllBytes($file))) }
