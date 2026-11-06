# Overnight soak v2 (HANDOFF.md §6.1 item 30): start it once before going to bed, from a PowerShell window — not
# from Claude Code — with nothing else heavy running:
#   pwsh -NoProfile -ExecutionPolicy Bypass -File \\wsl.localhost\<distro>\<repo>\haetty\tools\overnight-soak.ps1
# (-ExecutionPolicy Bypass: the script on \\wsl.localhost is unsigned, and the default RemoteSigned refuses it.)
# It
#   1. checks that no other heavy job runs (heavy-job slots, cargo/rustc/zig/Xvfb in WSL) and then holds both
#      slots for the whole night, so nothing else can start next to it;
#   2. builds -Commit (default 63822de, the 0.2.0 candidate) in its own checkout, for Linux and Windows;
#   3. runs the Linux soak v2 alone in WSL for -Minutes (default 60);
#   4. then waits (up to -IdleWaitMinutes, default 240) for a desktop nobody has touched for 120 s and runs the
#      Windows soak v2 for -Minutes; any keyboard or mouse input from you stops it at once (no more keys are
#      sent) and it counts as not passed;
#   5. writes both verdicts and both time series into one file: -OutDir\result-<date-time>.txt, also copied to
#      -OutDir\latest.txt (default C:\Users\Public\haetty-overnight);
#   6. keeps the computer from sleeping (and the screen from locking, which would stop the Windows part) only
#      while it runs; normal power settings return when it ends.
# The first lines of the result file are the verdicts: "RESULT LINUX PASS|FAIL|NOT-RUN …" and the same for
# WINDOWS. Progress goes to -OutDir\progress.log.
param(
    [string]$Commit = "63822de",
    [int]$Minutes = 60,
    [int]$IdleWaitMinutes = 240,
    [string]$OutDir = (Join-Path $env:PUBLIC "haetty-overnight"),
    # Inside WSL, relative to its home (PowerShell would expand a leading ~ itself): an Xvfb without root
    # (scripts/e2e/README.md); unused where Xvfb is installed.
    [string]$XRoot = "m2/xroot"
)
$ErrorActionPreference = "Continue"
. (Join-Path $PSScriptRoot "heavy.ps1")
$d, $sh = Get-HaettyHeavySh
$lsh = $sh -replace '/heavy\.sh$', '/overnight-soak-linux.sh'
New-Item -ItemType Directory -Force $OutDir | Out-Null
$stamp = Get-Date -Format "yyyyMMdd-HHmm"
$result = Join-Path $OutDir "result-$stamp.txt"; $plog = Join-Path $OutDir "progress.log"
function Say([string]$m) { $l = "$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss') $m"; Write-Host $l; Add-Content $plog $l }
function L([string[]]$a) { & wsl.exe -d $d --exec bash $lsh @a 2>&1 | ForEach-Object { "$_" } }
$lin = "NOT-RUN"; $win = "NOT-RUN"; $linOut = @(); $winOut = @(); $ver = @()

Add-Type @"
using System; using System.Runtime.InteropServices;
public static class OvernightPower {
  [DllImport("kernel32.dll")] public static extern uint SetThreadExecutionState(uint f);
  [StructLayout(LayoutKind.Sequential)] public struct LII { public uint cbSize; public uint dwTime; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LII p);
}
"@
function Idle { $l = New-Object OvernightPower+LII; $l.cbSize = 8; [void][OvernightPower]::GetLastInputInfo([ref]$l); [int](([Environment]::TickCount - $l.dwTime) / 1000) }
function Locked { [bool](Get-Process -Name LogonUI -ErrorAction SilentlyContinue) }

$holds = @()
try {
    Say "overnight soak: commit $Commit, $Minutes min each, results in $result"
    # 1. Nothing else heavy; then both slots are ours.
    Remove-Item env:HAETTY_HEAVY_SLOT -ErrorAction SilentlyContinue
    $chk = @(L @("check"))
    if ($LASTEXITCODE -ne 0) { $lin = "NOT-RUN (other heavy work running)"; $win = $lin; $linOut = $chk; Say ($chk -join "; "); return }
    $holds += Start-HaettyHeavyHold "overnight soak (1/2)"; $holds += Start-HaettyHeavyHold "overnight soak (2/2)"
    $env:HAETTY_HEAVY_SLOT = "overnight"
    $env:WSLENV = (@(($env:WSLENV -split ':') | Where-Object { $_ -and $_ -ne 'HAETTY_HEAVY_SLOT' }) + "HAETTY_HEAVY_SLOT") -join ':'
    # 0x80000000 continuous | 0x1 system required (no sleep) | 0x2 display required (no screen off or lock)
    $prev = [OvernightPower]::SetThreadExecutionState([uint32]"0x80000003")
    Say "slots held; sleep and screen lock prevented while this runs (SetThreadExecutionState: $(if ($prev -ne 0) { 'accepted' } else { 'FAILED' }))"

    # 2. The build of the commit.
    Say "building $Commit (Linux and Windows)"
    $ver = @(L @("build", $Commit, $OutDir))
    $exe = Join-Path $OutDir "bin\haetty.exe"
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $exe)) { $lin = "NOT-RUN (build failed)"; $win = $lin; $linOut = $ver; Say "build failed"; return }
    $ver += "windows: $((& $exe --version | Out-String).Trim())"
    $ver | ForEach-Object { Say $_ }

    # 3. Linux, alone.
    Say "Linux soak v2: $Minutes min"
    Remove-Item (Join-Path $OutDir "linux.csv") -ErrorAction SilentlyContinue
    $linOut = @(L @("soak", "$Minutes", $XRoot, $OutDir))
    $v = $linOut | Where-Object { $_ -match '^VERDICT ' } | Select-Object -Last 1
    $lin = if ($v -match 'VERDICT PASS') { "PASS" } elseif ($v) { "FAIL" } else { "FAIL (no verdict: $(($linOut | Select-Object -Last 1)))" }
    Say "Linux: $lin"

    # 4. Windows, on a desktop nobody uses.
    $until = (Get-Date).AddMinutes($IdleWaitMinutes)
    while ((Get-Date) -lt $until -and ((Idle) -lt 120 -or (Locked))) { Start-Sleep 20 }
    if ((Idle) -lt 120 -or (Locked)) {
        $win = "NOT-RUN ($(if (Locked) { 'the screen was locked' } else { 'the desktop was in use' }) for $IdleWaitMinutes min)"; Say "Windows: $win"; return
    }
    Say "Windows soak v2: $Minutes min (desktop idle $(Idle) s)"
    $wsrc = "\\wsl.localhost\$d\tmp\haetty-overnight\wt\haetty\scripts"
    $wdst = Join-Path $OutDir "scripts"; Remove-Item -Recurse -Force $wdst -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force "$wdst\e2e\windows" | Out-Null
    Copy-Item "$wsrc\test-guard.ps1" $wdst; Copy-Item "$wsrc\e2e\windows\*.ps1" "$wdst\e2e\windows"
    $env:HAETTY_TEST_TMP = Join-Path $env:PUBLIC "haetty-test"
    $wcsv = Join-Path $OutDir "windows.csv"; Remove-Item $wcsv -ErrorAction SilentlyContinue
    $winOut = & pwsh -NoProfile -ExecutionPolicy Bypass -File "$wdst\e2e\windows\soak.ps1" -Exe $exe -Out $wcsv -Dur ($Minutes * 60) 2>&1 | ForEach-Object { "$_" }
    $rc = $LASTEXITCODE
    $v = $winOut | Where-Object { $_ -match '^VERDICT ' } | Select-Object -Last 1
    $win = if ($v -match 'VERDICT PASS') { "PASS" } elseif ($rc -eq 2) { "FAIL (stopped: input from somebody, or the window lost the foreground)" } elseif ($rc -eq 77) { "NOT-RUN (desktop in use at the start)" } elseif ($v) { "FAIL" } else { "FAIL (no verdict, exit $rc)" }
    Say "Windows: $win"
}
finally {
    [void][OvernightPower]::SetThreadExecutionState([uint32]"0x80000000")
    foreach ($h in $holds) { try { $h.StandardInput.Close(); [void]$h.WaitForExit(15000) } catch {} }
    Remove-Item env:HAETTY_HEAVY_SLOT -ErrorAction SilentlyContinue
    [void](L @("clean"))
    $csv = { param($f) if (Test-Path $f) { Get-Content $f } else { "(none)" } }
    $body = @("RESULT LINUX $lin", "RESULT WINDOWS $win", "commit: $Commit; $Minutes min each; started $stamp, ended $(Get-Date -Format 'yyyyMMdd-HHmm')") + $ver +
        @("", "== Linux soak v2 (scripts/e2e/soak.sh)") + $linOut + @("-- linux.csv") + (& $csv (Join-Path $OutDir "linux.csv")) +
        @("", "== Windows soak v2 (scripts/e2e/windows/soak.ps1)") + $winOut + @("-- windows.csv") + (& $csv (Join-Path $OutDir "windows.csv"))
    $body | Set-Content $result -Encoding utf8
    Copy-Item $result (Join-Path $OutDir "latest.txt") -Force
    Say "done: LINUX $lin, WINDOWS $win; $result"
}
