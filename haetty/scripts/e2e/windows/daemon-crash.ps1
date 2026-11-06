# Windows: the daemon dies (Stop-Process by its own PID, never by name); its sessions' programs must not stay
# behind, the GUI keeps its window and the tab set. Isolation: test-guard.ps1 run directory, dev instance.
param([string]$Exe)
$ErrorActionPreference = "Continue"
. (Join-Path $PSScriptRoot "..\..\test-guard.ps1")
foreach ($h in (Join-Path $PSScriptRoot "..\..\..\tools\heavy.ps1"), (Join-Path $PSScriptRoot "heavy.ps1")) { if (Test-Path $h) { . $h; break } }
if (-not (Get-Command Enter-HaettyHeavy -ErrorAction SilentlyContinue)) { Write-Host "daemon-crash.ps1: tools\heavy.ps1 not found"; exit 2 }
Enter-HaettyHeavy "daemon-crash.ps1 $args"   # at most two heavy jobs, Linux and Windows together (tools/heavy.ps1)
$run = Initialize-HaettyGuard -Name HAETTY_TEST_TMP
$w = Join-Path $run "crash"; Assert-HaettyPath $w
foreach ($d in "$w", "$w\state\tabs", "$w\cfg") { New-Item -ItemType Directory -Force $d | Out-Null }
$env:HAETTY_INSTANCE = "dev"; $env:HAETTY_SOCKET = "\\.\pipe\haetty-crash-$PID"
$env:HAETTY_STATE_DIR = "$w\state"; $env:HAETTY_CONFIG_DIR = "$w\cfg"; $env:HAETTY_LOG = "info"; $env:HAETTY_LOG_FILE = "$w\gui.log"
$ps = (Get-Command powershell.exe).Source
$line = "tab2`t$($w -replace '\\', '\\')`t$($ps -replace '\\', '\\')`t" + ((@("-NoLogo", "-NoProfile", "-Command", "Start-Sleep 600") | ForEach-Object { $_ }) -join [char]0x1f) + "`t`t0`t0"
[IO.File]::WriteAllText("$w\state\tabs\last.tsv", "$line`n")
$gui = Start-Process -FilePath $Exe -ArgumentList @("--cols", "90", "--rows", "20", $ps, "-NoLogo", "-NoProfile", "-Command", "Start-Sleep 600") -WorkingDirectory $w -PassThru
Start-Sleep 6
$st = cmd /c "`"$Exe`" status --json" | Out-String | ConvertFrom-Json
$dp = $st.daemon_pid; $pids = @($st.sessions | ForEach-Object { $_.pid })
# Each session's program and everything under it (powershell, conhost/OpenConsole).
$all = @(Get-CimInstance Win32_Process | Where-Object { $_.ParentProcessId -eq $dp } | ForEach-Object { $_.ProcessId })
Write-Host "gui $($gui.Id) daemon $dp sessions $($pids -join ',') daemon children $($all -join ',')"
$before = Get-Content "$w\state\tabs\last.tsv" -Raw
Stop-Process -Id $dp -Force
Start-Sleep 5
$left = @($pids + $all | Where-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
if ($left.Count -eq 0) { Write-Host "PASS no orphans ($((@($pids + $all) | Sort-Object -Unique) -join ','))" }
else { Write-Host "FAIL orphans: $(($left | ForEach-Object { (Get-Process -Id $_).ProcessName + ':' + $_ }) -join ', ')" }
if (Get-Process -Id $gui.Id -ErrorAction SilentlyContinue) { Write-Host "PASS the window stays" } else { Write-Host "FAIL the window closed" }
if ((Get-Content "$w\state\tabs\last.tsv" -Raw) -eq $before) { Write-Host "PASS tab set last unchanged" } else { Write-Host "FAIL last changed" }
if (Select-String -Path "$w\gui.log" -Pattern "the daemon \(pid $dp\) is gone" -Quiet) { Write-Host "PASS logged" } else { Write-Host "FAIL not logged" }
Stop-Process -Id $gui.Id -ErrorAction SilentlyContinue
foreach ($p in $left) { Stop-Process -Id $p -ErrorAction SilentlyContinue }
