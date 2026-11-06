# 1-hour soak on Windows (stage S4): the dev GUI with 4 tabs (a burst of 500 coloured lines every 10 s, a steady
# line every 0.2 s, two idle); every 60 s private bytes, working set, handles and threads of the GUI and the
# daemon go to a CSV. Test isolation: test-guard.ps1, everything under a run directory in $env:HAETTY_TEST_TMP.
param([string]$Exe, [string]$Out, [int]$Dur = 3600)
$ErrorActionPreference = "Continue"
. (Join-Path $PSScriptRoot "..\..\test-guard.ps1")
$run = Initialize-HaettyGuard -Name HAETTY_TEST_TMP
$w = Join-Path $run "soak"; Assert-HaettyPath $w
foreach ($d in "$w", "$w\state\tabs", "$w\cfg") { New-Item -ItemType Directory -Force $d | Out-Null }
$env:HAETTY_INSTANCE = "dev"; $env:HAETTY_SOCKET = "\\.\pipe\haetty-soak-$PID"
$env:HAETTY_STATE_DIR = "$w\state"; $env:HAETTY_CONFIG_DIR = "$w\cfg"; $env:HAETTY_LOG = "info"; $env:HAETTY_LOG_FILE = "$w\gui.log"
$tab = "$w\tab.ps1"
@'
param([int]$Mode)
switch ($Mode) {
  0 { while ($true) { $e = [char]27; for ($i = 0; $i -lt 500; $i++) { Write-Host "$e[3$($i % 8)m burst line $i $([DateTime]::Now.Ticks)$e[0m" }; Start-Sleep 10 } }
  1 { while ($true) { Write-Host "tick $([DateTime]::Now.ToString('HH:mm:ss.fff'))"; Start-Sleep -Milliseconds 200 } }
  default { Start-Sleep 100000 }
}
'@ | Set-Content -Path $tab -Encoding utf8
$ps = (Get-Command powershell.exe).Source
$lines = foreach ($m in 1, 2, 3) {
  $args_ = @("-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $tab, "-Mode", "$m") | ForEach-Object { $_ -replace '\\', '\\' }
  "tab$m`t$($w -replace '\\', '\\')`t$($ps -replace '\\', '\\')`t$($args_ -join [char]0x1f)`t`t0`t0"
}
[IO.File]::WriteAllText("$w\state\tabs\last.tsv", (($lines -join "`n") + "`n"))
$gui = Start-Process -FilePath $Exe -ArgumentList @("--cols", "120", "--rows", "36", $ps, "-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $tab, "-Mode", "0") -WorkingDirectory $w -PassThru
Start-Sleep 5
$st = cmd /c "`"$Exe`" status --json" | Out-String | ConvertFrom-Json
$dp = $st.daemon_pid
Write-Host "gui $($gui.Id) daemon $dp sessions $($st.sessions.Count)"
"t_s,gui_private_kb,gui_ws_kb,gui_handles,gui_threads,daemon_private_kb,daemon_ws_kb,daemon_handles,daemon_threads,sessions" | Set-Content $Out
$t0 = Get-Date
while ($true) {
  $t = [int]((Get-Date) - $t0).TotalSeconds
  $g = Get-Process -Id $gui.Id -ErrorAction SilentlyContinue; $d = Get-Process -Id $dp -ErrorAction SilentlyContinue
  if (-not $g -or -not $d) { Write-Host "DIED at $t s"; break }
  $s = (cmd /c "`"$Exe`" status --json" | Out-String | ConvertFrom-Json).sessions | Where-Object { $_.state -eq "running" }
  "$t,$([int]($g.PrivateMemorySize64/1KB)),$([int]($g.WorkingSet64/1KB)),$($g.HandleCount),$($g.Threads.Count),$([int]($d.PrivateMemorySize64/1KB)),$([int]($d.WorkingSet64/1KB)),$($d.HandleCount),$($d.Threads.Count),$(@($s).Count)" | Add-Content $Out
  if ($t -ge $Dur) { break }
  Start-Sleep 60
}
Stop-Process -Id $gui.Id -ErrorAction SilentlyContinue
Start-Sleep 2
# The daemon by PID only (never by name).
if (Get-Process -Id $dp -ErrorAction SilentlyContinue) { Stop-Process -Id $dp -ErrorAction SilentlyContinue }
Write-Host "soak done: $((Get-Content $Out).Count) lines"
