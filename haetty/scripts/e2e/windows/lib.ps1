# Dot-sourced by the Windows end-to-end scripts. An isolated dev instance of haetty (own pipe, state, config, log)
# in a test run directory (test-guard.ps1), a fake `claude` (powershell.exe copied as claude.exe: Claude Code's
# integration applies to programs of that name) that logs its folder and arguments and records every key it reads,
# key input through SendKeys (SendInput) only while the haetty window is in the foreground, and checks.
#   Required: $Exe (haetty.exe). The daemon is stopped by the PID `status --json` reports, never by name.
$ErrorActionPreference = "Continue"
. (Join-Path $PSScriptRoot "..\..\test-guard.ps1")
foreach ($h in (Join-Path $PSScriptRoot "..\..\..\tools\heavy.ps1"), (Join-Path $PSScriptRoot "heavy.ps1")) { if (Test-Path $h) { . $h; break } }
# This file is dot-sourced: `exit` here would not stop the calling script (it went on without the functions
# below and wrote outside its run folder, V6) — so `throw`. A caller holding a slot already needs no heavy.ps1.
if (Get-Command Enter-HaettyHeavy -ErrorAction SilentlyContinue) { Enter-HaettyHeavy (Split-Path -Leaf $MyInvocation.PSCommandPath) }
elseif (-not $env:HAETTY_HEAVY_SLOT) { throw "lib.ps1: tools\heavy.ps1 not found (and no heavy-job slot held)" }   # at most two heavy jobs, Linux and Windows together (tools/heavy.ps1)
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class HW {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [StructLayout(LayoutKind.Sequential)] public struct LII { public uint cbSize; public uint dwTime; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LII p);
}
"@
$script:P = 0; $script:F = 0
function Ok($what) { $script:P++; Write-Host "PASS $what" }
function Bad($what, $why) { $script:F++; Write-Host "FAIL $what :: $why" }
function Total($name) { Write-Host "TOTAL ${name}: $script:P passed, $script:F failed" }

# These scripts type into a haetty window with SendInput, which goes to whatever window is in the foreground.
# So they run only on a desktop nobody is using: no input for $MinIdle seconds (else SKIP, exit 77), and they stop
# for good — window and daemon closed, exit 2 — the moment the haetty window is not in the foreground.
$script:MinIdle = 120
function Idle-Seconds { $l = New-Object HW+LII; $l.cbSize = 8; [void][HW]::GetLastInputInfo([ref]$l); [int](([Environment]::TickCount - $l.dwTime) / 1000) }
function Last-Input { $l = New-Object HW+LII; $l.cbSize = 8; [void][HW]::GetLastInputInfo([ref]$l); $l.dwTime }
function Require-Idle {
    $i = Idle-Seconds
    if ($i -lt $script:MinIdle) { Write-Host "SKIP: the desktop is in use (last input $i s ago; needs $script:MinIdle s idle)"; exit 77 }
    $script:LastSent = Last-Input
}
# Input since our own last keys (GetLastInputInfo counts SendInput too: only what came after it, plus a margin):
# somebody uses the computer — stop before sending anything more.
function Someone-Active { [int64](Last-Input) - [int64]$script:LastSent -gt 1500 }

function New-HaettyRun([string]$Name) {
    Require-Idle
    $run = Initialize-HaettyGuard -Name HAETTY_TEST_TMP
    $w = Join-Path $run $Name; Assert-HaettyPath $w
    foreach ($d in "$w", "$w\state\tabs", "$w\cfg", "$w\bin", "$w\home", "$w\claude") { New-Item -ItemType Directory -Force $d | Out-Null }
    Get-ChildItem env: | Where-Object { $_.Name -match '^CLAUDE_CODE_|^CLAUDECODE$|^CLAUDE_.*SESSION' } | ForEach-Object { Remove-Item "env:$($_.Name)" }
    $env:HAETTY_INSTANCE = "dev"; $env:HAETTY_SOCKET = "\\.\pipe\haetty-e2e-$Name-$PID"
    $env:HAETTY_STATE_DIR = "$w\state"; $env:HAETTY_CONFIG_DIR = "$w\cfg"; $env:HAETTY_LOG = "debug"; $env:HAETTY_LOG_FILE = "$w\gui.log"
    $env:CLAUDE_CONFIG_DIR = "$w\claude"; $env:USERPROFILE = "$w\home"; $env:HOME = "$w\home"
    $env:GIT_CONFIG_NOSYSTEM = "1"; $env:GIT_CONFIG_GLOBAL = "NUL"
    $ps = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
    Copy-Item $ps "$w\bin\claude.exe"
    # The fake Claude Code: logs PID, folder, arguments; then appends every key's character.
    @"
`$a = (`$args | ForEach-Object { '[' + ((`$_ -split "``n")[0]) + ']' }) -join ''
[IO.File]::AppendAllText('$w\claude-starts.log', "`$PID|`$PWD|`$a``n")
while (`$true) { `$k = [Console]::ReadKey(`$true); [IO.File]::AppendAllText('$w\claude-input.txt', [string]`$k.KeyChar) }
"@ | Set-Content -Path "$w\fake.ps1" -Encoding utf8
    $w
}

# The fake claude as (program, args).
function Fake-Claude([string]$W) { ,@("$W\bin\claude.exe", @("-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", "$W\fake.ps1")) }

function Start-Gui([string]$Program, [string[]]$Arguments, [string]$Cwd, [string[]]$Extra = @()) {
    $al = @("--cols", "100", "--rows", "26", "--restore-tabs", "false") + $Extra + @($Program) + $Arguments
    $script:Gui = Start-Process -FilePath $Exe -ArgumentList $al -WorkingDirectory $Cwd -PassThru
    for ($i = 0; $i -lt 60; $i++) { Start-Sleep -Milliseconds 250; $script:Gui.Refresh(); if ($script:Gui.MainWindowHandle -ne 0) { break } }
    Start-Sleep 2
}

function Ours { $fg = [HW]::GetForegroundWindow(); $fp = 0; [void][HW]::GetWindowThreadProcessId($fg, [ref]$fp); $fp -eq $script:Gui.Id }
function Front {
    for ($i = 0; $i -lt 10; $i++) {
        if (Ours) { return $true }
        [HW]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [HW]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)  # Alt: allows SetForegroundWindow
        $script:LastSent = Last-Input
        [void][HW]::ShowWindow($script:Gui.MainWindowHandle, 9); [void][HW]::SetForegroundWindow($script:Gui.MainWindowHandle)
        Start-Sleep -Milliseconds 300
    }
    Ours
}
# SendKeys syntax: ^ Ctrl, + Shift, % Alt, {ENTER}, {BS}; literal text must not contain + ^ % ~ ( ) { } [ ].
function Keys([string]$k, [int]$WaitMs = 600) {
    if (Someone-Active) {
        Write-Host "STOP: keyboard or mouse input from somebody else; no more keys are sent"
        Stop-Gui; Total "aborted"; exit 2
    }
    if (-not (Front)) {
        Bad "focus" "the haetty window is not in the foreground: stopping without sending more keys ($k)"
        Stop-Gui; Total "aborted"; exit 2
    }
    [System.Windows.Forms.SendKeys]::SendWait($k); $script:LastSent = Last-Input; Start-Sleep -Milliseconds $WaitMs
}
function Palette([string]$Filter) { Keys "^+p"; Keys $Filter 300; Keys "{ENTER}" }

# haetty.exe as a hidden child (no console window: a `cmd /c` window takes the foreground from the haetty window
# and keys sent then are lost), stdin written directly (a PowerShell pipe into the GUI-subsystem exe does not
# deliver; README Windows notes). Returns stdout.
function Run-Haetty([string[]]$Arguments, [string]$Stdin = $null, [hashtable]$Env = @{}) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $Exe; foreach ($a in $Arguments) { [void]$psi.ArgumentList.Add($a) }
    $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true; $psi.RedirectStandardInput = $true
    foreach ($k in $Env.Keys) { $psi.Environment[$k] = $Env[$k] }
    $p = [System.Diagnostics.Process]::Start($psi)
    if ($Stdin) { $p.StandardInput.Write($Stdin) }
    $p.StandardInput.Close()
    $out = $p.StandardOutput.ReadToEnd(); [void]$p.WaitForExit(10000)
    $out
}
function Status { Run-Haetty @("status", "--json") | ConvertFrom-Json }
function Running { @((Status).sessions | Where-Object { $_.state -eq "running" }) }
function Last { @(Get-Content "$env:HAETTY_STATE_DIR\tabs\last.tsv" -Encoding utf8 -ErrorAction SilentlyContinue | Where-Object { $_ -and -not $_.StartsWith("#") }) }
function LastNames { (Last | ForEach-Object { $f = $_ -split "`t"; "$($f[0])|$($f[6])" }) -join ";" }
# `haetty hook EVENT` / `haetty statusline` for a session, with JSON on stdin.
function Report([string]$Session, [string]$Kind, [string]$Json) {
    [void](Run-Haetty ($Kind -split ' ') $Json @{ HAETTY_SESSION_ID = $Session })
}
function Stop-Gui {
    $d = $null; try { $d = (Status).daemon_pid } catch {}
    if ($script:Gui -and -not $script:Gui.HasExited) { Stop-Process -Id $script:Gui.Id -ErrorAction SilentlyContinue }
    Start-Sleep 1
    if ($d) { Stop-Process -Id $d -ErrorAction SilentlyContinue }   # this run's daemon, by its PID
    Start-Sleep 1
}
# Not named Git: PowerShell names are case-insensitive, so `& git` inside would call the function itself.
$script:GitExe = (Get-Command git.exe -CommandType Application -ErrorAction Stop | Select-Object -First 1).Source
function RepoGit([string]$Repo) { & $script:GitExe -C $Repo -c user.name=t -c user.email=t@example.invalid -c commit.gpgsign=false @args 2>$null }
