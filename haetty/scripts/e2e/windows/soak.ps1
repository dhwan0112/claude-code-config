# Soak v2 (T4), Windows: the Linux soak.sh with the same operations — 200 x open/close a tab, 50 x detach/attach,
# 20 x task + worker exit + worktree removal, 10 x theme change — spread over -Dur seconds (default 3600) around four
# tabs (a burst of 500 lines every 10 s, a line every 0.2 s, two idle); a sample every 300 s between operations:
# threads, handles, private working set of the GUI and the daemon, the daemon's descendant processes, sessions,
# task worktrees left. Keys go through SendInput (lib.ps1): it runs only on an idle desktop and stops at once if
# the haetty window loses the foreground. Run with pwsh:
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"; pwsh -File soak.ps1 -Exe C:\...\haetty.exe -Out soak.csv
# Verdict (written before the first run; the baseline moved from t=0 to 5 min in V6, see below): threads last <=
# base + 3 (Windows' own pool threads), handles last <=
# first + 20; descendants and sessions last = first; worktrees 0; private working set: GUI last <= GUI at 15 min +
# 4 MB, daemon last <= daemon when the scrollback is full + 4 MB (E5c, as scripts/e2e/soak.sh: the first sample at
# which the largest session scrollback stopped growing, at least 15 min before the end, else FAIL; a build that
# does not report it is judged at 45 min); every operation done; window and daemon alive.
param([Parameter(Mandatory = $true)][string]$Exe, [Parameter(Mandatory = $true)][string]$Out, [int]$Dur = 3600)
. (Join-Path $PSScriptRoot "lib.ps1")
$W = New-HaettyRun "soak"
if (-not $W) { throw "soak.ps1: no run folder (lib.ps1 failed)" }
$R = "$W\proj"; New-Item -ItemType Directory $R | Out-Null
RepoGit $R init -q -b main; Set-Content "$R\a.txt" "one" -Encoding ascii; RepoGit $R add .; RepoGit $R commit -qm one
$ps = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
@'
param([int]$Mode)
switch ($Mode) {
  0 { while ($true) { $e = [char]27; for ($i = 0; $i -lt 500; $i++) { Write-Host "$e[3$($i % 8)m burst line $i $([DateTime]::Now.Ticks)$e[0m" }; Start-Sleep 10 } }
  1 { while ($true) { Write-Host "tick $([DateTime]::Now.ToString('HH:mm:ss.fff'))"; Start-Sleep -Milliseconds 200 } }
  default { Start-Sleep 100000 }
}
'@ | Set-Content -Path "$W\tab.ps1" -Encoding utf8
# The worker: Claude Code's name (claude.exe = powershell), prints a line and ends after 3 s.
"Write-Host `"worker in `$PWD`"; Start-Sleep 3" | Set-Content -Path "$W\worker.ps1" -Encoding utf8
function Theme([string]$name) {
    "theme = $name`nworker-command = $W\bin\claude.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $W\worker.ps1" | Set-Content "$W\cfg\config" -Encoding utf8
}
Theme "IBM 5153 CGA"
$tabArgs = @("-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "$W\tab.ps1", "-Mode")
$lines = foreach ($m in 0, 1, 2) {
    $a = ($tabArgs + "$m" | ForEach-Object { $_ -replace '\\', '\\' }) -join [char]0x1f
    "tab$m`t$($R -replace '\\', '\\')`t$($ps -replace '\\', '\\')`t$a`t`t0`t0"
}
[IO.File]::WriteAllText("$W\state\tabs\last.tsv", (($lines -join "`n") + "`n"))
# Not Start-Gui (it passes --restore-tabs false): this run needs the three restored tabs.
$script:Gui = Start-Process -FilePath $Exe -ArgumentList (@("--cols", "120", "--rows", "36", $ps) + $tabArgs + "3") -WorkingDirectory $R -PassThru
for ($i = 0; $i -lt 60; $i++) { Start-Sleep -Milliseconds 250; $script:Gui.Refresh(); if ($script:Gui.MainWindowHandle -ne 0) { break } }
Start-Sleep 4
$DP = (Status).daemon_pid
function Desc([int]$root) {
    $all = Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId
    $n = 0; $todo = @($root)
    while ($todo.Count) { $p = $todo[0]; $todo = @($todo | Select-Object -Skip 1); foreach ($c in $all | Where-Object { $_.ParentProcessId -eq $p }) { $n++; $todo += $c.ProcessId } }
    $n
}
function PWS([int]$id) { [int]((Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter "IDProcess=$id").WorkingSetPrivate / 1KB) }
function M([int]$id) { $p = Get-Process -Id $id; "$($p.Threads.Count),$($p.HandleCount),$(PWS $id)" }
function Wts { (@(RepoGit $R worktree list).Count - 1) + @(Get-ChildItem "$W\proj-tasks" -ErrorAction SilentlyContinue).Count }
$script:tabs = 0; $script:dets = 0; $script:tasks = 0; $script:themes = 0; $script:fails = 0
# The largest scrollback of a session in rows (-1: the build does not report it).
function SbMax { $v = @((Status).sessions | Where-Object { $null -ne $_.scrollback } | ForEach-Object { [int64]$_.scrollback }); if ($v.Count) { ($v | Measure-Object -Maximum).Maximum } else { -1 } }
"t_s,gui_threads,gui_handles,gui_private_ws_kb,daemon_threads,daemon_handles,daemon_private_ws_kb,daemon_descendants,sessions,worktrees,tabs_done,detach_done,tasks_done,themes_done,scrollback_rows" | Set-Content $Out
function Sample([int]$t) { "$t,$(M $script:Gui.Id),$(M $DP),$(Desc $DP),$((Running).Count),$(Wts),$script:tabs,$script:dets,$script:tasks,$script:themes,$(SbMax)" | Add-Content $Out }
function Op-Tab { Keys "^+t" 1300; Keys "^+w" 300; Keys "^+w" 800; $script:tabs++ }
function Op-Detach { Keys "%4"; Palette "detach"; Start-Sleep 0.5; Palette "attach detached"; Start-Sleep 0.8; $script:dets++ }
function Op-Task {
    Keys "%1"; Palette "new task"; Keys "soak task $script:tasks" 200; Keys "{ENTER}"; Keys "d" 200; Keys "{ENTER}"; Keys "{ENTER}" 2500
    Start-Sleep 4
    Keys "^+w"; Start-Sleep 0.5; Palette "remove a task"; Keys "{ENTER}" 1800
    if ((Wts) -ne 0) { $script:fails++; Write-Host "task $($script:tasks): $(Wts) worktree(s) left" }
    $script:tasks++
}
function Op-Theme { if ($script:themes % 2 -eq 0) { Theme "haetty classic" } else { Theme "IBM 5153 CGA" }; Palette "reload config"; $script:themes++ }
$f = [Math]::Min(1.0, $Dur / 3600)
$n = { param($c) [Math]::Max(1, [int][Math]::Round($c * $f)) }
$ops = @("tab") * (& $n 200) + @("detach") * (& $n 50) + @("task") * (& $n 20) + @("theme") * (& $n 10)
$rng = New-Object System.Random 7; $ops = $ops | Sort-Object { $rng.Next() }
$gap = [Math]::Max(0, ($Dur - 120 - 1100 * $ops.Count / 280) / $ops.Count)
$t0 = Get-Date; $next = 300
Sample 0
foreach ($op in $ops) {
    $now = [int]((Get-Date) - $t0).TotalSeconds
    if ($now -ge $next) { Sample $now; $next += 300 }
    if ($script:Gui.HasExited -or -not (Get-Process -Id $DP -ErrorAction SilentlyContinue)) { Write-Host "DIED at $now s"; break }
    switch ($op) { "tab" { Op-Tab } "detach" { Op-Detach } "task" { Op-Task } "theme" { Op-Theme } }
    Start-Sleep -Milliseconds ([int]($gap * 1000))
}
while (((Get-Date) - $t0).TotalSeconds -lt $Dur) {
    $now = [int]((Get-Date) - $t0).TotalSeconds; if ($now -ge $next) { Sample $now; $next += 300 }; Start-Sleep 5
}
Start-Sleep 3; Sample ([int]((Get-Date) - $t0).TotalSeconds)
$alive = (-not $script:Gui.HasExited) -and [bool](Get-Process -Id $DP -ErrorAction SilentlyContinue)
Stop-Gui
Write-Host "operations: tabs $script:tabs, detach $script:dets, tasks $script:tasks, themes $script:themes; task cleanup failures $script:fails; alive at the end: $alive"
$rows = Import-Csv $Out | ForEach-Object { $o = @{}; foreach ($p in $_.PSObject.Properties) { $o[$p.Name] = [int]$p.Value }; $o }
$first = $rows[0]; $last = $rows[-1]
function At([int]$t) { $rows | Sort-Object { [Math]::Abs($_.t_s - $t) } | Select-Object -First 1 }
# Threads and handles are compared with the sample at 5 min, after the window's warm-up: at t=0 the GUI has not
# yet opened what it keeps (V6: 243 handles at 4 s, 278 a little later, then flat over 60 tab opens/closes).
# A run without a later sample compares with t=0.
$base = if ($rows.Count -ge 3) { At 300 } else { $first }
$ok = $true
function Check($name, $cond, $detail) { $script:ok = $script:ok -and $cond; Write-Host "$(if ($cond) { 'PASS' } else { 'FAIL' }) ${name}: $detail" }
Check "gui_threads" ($last.gui_threads -le $base.gui_threads + 3) "$($base.gui_threads) at $($base.t_s) s -> $($last.gui_threads)"
Check "daemon_threads" ($last.daemon_threads -le $base.daemon_threads + 3) "$($base.daemon_threads) at $($base.t_s) s -> $($last.daemon_threads)"
Check "gui_handles" ($last.gui_handles -le $base.gui_handles + 20) "$($base.gui_handles) at $($base.t_s) s -> $($last.gui_handles)"
Check "daemon_handles" ($last.daemon_handles -le $base.daemon_handles + 20) "$($base.daemon_handles) at $($base.t_s) s -> $($last.daemon_handles)"
Check "daemon_descendants" ($last.daemon_descendants -eq $first.daemon_descendants) "$($first.daemon_descendants) -> $($last.daemon_descendants)"
Check "sessions" ($last.sessions -eq $first.sessions) "$($first.sessions) -> $($last.sessions)"
Check "worktrees" ($last.worktrees -eq 0) "$($last.worktrees)"
$g15 = At 900
Check "gui_private_ws" ($last.gui_private_ws_kb -le $g15.gui_private_ws_kb + 4096) "$($g15.gui_private_ws_kb) kB at $($g15.t_s) s -> $($last.gui_private_ws_kb) kB"
$sb = @($rows | ForEach-Object { if ($_.ContainsKey('scrollback_rows')) { $_.scrollback_rows } else { -1 } })
if (($sb | Measure-Object -Minimum).Minimum -lt 0) {
    $d = At 2700
    Check "daemon_private_ws" ($last.daemon_private_ws_kb -le $d.daemon_private_ws_kb + 4096) "$($d.daemon_private_ws_kb) kB at $($d.t_s) s -> $($last.daemon_private_ws_kb) kB (at 45 min: this build reports no scrollback)"
} else {
    $full = $null
    for ($i = 1; $i -lt $rows.Count; $i++) { if ($rows[$i].scrollback_rows -gt 0 -and $rows[$i].scrollback_rows -le $rows[$i - 1].scrollback_rows * 1.01) { $full = $rows[$i]; break } }
    if (-not $full -or $last.t_s - $full.t_s -lt 900) {
        $when = if ($full) { "only at $($full.t_s) s" } else { "never" }
        Check "daemon_private_ws" $false "scrollback full $when ($($sb[-1]) rows at the end): less than 15 min after it to judge"
    } else {
        Check "daemon_private_ws" ($last.daemon_private_ws_kb -le $full.daemon_private_ws_kb + 4096) "$($full.daemon_private_ws_kb) kB when the scrollback was full ($($full.scrollback_rows) rows, $($full.t_s) s) -> $($last.daemon_private_ws_kb) kB"
    }
}
Write-Host "VERDICT $(if ($ok -and $alive -and $script:fails -eq 0) { 'PASS' } else { 'FAIL' })"
