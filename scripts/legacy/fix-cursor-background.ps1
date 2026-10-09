# Optional Cursor 3.22.12 local foreground Shell/Task promotion compatibility.
# Requires the separately verified child-Stop patch. Close Cursor first.
param(
    [string]$CursorApp = (Join-Path $env:LOCALAPPDATA 'Programs/cursor/resources/app'),
    [switch]$Check,
    [switch]$Restore
)
$ErrorActionPreference = 'Stop'
$utf8 = [Text.UTF8Encoding]::new($false, $true)
function Hash-Bytes([byte[]]$bytes) {
    $sha = [Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '') }
    finally { $sha.Dispose() }
}
$glassOld = 'ne=X?.callId??Q?.toolCallId;return S0e("div",'
$glassNew = @'
ne=X?.callId??Q?.toolCallId;const nexusorIsTask=X?.toolCallVm?.case==="taskToolCall",nexusorCanBackground=y?.isGlass&&t.kind==="activity"&&t.entry.isStreaming===!0&&(X?.toolCallVm?.case==="shellToolCall"||nexusorIsTask),nexusorBackground=async event=>{const button=event.currentTarget;button.disabled=!0;let reference;try{const services=y.instantiationService,data=services.invokeFunction(a=>a.get(no)),matches=data.getLoadedComposers().map(id=>data.getHandleIfLoaded(id)).filter(h=>h&&h.data.fullConversationHeadersOnly.some(row=>h.data.conversationMap[row.bubbleId]?.toolFormerData?.toolCallId===ne));if(matches.length!==1)throw new Error("Cannot resolve active tool conversation");const handle=matches[0],workspace=handle.data.workspaceIdentifier,collection=services.invokeFunction(a=>a.get(K_));if(!workspace)throw new Error("Cannot resolve tool workspace");reference=await collection.createWorkspaceReference(workspace,Mc.Terminal);const host=reference.object.instantiationService.invokeFunction(a=>a.get(_Ht)),model=handle.data.modelConfig?.modelName;if(!model?.startsWith("plugin:")&&!model?.startsWith("combo:"))throw new Error("Manual background compatibility requires a Nexusor model");await host.sendAction(new k0n({sessionId:handle.composerId,action:new Ax({action:nexusorIsTask?{case:"backgroundSubagentAction",value:new u0o({toolCallId:ne})}:{case:"backgroundShellAction",value:new c0o({toolCallId:ne})}}),modelId:model}).toBinary());button.textContent="Background requested"}catch(error){button.disabled=!1;y.notificationService.error(String(error));console.error("Nexusor background request failed",error)}finally{reference?.dispose()}};const nexusorContent=nexusorCanBackground?S0e("div",{children:[W,S0e("button",{type:"button",className:"monaco-button monaco-text-button secondary","data-nexusor-background-shell":ne,"data-nexusor-background-kind":nexusorIsTask?"task":"shell",onClick:nexusorBackground,children:"Run in background"})]}):W;return S0e("div",
'@
$childOld = 'children:S0e(nls.Provider,{value:e.videoAnnotations,children:W})'
$childNew = 'children:S0e(nls.Provider,{value:e.videoAnnotations,children:nexusorContent})'
$hostOld = 'this.ensureNotDisposing();const d=this.getSession(t.sessionId),p=void 0===t.action?void 0:an(d,t.action);'
$hostNew = @'
this.ensureNotDisposing();const d=this.getSession(t.sessionId);if(["backgroundShellAction","backgroundSubagentAction"].includes(t.action?.action?.case)){const active=[...d.turns.values()].filter(turn=>turn.state==="running"&&!turn.cancelled);if(active.length!==1)throw new Error("Nexusor background control needs one active turn");const turn=active[0];yield turn.conversationActionManager.submitConversationAction(t.action);return{turnId:turn.turnId}}const p=void 0===t.action?void 0:an(d,t.action);
'@
$specs = @(
    @{ Relative='out/vs/workbench/workbench.glass.main.js'; Hash='A67145CC9D39A8BABAE7B6994686FF6CFABD63F2114709460CB03FA0787F76E2'; Edits=@(@($glassOld,$glassNew),@($childOld,$childNew)) },
    @{ Relative='extensions/cursor-agent-host/dist/main.js'; Hash='1293928F9EE0E4F3949385488B91416B63EFB43C0E699455B125A48ECA10430B'; Edits=@(,@($hostOld,$hostNew)) }
)
$plans = foreach ($spec in $specs) {
    $file = Join-Path $CursorApp $spec.Relative
    $backup = "$file.nexusor-background.bak"
    $current = [IO.File]::ReadAllBytes($file)
    $original = if (Test-Path $backup) { [IO.File]::ReadAllBytes($backup) } else { $current }
    if ((Hash-Bytes $original) -ne $spec.Hash) { throw "Unsupported Cursor file: $file" }
    $text = $utf8.GetString($original)
    foreach ($edit in $spec.Edits) {
        if (($text.Split([string[]]@($edit[0]),[StringSplitOptions]::None).Count - 1) -ne 1) { throw "Ambiguous patch anchor: $file" }
        $text = $text.Replace($edit[0],$edit[1])
    }
    $patched = $utf8.GetBytes($text)
    $hash = Hash-Bytes $current
    $patchedHash = Hash-Bytes $patched
    if ($hash -ne $spec.Hash -and $hash -ne $patchedHash) { throw "Cursor changed; refusing overwrite: $file" }
    @{ File=$file; Backup=$backup; Original=$original; Current=$current; Patched=$patched; Applied=($hash -eq $patchedHash); PatchedHash=$patchedHash }
}
if ($Check) { $plans | ForEach-Object { [pscustomobject]@{Path=$_.File;Applied=$_.Applied;PatchedSHA256=$_.PatchedHash} }; return }
if (Get-Process Cursor -ErrorAction SilentlyContinue) { throw 'Close Cursor before applying/restoring.' }
function Replace-Bytes($file,[byte[]]$bytes) {
    $temp="$file.$([Guid]::NewGuid().ToString('N')).tmp"
    try { [IO.File]::WriteAllBytes($temp,$bytes); [IO.File]::Replace($temp,$file,"$temp.previous") }
    finally { foreach($p in @($temp,"$temp.previous")) { if(Test-Path $p){Remove-Item -LiteralPath $p} } }
}
$written = @()
try {
    foreach ($plan in $plans) {
        if (!(Test-Path $plan.Backup)) { [IO.File]::WriteAllBytes($plan.Backup,$plan.Original) }
        $target = if ($Restore) { $plan.Original } else { $plan.Patched }
        Replace-Bytes $plan.File $target
        $written += $plan
    }
} catch {
    foreach($plan in $written){ Replace-Bytes $plan.File $plan.Current }
    throw
}
[pscustomobject]@{Restored=[bool]$Restore;Files=$plans.Count}
