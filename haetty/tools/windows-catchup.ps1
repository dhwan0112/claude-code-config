# The Windows GUI checks in one run (stage W1, part B): start it once from a PowerShell window when you leave the
# desk — not from Claude Code — and read one result file afterwards.
#   pwsh -NoProfile -ExecutionPolicy Bypass -File \\wsl.localhost\<distro>\<repo>\haetty\tools\windows-catchup.ps1
# It
#   1. builds this checkout for Windows in WSL (a heavy-job slot): haetty.exe, two `update-test` builds that report
#      0.90.0 and 0.90.1, and the Linux release server of the update test (skipped with -NoBuild: the files of the
#      last run in -OutDir\bin are used);
#   2. stages the end-to-end scripts beside the results and runs, in this order, each only after 120 s without any
#      keyboard or mouse input (it waits up to -IdleWaitMinutes for that; the scripts' own keys count as input):
#        focus-probe.ps1   can this session bring a haetty window to the foreground? (with the DIAG lines)
#        tabs.ps1          the tab features
#        orchestrator.ps1  a task, its worktree, the worker tab, removal (the seven-question wizard)
#        settings.ps1      the settings button and panel (also the click at the display's scale: run at 150 % too)
#        update.ps1        the in-app update: form (b) and form (a), Restart now, Roll back, a version that ends at once
#        soak.ps1          -SoakMinutes (default 10) on libghostty, then on the native engine
#      and, with -TaskFocusProbe, focus-probe.ps1 once more from a scheduled task of its own (run level Limited,
#      as the night runs; the task is removed again) to compare the two DIAG lines;
#   3. stops a script at once when you touch the keyboard or the mouse (no more keys are sent; that item is FAIL
#      (stopped), the next one waits for an idle desktop again);
#   4. writes -OutDir\result-<date-time>.txt (copied to latest.txt): a table with PASS / FAIL / NOT-RUN per item and
#      the log of each failed item, then every DIAG line; the full logs are in -OutDir\logs.
# Isolation: every haetty of the run is a dev instance in $env:PUBLIC\haetty-test (test-guard.ps1); LOCALAPPDATA and
# APPDATA of the scripts point into -OutDir\profile, so the real profile, a stable install and its daemon are never
# touched. The computer is kept awake and the screen on while it runs.
# -DryRun: nothing is built, started or sent — the scripts are parsed, the paths and tools are checked, and the plan
# is printed (use it after changing a script).
param(
    [int]$SoakMinutes = 10,
    [int]$IdleWaitMinutes = 60,
    [string]$OutDir = (Join-Path $env:PUBLIC "haetty-catchup"),
    [string]$Only = "",            # a comma list of item names to run instead of all
    [switch]$NoBuild,
    [switch]$TaskFocusProbe,
    [switch]$DryRun
)
$ErrorActionPreference = "Continue"
. (Join-Path $PSScriptRoot "heavy.ps1")
$distro, $heavySh = Get-HaettyHeavySh
$proj = $heavySh -replace '/tools/heavy\.sh$', ''          # …/haetty in WSL
$projUnc = Split-Path $PSScriptRoot                         # the same folder from Windows
$bin = Join-Path $OutDir "bin"; $logs = Join-Path $OutDir "logs"; $stage = Join-Path $OutDir "scripts"
$stamp = Get-Date -Format "yyyyMMdd-HHmm"
$result = Join-Path $OutDir "result-$stamp.txt"
$exe = Join-Path $bin "haetty.exe"; $old = Join-Path $bin "haetty-0.90.0.exe"; $new = Join-Path $bin "haetty-0.90.1.exe"
$ca = Join-Path $stage "TEST-ONLY-ca.pem"
$wslServer = "${distro}:$proj/target/debug/examples/fake_release_server"
$A = "aaaaaaaaaaaa0000000000000000000000000000"; $B = "bbbbbbbbbbbb0000000000000000000000000000"

# The plan: name, script, arguments.
$items = @(
    @{ name = "focus-probe"; script = "focus-probe.ps1"; args = @("-Exe", $exe) },
    @{ name = "tabs"; script = "tabs.ps1"; args = @("-Exe", $exe) },
    @{ name = "orchestrator"; script = "orchestrator.ps1"; args = @("-Exe", $exe) },
    @{ name = "settings"; script = "settings.ps1"; args = @("-Exe", $exe) },
    @{ name = "update"; script = "update.ps1"; args = @("-Old", $old, "-New", $new, "-Ca", $ca, "-WslServer", $wslServer) },
    @{ name = "soak-ghostty"; script = "soak.ps1"; args = @("-Exe", $exe, "-Out", (Join-Path $logs "soak-ghostty.csv"), "-Dur", "$($SoakMinutes * 60)"); engine = "" },
    @{ name = "soak-native"; script = "soak.ps1"; args = @("-Exe", $exe, "-Out", (Join-Path $logs "soak-native.csv"), "-Dur", "$($SoakMinutes * 60)"); engine = "native" }
)
if ($Only) { $want = $Only -split ','; $items = @($items | Where-Object { $want -contains $_.name }) }

function Wsl([string]$cmd) { & wsl.exe -d $distro --exec bash -c $cmd 2>&1 | ForEach-Object { "$_" } }
function Parse-All([string]$dir) {
    $bad = @()
    foreach ($f in Get-ChildItem -Recurse -File -Filter *.ps1 $dir) {
        $t = $null; $e = $null
        [void][System.Management.Automation.Language.Parser]::ParseFile($f.FullName, [ref]$t, [ref]$e)
        if ($e.Count) { $bad += "$($f.Name): line $($e[0].Extent.StartLineNumber): $($e[0].Message)" }
    }
    $bad
}
function Stage-Scripts {
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force "$stage\e2e\windows", "$stage\e2e\soak-cases", $logs, $bin | Out-Null
    Copy-Item "$projUnc\scripts\test-guard.ps1" $stage
    Copy-Item "$projUnc\scripts\e2e\windows\*.ps1" "$stage\e2e\windows"
    Copy-Item -Recurse "$projUnc\scripts\e2e\soak-cases\*" "$stage\e2e\soak-cases"
    Copy-Item (Join-Path $PSScriptRoot "heavy.ps1") "$stage\e2e\windows\heavy.ps1"   # lib.ps1 looks beside itself too
    Copy-Item "$projUnc\crates\haetty-update\tests\TEST-ONLY-ca.pem" $ca
}

if ($DryRun) {
    "DRY RUN: nothing is built, started or sent."
    "checkout: $projUnc  (WSL $distro, $proj)"
    "results would go to $result ; logs to $logs"
    $src = "$projUnc\scripts\e2e\windows"
    $problems = @()
    foreach ($i in $items) { if (-not (Test-Path (Join-Path $src $i.script))) { $problems += "missing script: $($i.script)" } }
    foreach ($f in "$projUnc\scripts\test-guard.ps1", "$src\lib.ps1", "$src\soak-verdict.ps1", "$projUnc\crates\haetty-update\tests\TEST-ONLY-ca.pem", (Join-Path $PSScriptRoot "heavy.ps1")) { if (-not (Test-Path $f)) { $problems += "missing file: $f" } }
    $pe = @(Parse-All "$projUnc\scripts") + @(Parse-All $PSScriptRoot)
    "parse (PowerShell $($PSVersionTable.PSVersion)): $(if ($pe.Count) { $pe -join '; ' } else { 'every .ps1 under scripts\ and tools\ parses' })"
    $problems += $pe
    $ps5 = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
    foreach ($t in @(@("pwsh", (Get-Command pwsh -ErrorAction SilentlyContinue)), @("powershell.exe 5.1 (the fake claude, update.ps1's stand-in)", (Test-Path $ps5)), @("git.exe", (Get-Command git.exe -ErrorAction SilentlyContinue)), @("wsl.exe", (Get-Command wsl.exe -ErrorAction SilentlyContinue)))) {
        "tool {0}: {1}" -f $t[0], $(if ($t[1]) { "found" } else { $problems += "missing tool: $($t[0])"; "MISSING" })
    }
    $w = @(Wsl ". ~/.cargo/env 2>/dev/null; command -v zig >/dev/null || PATH=`$HOME/.local/bin:`$PATH; cd '$proj' && echo cargo=`$(command -v cargo) zig=`$(command -v zig) target=`$(rustup target list --installed | grep -c windows-gnu) head=`$(git rev-parse --short HEAD)")
    "WSL: $($w -join ' ')"
    if (($w -join ' ') -notmatch 'cargo=/\S+ zig=/\S+ target=1') { $problems += "WSL toolchain: $($w -join ' ')" }
    "binaries of the last run in ${bin}: haetty.exe $(Test-Path $exe), haetty-0.90.0.exe $(Test-Path $old), haetty-0.90.1.exe $(Test-Path $new)$(if ($NoBuild) { ' (-NoBuild: these are needed)' } else { ' (a run builds them)' })"
    if ($NoBuild) { foreach ($f in $exe, $old, $new) { if (-not (Test-Path $f)) { $problems += "missing binary for -NoBuild: $f" } } }
    "build commands (WSL, one heavy slot):"
    "  cargo build -q --release -p haetty-gui --target x86_64-pc-windows-gnu"
    "  HAETTY_BUILD_VERSION=0.90.0 HAETTY_BUILD_COMMIT=$A CARGO_TARGET_DIR=target-updtest cargo build -q --profile dist -p haetty-gui --features update-test --target x86_64-pc-windows-gnu   (and 0.90.1, $($B.Substring(0, 12))…)"
    "  cargo build -q --example fake_release_server -p haetty-update"
    "the run, in this order (each after 120 s without input, waiting up to $IdleWaitMinutes min):"
    foreach ($i in $items) { "  {0,-13} pwsh -File scripts\e2e\windows\{1} {2}{3}" -f $i.name, $i.script, ($i.args -join ' '), $(if ($i.engine) { "   [HAETTY_E2E_ENGINE=$($i.engine)]" } else { "" }) }
    if ($TaskFocusProbe) { "  focus-probe (task): the same script from a scheduled task 'haetty-catchup-focus' (run level Limited), removed afterwards" }
    "in Claude Code now: $([bool]($env:CLAUDECODE -or $env:CLAUDE_CODE_ENTRYPOINT)) (a real run refuses to start there)"
    if ($problems.Count) { "DRY RUN: $($problems.Count) problem(s):"; $problems | ForEach-Object { "  $_" }; exit 1 }
    "DRY RUN: no problem found."
    exit 0
}

if ($env:CLAUDECODE -or $env:CLAUDE_CODE_ENTRYPOINT) { Write-Host "windows-catchup.ps1: start it from a PowerShell window, not from Claude Code (it sends keys to the desktop and runs for a long time). -DryRun works anywhere."; exit 2 }
New-Item -ItemType Directory -Force $OutDir, $logs, $bin | Out-Null
$plog = Join-Path $OutDir "progress.log"
function Say([string]$m) { $l = "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $m"; Write-Host $l; Add-Content $plog $l }
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class CatchupPower {
  [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);
  [StructLayout(LayoutKind.Sequential)] public struct LII { public uint cbSize; public uint dwTime; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LII p);
}
"@
function Idle { $l = New-Object CatchupPower+LII; $l.cbSize = 8; [void][CatchupPower]::GetLastInputInfo([ref]$l); [int](([Environment]::TickCount - $l.dwTime) / 1000) }
function Locked { [bool](Get-Process -Name LogonUI -ErrorAction SilentlyContinue) }
function Wait-Desktop {
    $until = (Get-Date).AddMinutes($IdleWaitMinutes)
    while ((Get-Date) -lt $until -and ((Idle) -lt 125 -or (Locked))) { Start-Sleep 10 }
    if ((Idle) -ge 125 -and -not (Locked)) { return $null }
    "NOT-RUN (foreground: $(if (Locked) { 'the screen was locked' } else { 'the desktop was in use' }) for $IdleWaitMinutes min)"
}
$rows = [ordered]@{}; foreach ($i in $items) { $rows[$i.name] = "NOT-RUN" }
if ($TaskFocusProbe) { $rows["focus-probe (task)"] = "NOT-RUN" }
$head = @()
try {
    [void][CatchupPower]::SetThreadExecutionState([uint32]"0x80000003")   # no sleep, the screen stays on
    Say "windows catch-up: results in $result"
    # 1. The builds.
    if (-not $NoBuild) {
        Say "building in WSL ($proj): haetty.exe, the two update-test builds, the release server"
        $pre = ". ~/.cargo/env 2>/dev/null; command -v zig >/dev/null || PATH=`$HOME/.local/bin:`$PATH; cd '$proj' || exit 2; T=x86_64-pc-windows-gnu; "
        $build = $pre + "tools/heavy.sh --label 'windows-catchup build' -- bash -c '" +
            "cargo build -q --release -p haetty-gui --target x86_64-pc-windows-gnu && " +
            "HAETTY_BUILD_VERSION=0.90.0 HAETTY_BUILD_COMMIT=$A CARGO_TARGET_DIR=target-updtest cargo build -q --profile dist -p haetty-gui --features update-test --target x86_64-pc-windows-gnu && cp target-updtest/x86_64-pc-windows-gnu/dist/haetty.exe /tmp/haetty-catchup-0.90.0.exe && " +
            "HAETTY_BUILD_VERSION=0.90.1 HAETTY_BUILD_COMMIT=$B CARGO_TARGET_DIR=target-updtest cargo build -q --profile dist -p haetty-gui --features update-test --target x86_64-pc-windows-gnu && cp target-updtest/x86_64-pc-windows-gnu/dist/haetty.exe /tmp/haetty-catchup-0.90.1.exe && " +
            "cargo build -q --example fake_release_server -p haetty-update' 2>&1 | tail -5"
        $o = @(Wsl $build); $o | ForEach-Object { Say "  $_" }
        $wbin = "\\wsl.localhost\$distro"
        Copy-Item -LiteralPath "$projUnc\target\x86_64-pc-windows-gnu\release\haetty.exe" $exe -Force -ErrorAction SilentlyContinue
        Copy-Item -LiteralPath "$wbin\tmp\haetty-catchup-0.90.0.exe" $old -Force -ErrorAction SilentlyContinue
        Copy-Item -LiteralPath "$wbin\tmp\haetty-catchup-0.90.1.exe" $new -Force -ErrorAction SilentlyContinue
    }
    foreach ($f in $exe, $old, $new) { if (-not (Test-Path $f)) { Say "missing ${f}: the build failed (or -NoBuild without an earlier run)"; foreach ($k in @($rows.Keys)) { $rows[$k] = "NOT-RUN (build)" }; return } }
    $env:HAETTY_INSTANCE = "dev"
    $head += "haetty.exe: $((& $exe --version | Out-String).Trim())"
    $head += "update-test builds: $((& $old --version | Out-String).Trim()) | $((& $new --version | Out-String).Trim())"
    $head += "harness: $((Wsl "git -C '$proj' log -1 --format='%h %s'") -join ' ')$(if ((Wsl "git -C '$proj' status --porcelain") -join '') { ' (+ uncommitted changes)' })"
    $head | ForEach-Object { Say $_ }
    # 2. The scripts, staged; this run's environment.
    Stage-Scripts
    $pe = @(Parse-All $stage); if ($pe.Count) { Say "parse errors: $($pe -join '; ')"; foreach ($k in @($rows.Keys)) { $rows[$k] = "NOT-RUN (a script does not parse)" }; return }
    . "$stage\test-guard.ps1"    # keeps the real profile's folders (HAETTY_REAL_*) before they are redirected
    $env:HAETTY_TEST_TMP = Join-Path $env:PUBLIC "haetty-test"
    $env:HAETTY_HEAVY_SH = "${distro}:$heavySh"
    New-Item -ItemType Directory -Force "$OutDir\profile\local", "$OutDir\profile\roaming" | Out-Null
    $env:LOCALAPPDATA = "$OutDir\profile\local"; $env:APPDATA = "$OutDir\profile\roaming"
    Get-ChildItem env: | Where-Object { $_.Name -match '^CLAUDE_CODE_|^CLAUDECODE$|^CLAUDE_.*SESSION|^HAETTY_SESSION_ID$' } | ForEach-Object { Remove-Item "env:$($_.Name)" }
    # 3. One item after the other.
    foreach ($i in $items) {
        $why = Wait-Desktop
        if ($why) { $rows[$i.name] = $why; Say "$($i.name): $why"; continue }
        if ($i.engine) { $env:HAETTY_E2E_ENGINE = $i.engine } else { Remove-Item env:HAETTY_E2E_ENGINE -ErrorAction SilentlyContinue }
        $log = Join-Path $logs "$($i.name).log"
        Say "$($i.name): started (desktop idle $(Idle) s)"
        $al = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "$stage\e2e\windows\$($i.script)") + $i.args
        & pwsh @al 2>&1 | ForEach-Object { "$_" } | Set-Content -Path $log -Encoding utf8
        $rc = $LASTEXITCODE
        $out = @(Get-Content $log)
        $total = $out | Where-Object { $_ -match '^TOTAL ' } | Select-Object -Last 1
        $verdict = $out | Where-Object { $_ -match '^VERDICT ' } | Select-Object -Last 1
        $rows[$i.name] = if ($rc -eq 77) { "NOT-RUN (foreground: the desktop was in use at its start)" }
            elseif ($rc -eq 2 -and ($out -match '^(STOP|FAIL focus)')) { "FAIL (stopped: input from somebody, or the window lost the foreground) - $log" }
            elseif ($i.script -eq "soak.ps1") { if ($verdict -match 'VERDICT PASS') { "PASS ($SoakMinutes min, $($out | Where-Object { $_ -match '^operations:' } | Select-Object -Last 1))" } else { "FAIL ($verdict; exit $rc) - $log" } }
            elseif ($rc -eq 0 -and $total -match ' 0 failed') { "PASS ($total)" }
            else { "FAIL (exit $rc; $total) - $log" }
        Say "$($i.name): $($rows[$i.name])"
    }
    # 4. The focus probe once more, from a scheduled task (medium integrity, as the night runs start).
    if ($TaskFocusProbe) {
        $why = Wait-Desktop
        if ($why) { $rows["focus-probe (task)"] = $why }
        else {
            $tlog = Join-Path $logs "focus-probe-task.log"; $done = Join-Path $logs "focus-probe-task.done"
            Remove-Item $tlog, $done -ErrorAction SilentlyContinue
            $wrap = Join-Path $OutDir "focus-probe-task.ps1"
            @"
`$env:HAETTY_TEST_TMP = '$($env:HAETTY_TEST_TMP)'; `$env:HAETTY_HEAVY_SH = '$($env:HAETTY_HEAVY_SH)'; `$env:HAETTY_INSTANCE = 'dev'
`$env:HAETTY_REAL_HOME = '$($env:HAETTY_REAL_HOME)'; `$env:HAETTY_REAL_APPDATA = '$($env:HAETTY_REAL_APPDATA)'; `$env:HAETTY_REAL_LOCALAPPDATA = '$($env:HAETTY_REAL_LOCALAPPDATA)'
`$env:LOCALAPPDATA = '$($env:LOCALAPPDATA)'; `$env:APPDATA = '$($env:APPDATA)'
& pwsh -NoProfile -ExecutionPolicy Bypass -File '$stage\e2e\windows\focus-probe.ps1' -Exe '$exe' 2>&1 | ForEach-Object { "`$_" } | Set-Content -Path '$tlog' -Encoding utf8
"exit `$LASTEXITCODE" | Set-Content '$done'
"@ | Set-Content -Path $wrap -Encoding utf8
            $name = "haetty-catchup-focus"
            $act = New-ScheduledTaskAction -Execute (Get-Command pwsh).Source -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$wrap`""
            $pr = New-ScheduledTaskPrincipal -UserId ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive -RunLevel Limited
            Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue
            Register-ScheduledTask -TaskName $name -Action $act -Principal $pr | Out-Null
            Start-ScheduledTask -TaskName $name
            for ($k = 0; $k -lt 120 -and -not (Test-Path $done); $k++) { Start-Sleep 2 }
            Unregister-ScheduledTask -TaskName $name -Confirm:$false
            $out = @(Get-Content $tlog -ErrorAction SilentlyContinue); $total = $out | Where-Object { $_ -match '^TOTAL ' } | Select-Object -Last 1
            $rows["focus-probe (task)"] = if (-not (Test-Path $done)) { "FAIL (the task did not finish in 4 min) - $tlog" }
                elseif ($out -match '^SKIP') { "NOT-RUN (foreground: the desktop was in use at its start)" }
                elseif ($total -match ' 0 failed') { "PASS ($total)" } else { "FAIL ($total) - $tlog" }
            Say "focus-probe (task): $($rows['focus-probe (task)']); the task $name was removed"
        }
    }
}
finally {
    [void][CatchupPower]::SetThreadExecutionState([uint32]"0x80000000")
    $w = [Math]::Max(4, [int](($rows.Keys | ForEach-Object { $_.Length } | Measure-Object -Maximum).Maximum))   # (no item selected: an empty table)
    $body = @("windows-catchup.ps1, started $stamp, ended $(Get-Date -Format 'yyyyMMdd-HHmm')") + $head + @("", ("{0,-$w}  {1}" -f "item", "result"), ("{0,-$w}  {1}" -f ("-" * $w), "------"))
    foreach ($k in $rows.Keys) { $body += ("{0,-$w}  {1}" -f $k, $rows[$k]) }
    $body += @("", "== DIAG lines (scripts\e2e\windows\lib.ps1)")
    foreach ($f in Get-ChildItem $logs -Filter *.log -ErrorAction SilentlyContinue) { Get-Content $f.FullName | Where-Object { $_ -match '^DIAG ' } | ForEach-Object { $body += "$($f.BaseName): $_" } }
    foreach ($k in $rows.Keys) {
        if ($rows[$k] -match '^FAIL') { $f = Join-Path $logs "$(($k -replace ' \(task\)', '-task')).log"; $body += @("", "== $k ($f)"); if (Test-Path $f) { $body += Get-Content $f } }
    }
    $body | Set-Content $result -Encoding utf8
    Copy-Item $result (Join-Path $OutDir "latest.txt") -Force
    Write-Host ""; $body | Select-Object -First ($rows.Count + 6 + $head.Count) | ForEach-Object { Write-Host $_ }
    Write-Host "result file: $result"
}
