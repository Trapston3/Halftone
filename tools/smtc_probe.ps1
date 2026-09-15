# PS1 script: read SMTC sessions (machine-verified probe of the OS now-playing state)
using namespace Windows.Media.Control
Add-Type -AssemblyName System.Runtime.WindowsRuntime | Out-Null
Add-Type -AssemblyName System.Runtime.InteropServices.WindowsRuntime | Out-Null
if (-not ('System.WindowsRuntimeSystemExtensions' -as [type])) { Add-Type -AssemblyName System.Runtime.WindowsRuntime, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089 }
$null = [System.WindowsRuntimeSystemExtensions]::AsTask
[Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager,Windows.Media.Control,ContentType=WindowsRuntime] | Out-Null

$op = [Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]::RequestAsync()
$sysrt = [System.Runtime.WindowsRuntime.WindowsRuntimeSystemExtensions]
$asTask = $sysrt.GetMethods() | Where-Object { $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1' } | Select-Object -First 1
$asTask2 = $asTask
$task = $asTask.MakeGenericMethod([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionManager]).Invoke($null, @($op))
if (-not $task.Wait(5000)) { Write-Output "ERR=manager-timeout"; exit }
$mgr = $task.Result
$sessions = @($mgr.GetSessions())
if ($sessions.Count -eq 0) { Write-Output "NO_SESSIONS"; exit }
$session = $sessions | Select-Object -First 1
Write-Output ("SRC=" + $session.SourceAppUserModelId)
$propsOp = $session.TryGetMediaPropertiesAsync()
$propsTask = $asTask2.MakeGenericMethod([Windows.Media.Control.GlobalSystemMediaTransportControlsSessionMediaProperties]).Invoke($null, @($propsOp))
if (-not $propsTask.Wait(4000)) { Write-Output "ERR=props-timeout"; exit }
$props = $propsTask.Result
Write-Output ("TITLE=" + $props.Title)
Write-Output ("ARTIST=" + $props.Artist)
Write-Output ("ALBUM=" + $props.AlbumTitle)
Write-Output ("STATUS=" + $session.PlaybackInfo.PlaybackStatus)
$tp = $session.GetTimelineProperties()
Write-Output ("POS=" + [math]::Round($tp.Position.TotalSeconds,1))
Write-Output ("END=" + [math]::Round($tp.EndTime.TotalSeconds,1))
