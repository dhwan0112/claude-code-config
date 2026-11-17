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
using System; using System.Text; using System.Runtime.InteropServices;
public static class HW {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", SetLastError = true)] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
  [StructLayout(LayoutKind.Sequential)] public struct LII { public uint cbSize; public uint dwTime; }
  [DllImport("user32.dll")] public static extern bool GetLastInputInfo(ref LII p);
  // Stage W1: the pointer (windows\settings.ps1) and what the focus diagnostics read. Nothing here sends input
  // except SetCursorPos and mouse_event, which the scripts call only through Click-At / Move-To.
  [StructLayout(LayoutKind.Sequential)] public struct PT { public int x; public int y; }
  [StructLayout(LayoutKind.Sequential)] public struct RC { public int l; public int t; public int r; public int b; }
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RC r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref PT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool SystemParametersInfoW(uint action, uint p, ref uint v, uint f);
  [DllImport("user32.dll")] static extern IntPtr GetProcessWindowStation();
  [DllImport("user32.dll")] static extern IntPtr GetThreadDesktop(uint tid);
  [DllImport("user32.dll", SetLastError = true)] static extern IntPtr OpenInputDesktop(uint flags, bool inherit, uint access);
  [DllImport("user32.dll")] static extern bool CloseDesktop(IntPtr h);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern bool GetUserObjectInformationW(IntPtr h, int index, StringBuilder s, int n, out int needed);
  [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
  [DllImport("kernel32.dll")] public static extern uint WTSGetActiveConsoleSessionId();
  [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, uint pid);
  [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool OpenProcessToken(IntPtr p, uint access, out IntPtr tok);
  [DllImport("advapi32.dll", SetLastError = true)] static extern bool GetTokenInformation(IntPtr tok, int cls, IntPtr buf, uint len, out uint ret);
  [DllImport("advapi32.dll")] static extern IntPtr GetSidSubAuthorityCount(IntPtr sid);
  [DllImport("advapi32.dll")] static extern IntPtr GetSidSubAuthority(IntPtr sid, uint n);
  public static string ClassName(IntPtr h) { StringBuilder s = new StringBuilder(128); GetClassNameW(h, s, 128); return s.ToString(); }
  public static uint ForegroundLockTimeout() { uint v = 0; SystemParametersInfoW(0x2000, 0, ref v, 0); return v; }
  static string ObjName(IntPtr h) { StringBuilder s = new StringBuilder(256); int n; return GetUserObjectInformationW(h, 2, s, 512, out n) ? s.ToString() : "?"; }
  public static string Station() { return ObjName(GetProcessWindowStation()) + "\\" + ObjName(GetThreadDesktop(GetCurrentThreadId())); }
  // The desktop that receives input now: "Default" for the user's, "Winlogon" (or not readable) while locked.
  public static string InputDesktop() {
    IntPtr d = OpenInputDesktop(0, false, 0x0001);
    if (d == IntPtr.Zero) return "not readable (error " + Marshal.GetLastWin32Error() + ")";
    string n = ObjName(d); CloseDesktop(d); return n;
  }
  // low / medium / high / system: a medium process cannot put keys into, or take the foreground from, a high one.
  public static string Integrity(uint pid) {
    IntPtr p = OpenProcess(0x1000, false, pid);
    if (p == IntPtr.Zero) return "unknown (OpenProcess error " + Marshal.GetLastWin32Error() + ")";
    try {
      IntPtr tok;
      if (!OpenProcessToken(p, 0x0008, out tok)) return "unknown (OpenProcessToken error " + Marshal.GetLastWin32Error() + ")";
      try {
        uint len; GetTokenInformation(tok, 25, IntPtr.Zero, 0, out len);
        IntPtr buf = Marshal.AllocHGlobal((int)len);
        try {
          if (!GetTokenInformation(tok, 25, buf, len, out len)) return "unknown (GetTokenInformation error " + Marshal.GetLastWin32Error() + ")";
          IntPtr sid = Marshal.ReadIntPtr(buf);
          int n = Marshal.ReadByte(GetSidSubAuthorityCount(sid));
          int rid = Marshal.ReadInt32(GetSidSubAuthority(sid, (uint)(n - 1)));
          return rid >= 0x4000 ? "system" : rid >= 0x3000 ? "high" : rid >= 0x2000 ? "medium" : "low";
        } finally { Marshal.FreeHGlobal(buf); }
      } finally { CloseHandle(tok); }
    } finally { CloseHandle(p); }
  }
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

# Focus diagnostics (stage W1; the night runs of 2026-11-10 and 11-11 stopped at their first focus check and left
# no record of the desktop's state). One line, written to the host (never to the output a caller captures): who
# runs the script (integrity, session, window station and desktop), which window has the foreground (process,
# integrity, window class — never its title: titles can hold what the person was doing), whether the screen is
# locked, the desktop that takes input, the idle time, the foreground lock timeout, the display's idle timeout.
# Reading only: it sends nothing and moves no window.
function Display-Timeout {
    if ($null -ne $script:DisplayTimeout) { return $script:DisplayTimeout }
    $v = @(& powercfg.exe /q SCHEME_CURRENT SUB_VIDEO VIDEOIDLE 2>$null | Select-String '0x[0-9a-fA-F]{8}\s*$' | ForEach-Object { [Convert]::ToInt32(($_.Line -replace '.*(0x[0-9a-fA-F]{8})\s*$', '$1'), 16) })
    $script:DisplayTimeout = if ($v.Count -ge 2) { "AC $($v[$v.Count - 2]) s, DC $($v[$v.Count - 1]) s" } else { "unknown" }
    $script:DisplayTimeout
}
function Focus-Diag([string]$Where) {
    $me = [Diagnostics.Process]::GetCurrentProcess()
    $fg = [HW]::GetForegroundWindow(); $fp = 0; [void][HW]::GetWindowThreadProcessId($fg, [ref]$fp)
    $fn = if ($fp) { try { (Get-Process -Id $fp -ErrorAction Stop).ProcessName } catch { "?" } } else { "none" }
    $fi = if ($fp) { [HW]::Integrity($fp) } else { "-" }
    $ours = if ($script:Gui) { "pid $($script:Gui.Id) hwnd $($script:Gui.MainWindowHandle) minimized $([HW]::IsIconic($script:Gui.MainWindowHandle)) integrity $([HW]::Integrity($script:Gui.Id))" } else { "not started" }
    $locked = [bool](Get-Process -Name LogonUI -ErrorAction SilentlyContinue)
    Write-Host ("DIAG {0}: script pid {1} integrity {2} session {3} (console session {4}, remote {5}) station {6}; foreground hwnd {7} = {8} (pid {9}, integrity {10}, class {11}); haetty window: {12}; ours in front: {13}; locked {14}; input desktop {15}; idle {16} s; foreground lock timeout {17} ms; display timeout {18}" -f
        $Where, $PID, [HW]::Integrity($PID), $me.SessionId, [HW]::WTSGetActiveConsoleSessionId(), ([HW]::GetSystemMetrics(0x1000) -ne 0), [HW]::Station(),
        $fg, $fn, $fp, $fi, $(if ($fg -ne [IntPtr]::Zero) { [HW]::ClassName($fg) } else { "-" }), $ours, $(if ($script:Gui) { Ours } else { "-" }), $locked, [HW]::InputDesktop(), (Idle-Seconds), [HW]::ForegroundLockTimeout(), (Display-Timeout))
}

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
    Focus-Diag "start of $Name"
    $w
}

# The fake claude as (program, args).
function Fake-Claude([string]$W) { ,@("$W\bin\claude.exe", @("-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", "$W\fake.ps1")) }

# HAETTY_E2E_ENGINE (tools/overnight-soak.ps1 -Engine): every GUI of the run on that engine (`--engine`, the GUI's
# setting from its command line: the scripts write their own config files).
function Engine-Args { if ($env:HAETTY_E2E_ENGINE) { @("--engine", $env:HAETTY_E2E_ENGINE) } else { @() } }

function Start-Gui([string]$Program, [string[]]$Arguments, [string]$Cwd, [string[]]$Extra = @()) {
    $al = @("--cols", "100", "--rows", "26", "--restore-tabs", "false") + (Engine-Args) + $Extra + @($Program) + $Arguments
    $script:Gui = Start-Process -FilePath $Exe -ArgumentList $al -WorkingDirectory $Cwd -PassThru
    for ($i = 0; $i -lt 60; $i++) { Start-Sleep -Milliseconds 250; $script:Gui.Refresh(); if ($script:Gui.MainWindowHandle -ne 0) { break } }
    Start-Sleep 2
    Focus-Diag "window up"
}

function Ours { $fg = [HW]::GetForegroundWindow(); $fp = 0; [void][HW]::GetWindowThreadProcessId($fg, [ref]$fp); $fp -eq $script:Gui.Id }
function Front {
    $tries = @()
    for ($i = 0; $i -lt 10; $i++) {
        if (Ours) { return $true }
        [HW]::keybd_event(0x12, 0, 0, [UIntPtr]::Zero); [HW]::keybd_event(0x12, 0, 2, [UIntPtr]::Zero)  # Alt: allows SetForegroundWindow
        $script:LastSent = Last-Input
        [void][HW]::ShowWindow($script:Gui.MainWindowHandle, 9)
        $ok = [HW]::SetForegroundWindow($script:Gui.MainWindowHandle); $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        $tries += "$ok/$err"
        Start-Sleep -Milliseconds 300
    }
    if (Ours) { return $true }
    # Not in front after ten tries: what the desktop looked like (SetForegroundWindow's result/last error per try).
    Write-Host "DIAG SetForegroundWindow tries (result/error): $($tries -join ' ')"
    Focus-Diag "focus lost"
    $false
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

# The pointer (windows\settings.ps1): the same rules as Keys — nothing while somebody else types or moves the mouse,
# nothing unless the haetty window is in front. ($X, $Y) are pixels in the window's client area.
function Client-Size { $r = New-Object HW+RC; [void][HW]::GetClientRect($script:Gui.MainWindowHandle, [ref]$r); @(($r.r - $r.l), ($r.b - $r.t)) }
function Move-To([int]$X, [int]$Y, [int]$WaitMs = 400) {
    if (Someone-Active) { Write-Host "STOP: keyboard or mouse input from somebody else; the pointer is not moved"; Stop-Gui; Total "aborted"; exit 2 }
    if (-not (Front)) { Bad "focus" "the haetty window is not in the foreground: the pointer is not moved"; Stop-Gui; Total "aborted"; exit 2 }
    $p = New-Object HW+PT; $p.x = $X; $p.y = $Y; [void][HW]::ClientToScreen($script:Gui.MainWindowHandle, [ref]$p)
    [void][HW]::SetCursorPos($p.x, $p.y)
    [HW]::mouse_event(0x0001, 0, 0, 0, [UIntPtr]::Zero)   # a zero move: the window gets a pointer-moved message
    $script:LastSent = Last-Input; Start-Sleep -Milliseconds $WaitMs
}
function Click-At([int]$X, [int]$Y, [int]$WaitMs = 600) {
    Move-To $X $Y 200
    [HW]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero); [HW]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)   # left down, up
    $script:LastSent = Last-Input; Start-Sleep -Milliseconds $WaitMs
}

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
