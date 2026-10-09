# Cursor 3.22.12 binary image Write: opt-in atomic create-new after existing permission checks.
param([string]$CursorApp=(Join-Path $env:LOCALAPPDATA 'Programs/cursor/resources/app'),[switch]$Check,[switch]$Restore)
$ErrorActionPreference='Stop'
$file=Join-Path $CursorApp 'extensions/cursor-agent-exec/dist/main.js'
$backup="$file.nexusor-image-write.bak"
$original=if(Test-Path $backup){[IO.File]::ReadAllBytes($backup)}else{[IO.File]::ReadAllBytes($file)}
function Hash([byte[]]$data){[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($data)).ToLowerInvariant()}
if((Hash $original) -ne '2a0e07b7e9479b856072f5fcc8a1e5b88e2218c8f5071d445f47938f44d62d64'){throw 'Unsupported Cursor exec; do not overwrite.'}
$text=[Text.Encoding]::UTF8.GetString($original)
$edits=@(
 @('await Pu(h,y,{flush:!0})','if(t.encodingHint==="nexusor-image-create-new-v1")o.signal?.throwIfAborted();await Pu(h,y,{flush:!0,nexusorExclusive:t.encodingHint==="nexusor-image-create-new-v1"&&void 0!==l})'),
 @('let t=e.O_WRONLY|e.O_CREAT;return','let t=e.O_WRONLY|e.O_CREAT;if(r?.nexusorExclusive)t|=e.O_EXCL;return'),
 @('u=Vo(c,["assets","agent-tools","swarm-agents"]);if','u=Vo(c,["assets","agent-tools","swarm-agents"]);if(t.encodingHint==="nexusor-image-probe-v1")return new ga.v3({result:{case:"error",value:new ga.QM({path:c,error:"NEXUSOR_IMAGE_CREATE_NEW_V1"})}});if')
)
foreach($edit in $edits){if(($text.Split([string[]]@($edit[0]),[StringSplitOptions]::None).Count-1)-ne 1){throw 'Ambiguous anchor'};$text=$text.Replace($edit[0],$edit[1])}
$patched=[Text.UTF8Encoding]::new($false).GetBytes($text)
$current=[IO.File]::ReadAllBytes($file)
if((Hash $current)-ne(Hash $original)-and(Hash $current)-ne(Hash $patched)){throw 'Cursor file changed; refusing overwrite'}
if($Check){[pscustomobject]@{Applied=((Hash $current)-eq(Hash $patched));Hash=(Hash $current)};return}
if(Get-Process Cursor -ErrorAction SilentlyContinue){throw 'Close Cursor first'}
if(!(Test-Path $backup)){[IO.File]::WriteAllBytes($backup,$original)}
$target=if($Restore){$original}else{$patched}
$temp="$file.$([guid]::NewGuid().ToString('N')).tmp"
try{[IO.File]::WriteAllBytes($temp,$target);[IO.File]::Replace($temp,$file,"$temp.previous")}finally{foreach($p in @($temp,"$temp.previous")){if(Test-Path $p){Remove-Item -LiteralPath $p}}}
[pscustomobject]@{Restored=[bool]$Restore;Hash=(Hash $target)}
