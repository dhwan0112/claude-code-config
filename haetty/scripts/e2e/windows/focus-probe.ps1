# Windows: can this session bring a haetty window to the foreground? (Stage W1: the night runs of 2026-11-10 and
# 11-11 stopped at their first focus check; this is that check alone, with lib.ps1's DIAG lines around it.)
# Starts one dev window with a shell, records whether it came to the front by itself, asks for the foreground the way
# every end-to-end script does (Front: an Alt key, ShowWindow, SetForegroundWindow, ten tries) and closes it. The
# only key it sends is Front's Alt. Run it once from a PowerShell window and once from a scheduled
# task (tools\windows-catchup.ps1 -TaskFocusProbe does both) and compare the DIAG lines.
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"; pwsh -File focus-probe.ps1 -Exe C:\...\haetty.exe
# Like the others it needs a desktop nobody has touched for 120 s (else SKIP, exit 77).
param([Parameter(Mandatory = $true)][string]$Exe)
. (Join-Path $PSScriptRoot "lib.ps1")
$W = New-HaettyRun "focus"
if (-not $W) { throw "focus-probe.ps1: no run folder (lib.ps1 failed)" }
Start-Gui "cmd.exe" @("/k") $W
$own = Ours
Write-Host "the new window had the foreground by itself: $own"
if (Front) { Ok "the haetty window is in the foreground (by itself: $own)" }
else { Bad "focus" "the haetty window did not get the foreground (the DIAG lines above say what was in front)" }
Focus-Diag "end"
Stop-Gui
Total "focus-probe"
exit $script:F
