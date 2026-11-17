# Windows: the in-app update end to end (the Linux update.sh, stage PU, DESIGN.md 2.4 and 3.17). What it covers that
# Linux cannot: Schannel with the test CA (HAETTY_UPDATE_TEST_ROOT), the junction `current` (form a), the Start
# menu shortcut through `current`, and replacing the running haetty.exe (form b).
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"
#   pwsh -File update.ps1 -Old C:\...\haetty-0.90.0.exe -New C:\...\haetty-0.90.1.exe -Ca C:\...\TEST-ONLY-ca.pem
#        [-WslServer Ubuntu:/home/<you>/…/haetty/target/debug/examples/fake_release_server] [-Server C:\...\start-server.cmd]
#        [-SetupOnly]
# Old and New are `--features update-test` builds with HAETTY_BUILD_VERSION 0.90.0 / 0.90.1 and HAETTY_BUILD_COMMIT
# aaaaaaaaaaaa0000000000000000000000000000 / bbbbbbbbbbbb0000000000000000000000000000 (the same as update.sh).
# The release server is Linux-only (rustls is not in the Windows build): `fake_release_server` runs in WSL on this
# script's folder $W\srv (as /mnt/<drive>/…); WSL 2 forwards localhost, which the TEST-ONLY certificate names.
# -WslServer names it; without it the path comes from HAETTY_HEAVY_SH (<distro>:<…>/tools/heavy.sh). -Server instead
# starts a program of yours with the folder as its argument.
# Form (a): 0.90.0 as a version folder and junction -> the panel's Download and install -> Restart now -> 0.90.1 runs
# with the tab back -> Roll back -> 0.90.0 again; a version that ends at once (0.90.2) is rolled back by the restart's
# watch. Form (b): a single haetty.exe replaced while it runs, haetty.exe.old gone at the next start. The windows run
# without HAETTY_INSTANCE (the update needs the stable instance's rules) with every folder inside the run directory:
# own pipe, state, config, LOCALAPPDATA and APPDATA. The real install folder and Start menu are compared before
# and after. -SetupOnly: everything up to the first window (no key is sent), to check the plumbing.
# The haetty window must stay in the foreground while it runs.
param(
    [Parameter(Mandatory = $true)][string]$Old,
    [Parameter(Mandatory = $true)][string]$New,
    [Parameter(Mandatory = $true)][string]$Ca,
    [string]$WslServer,
    [string]$Server,
    [switch]$SetupOnly
)
. (Join-Path $PSScriptRoot "lib.ps1")
$W = New-HaettyRun "update"
if (-not $W) { throw "update.ps1: no run folder (lib.ps1 failed)" }
$A = "aaaaaaaaaaaa0000000000000000000000000000"; $B = "bbbbbbbbbbbb0000000000000000000000000000"; $C = "cccccccccccc0000000000000000000000000000"
# The real install and Start menu, before (the real profile was kept by test-guard.ps1 before anything was redirected).
$realRoot = Join-Path $env:HAETTY_REAL_LOCALAPPDATA "Programs\haetty"
$realLnk = Join-Path $env:HAETTY_REAL_APPDATA "Microsoft\Windows\Start Menu\Programs\haetty.lnk"
function Real-State {
    $r = if (Test-Path -LiteralPath $realRoot) { (Get-ChildItem -LiteralPath $realRoot -Force | ForEach-Object { "$($_.Name)@$($_.LastWriteTimeUtc.Ticks)" }) -join "," } else { "absent" }
    $l = if (Test-Path -LiteralPath $realLnk) { (Get-Item -LiteralPath $realLnk).LastWriteTimeUtc.Ticks } else { "absent" }
    "$r|$l"
}
$real0 = Real-State
# This run's world: no instance name, every folder in $W.
Remove-Item env:HAETTY_INSTANCE -ErrorAction SilentlyContinue
foreach ($d in "$W\local", "$W\roaming", "$W\srv", "$W\pack", "$W\stage", "$W\single") { New-Item -ItemType Directory -Force $d | Out-Null }
$env:LOCALAPPDATA = "$W\local"; $env:APPDATA = "$W\roaming"
$env:HAETTY_UPDATE_TEST = "1"; $env:HAETTY_UPDATE_TEST_FIRST_MS = "1500"; $env:HAETTY_UPDATE_TEST_ROOT = $Ca
foreach ($k in "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY") { Remove-Item "env:$k" -ErrorAction SilentlyContinue }
$OPT = "$W\local\Programs\haetty"; $LINK = "$OPT\current\haetty.exe"

# A program's stdout through a pipe of our own (haetty.exe is a GUI-subsystem program).
function Run-Exe([string]$File, [string[]]$Arguments) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $File; foreach ($a in $Arguments) { [void]$psi.ArgumentList.Add($a) }
    $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true; $psi.RedirectStandardOutput = $true; $psi.RedirectStandardError = $true
    $p = [System.Diagnostics.Process]::Start($psi)
    $o = $p.StandardOutput.ReadToEnd(); $e = $p.StandardError.ReadToEnd(); [void]$p.WaitForExit(30000)
    @{ rc = $p.ExitCode; out = $o; err = $e }
}
function Version-Of([string]$File) { ((Run-Exe $File @("--version")).out).Trim() }
function To-Wsl([string]$p) { "/mnt/" + $p.Substring(0, 1).ToLower() + ($p.Substring(2) -replace '\\', '/') }

# 0.90.2: a program that answers --version correctly and ends at once otherwise (the install's own check passes;
# the restart's watch must notice). Compiled here with Windows PowerShell 5.1 (it can write an .exe).
$dying = "$W\haetty-0.90.2.exe"
$src = 'public static class P { public static int Main(string[] a) { if (a.Length > 0 && a[0] == "--version") { System.Console.WriteLine("haetty 0.90.2 (commit cccccccccccc, built 2026-11-20, instance stable)"); return 0; } return 3; } }'
& (Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe") -NoProfile -Command "Add-Type -TypeDefinition '$src' -OutputAssembly '$dying' -OutputType ConsoleApplication" 2>&1 | Out-Null
if ((Test-Path $dying) -and (Version-Of $dying) -match "0\.90\.2 \(commit cccccccccccc") { Ok "the stand-in for 0.90.2 (ends at once) was made" } else { Bad "stand-in 0.90.2" "not built" }

# The server's release: haetty.exe and LICENSE in a zip, the manifest written by New itself.
function Release([string]$Ver, [string]$Commit, [string]$Binary) {
    Remove-Item "$W\pack\*" -Force -ErrorAction SilentlyContinue
    Copy-Item $Binary "$W\pack\haetty.exe"; Set-Content "$W\pack\LICENSE" "MIT" -Encoding ascii
    Get-ChildItem "$W\srv" -Filter "haetty-*" | Remove-Item -Force
    $zip = "$W\srv\haetty-$Ver-x86_64-windows.zip"
    Compress-Archive -Path "$W\pack\haetty.exe", "$W\pack\LICENSE" -DestinationPath $zip
    Set-Content "$W\notes.md" "### Fixed`n- release $Ver" -Encoding ascii
    $m = Run-Exe $New @("release-manifest", "--version", $Ver, "--commit", $Commit, "--built", "2026-11-20T00:00:00Z", "--protocol", "12", "--notes", "$W\notes.md", "--asset", "x86_64-windows=${zip}:haetty.exe,LICENSE")
    if ($m.rc -ne 0 -or -not $m.out) { Bad "release-manifest $Ver" "$($m.rc) $($m.err)" }
    [IO.File]::WriteAllText("$W\srv\manifest.json", $m.out)
    Set-Content "$W\srv\reload" "x"; Start-Sleep -Milliseconds 900
}
Release "0.90.1" $B $New

# The release server.
$srvProc = $null; $wslDistro = $null
if ($Server) { $srvProc = Start-Process $Server -ArgumentList "$W\srv" -PassThru -WindowStyle Hidden }
else {
    if (-not $WslServer -and $env:HAETTY_HEAVY_SH -match '^([^:]+):(/.+)/tools/heavy\.sh$') { $WslServer = "$($Matches[1]):$($Matches[2])/target/debug/examples/fake_release_server" }
    if ($WslServer -notmatch '^([^:]+):(/.+)$') { throw "update.ps1: give -WslServer <distro>:<path of fake_release_server> (or -Server)" }
    $wslDistro = $Matches[1]; $bin = $Matches[2]
    # Its Linux PID goes to srv/pid, so the end can stop exactly that process.
    # (An argument list of its own: Start-Process would split the bash command at its spaces.)
    $lx = To-Wsl "$W\srv"
    $psi = New-Object System.Diagnostics.ProcessStartInfo "wsl.exe"
    foreach ($a in @("-d", $wslDistro, "--exec", "bash", "-c", "echo `$`$ > '$lx/pid'; exec '$bin' '$lx'")) { [void]$psi.ArgumentList.Add($a) }
    $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
    $srvProc = [System.Diagnostics.Process]::Start($psi)
}
for ($i = 0; $i -lt 100 -and -not (Test-Path "$W\srv\port"); $i++) { Start-Sleep -Milliseconds 100 }
if (-not (Test-Path "$W\srv\port")) { Bad "release server" "no port file in $W\srv"; Total "update.ps1"; exit 1 }
$port = (Get-Content "$W\srv\port").Trim()
[IO.File]::WriteAllText("$W\cfg\config", "update-url = https://localhost:$port/r`n")
function Stop-Server {
    if ($wslDistro -and (Test-Path "$W\srv\pid")) { & wsl.exe -d $wslDistro --exec kill (Get-Content "$W\srv\pid").Trim() 2>$null }
    if ($srvProc -and -not $srvProc.HasExited) { Stop-Process -Id $srvProc.Id -ErrorAction SilentlyContinue }
}

# 0.90.0 installed as a version folder and the junction, through the layout code itself.
Copy-Item $Old "$W\stage\haetty.exe"
$r = Run-Exe $Old @("install-layout", "put", "--opt", $OPT, "--name", "0.90.0-aaaaaaaaaaaa", "--commit", $A, "--", "$W\stage\haetty.exe")
$r2 = Run-Exe $Old @("install-layout", "activate", "0.90.0-aaaaaaaaaaaa", "--opt", $OPT)
function Active { $j = Get-Item -LiteralPath "$OPT\current" -Force -ErrorAction SilentlyContinue; if ($j) { Split-Path ([string]$j.Target) -Leaf } else { "" } }
if ((Active) -eq "0.90.0-aaaaaaaaaaaa" -and (Version-Of $LINK) -match "0\.90\.0 \(commit aaaaaaaaaaaa") { Ok "0.90.0 installed as a version folder and junction" } else { Bad "setup" "$(Active) $($r.out) $($r.err) $($r2.out) $($r2.err)" }
# A Start menu shortcut as install-stable.ps1 makes it, in this run's own APPDATA: it names current\haetty.exe.
$lnk = "$W\roaming\Microsoft\Windows\Start Menu\Programs\haetty.lnk"
New-Item -ItemType Directory -Force (Split-Path $lnk) | Out-Null
$sc = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk); $sc.TargetPath = $LINK; $sc.Save()
function Shortcut-Version { Version-Of ((New-Object -ComObject WScript.Shell).CreateShortcut($lnk).TargetPath) }
# One request with the test CA, without a window: Schannel trusts the server and the signature is the test key's.
$chk = (Run-Exe $LINK @("update", "--check", "--json")).out.Trim()
if ($chk -match '"latest":"0\.90\.1","newer":true') { Ok "update --check over HTTPS (Schannel, the test CA): $chk" } else { Bad "update --check" $chk }

if ($SetupOnly) {
    Stop-Server
    if ((Real-State) -eq $real0) { Ok "the real install folder and Start menu are as before" } else { Bad "real" "changed" }
    Total "update.ps1 (-SetupOnly: no window, no key)"
    exit $script:F
}

function Wait-Log([string]$Pattern, [int]$Tries = 100) {
    for ($i = 0; $i -lt $Tries; $i++) { if (Select-String -Path "$W\gui.log" -Pattern $Pattern -Quiet -ErrorAction SilentlyContinue) { return $true }; Start-Sleep -Milliseconds 200 }
    $false
}
function Last-Log([string]$Pattern) { (Select-String -Path "$W\gui.log" -Pattern $Pattern -ErrorAction SilentlyContinue | Select-Object -Last 1).Line }
function Count-Log([string]$Pattern) { @(Select-String -Path "$W\gui.log" -Pattern $Pattern -ErrorAction SilentlyContinue).Count }
# The window a restart started: its PID from the log line ("restart: ... started (pid N)"), never by name.
function Restarted-Pid { $m = Select-String -Path "$W\gui.log" -Pattern 'started \(pid (\d+)\)' -ErrorAction SilentlyContinue | Select-Object -Last 1; if ($m) { [int]$m.Matches[0].Groups[1].Value } else { 0 } }
# Every window this run started or took over, and every daemon the log names: stopped by PID at the end, whatever
# way the script ends (lib.ps1's Keys leaves with `exit` when the focus is lost or somebody types). Only processes
# whose executable lies in this run's folder are stopped.
$script:Windows = @()
function Take-Window([int]$Id) {
    if ($Id -le 0) { return $false }   # (PID 0 exists: the idle process)
    $script:Gui = Get-Process -Id $Id -ErrorAction SilentlyContinue
    if (-not $script:Gui) { return $false }
    $script:Windows += $Id
    for ($i = 0; $i -lt 60; $i++) { Start-Sleep -Milliseconds 250; $script:Gui.Refresh(); if ($script:Gui.MainWindowHandle -ne 0) { break } }
    Start-Sleep -Milliseconds 1500
    Focus-Diag "window of pid $Id"
    $script:Gui.MainWindowHandle -ne 0
}
function Start-Window([string]$File, [string[]]$Program) {
    $script:Gui = Start-Process -FilePath $File -ArgumentList (@("--cols", "90", "--rows", "20") + $Program) -WorkingDirectory $W -PassThru
    [void](Take-Window $script:Gui.Id)
}
function Stop-Ours([int[]]$Ids) {
    foreach ($id in ($Ids | Sort-Object -Unique)) {
        $p = Get-Process -Id $id -ErrorAction SilentlyContinue
        if ($p -and $p.Path -and $p.Path.StartsWith($W, [StringComparison]::OrdinalIgnoreCase)) { Stop-Process -Id $id -ErrorAction SilentlyContinue }
    }
}
function Stop-Everything {
    Stop-Ours (@($script:Windows) + @(Select-String -Path "$W\gui.log" -Pattern 'started \(pid (\d+)\)' -ErrorAction SilentlyContinue | ForEach-Object { [int]$_.Matches[0].Groups[1].Value }))
    Start-Sleep 1
    Stop-Ours @(Select-String -Path "$W\gui.log" -Pattern 'daemon (\d+) listening' -ErrorAction SilentlyContinue | ForEach-Object { [int]$_.Matches[0].Groups[1].Value })
    Start-Sleep 1
}
function Updates-Row([int]$N) { Palette "updates"; for ($i = 0; $i -lt $N; $i++) { Keys "{DOWN}" 300 } }
# After "Download and install": the next "update install: ..." line of the log (installed, or not installed: why).
function Install-Line([int]$Before) {
    for ($i = 0; $i -lt 300 -and (Count-Log "update install: ") -eq $Before; $i++) { Start-Sleep -Milliseconds 200 }
    Last-Log "update install: "
}

# --- Form (a) ------------------------------------------------------------------------------------------------
function Form-A {
    # An interactive shell: a tab that a restart restores and that stays open.
    Start-Window $LINK @("cmd.exe", "/k")
    $first = $script:Gui
    if (Wait-Log "update check: 0.90.1 available") { Ok "the check found 0.90.1 (the window's own check, 1.5 s after the start)" } else { Bad "check" (Last-Log "update check"); return }
    $n = Count-Log "update install: "
    Updates-Row 3; Keys "{ENTER}"   # Download and install
    $line = Install-Line $n
    if ($line -match "update install: 0\.90\.1-bbbbbbbbbbbb installed") { Ok "downloaded, verified, installed beside 0.90.0" }
    else {
        # Nothing more is typed into a panel that is not in the state the next keys expect.
        Bad "install" $line; Keys "{ESC}"
        Write-Host "form (a) stops here: the install did not happen, so Restart now, Roll back and the watch are not run"
        return
    }
    if ((Active) -eq "0.90.1-bbbbbbbbbbbb" -and (Test-Path "$OPT\0.90.0-aaaaaaaaaaaa\haetty.exe")) { Ok "the junction points at 0.90.1; 0.90.0 stays" } else { Bad "junction" (Active) }
    $nf = @(Get-ChildItem "$OPT\0.90.1-bbbbbbbbbbbb" -File -ErrorAction SilentlyContinue)
    if ($nf.Count -ge 2 -and -not ($nf | Where-Object { -not $_.IsReadOnly })) { Ok "the new folder's files are read-only: $(($nf.Name) -join ' ')" } else { Bad "read-only" (($nf | ForEach-Object { "$($_.Name)=$($_.IsReadOnly)" }) -join " ") }
    if ((Shortcut-Version) -match "0\.90\.1 \(commit bbbbbbbbbbbb") { Ok "the Start menu shortcut (through current) starts 0.90.1" } else { Bad "shortcut" (Shortcut-Version) }
    if (-not $first.HasExited) { Ok "installing changed nothing running (0.90.0's window still up)" } else { Bad "running" "window gone" }
    Keys "{ESC}"; Updates-Row 3; Keys "{ENTER}"; Keys "{ENTER}"   # Restart now, twice
    [void]$first.WaitForExit(30000)
    if (Wait-Log "restart: .* started" 150) { Ok "Restart now: the window closed, the daemon ended, the new version started" } else { Bad "restart" (Last-Log "restart:"); return }
    $np = Restarted-Pid; Start-Sleep 3
    $v = Version-Of $LINK
    if ($v -match "0\.90\.1 \(commit bbbbbbbbbbbb") { Ok "0.90.1 runs: $v" } else { Bad "version" $v }
    if (@((Status).sessions).Count -ge 1) { Ok "the tab is back after the restart" } else { Bad "tabs" "no session in status --json" }
    Start-Sleep 21
    if (-not (Test-Path "$W\state\update\pending")) { Ok "the new version stayed up 20 s: confirmed" } else { Bad "confirm" (Get-Content "$W\state\update\pending" -Raw) }
    # Roll back from the panel; the server already has 0.90.2, which ends at once.
    Release "0.90.2" $C $dying
    if (-not (Take-Window $np)) { Bad "restarted window" "pid $np has no window"; return }
    Updates-Row 1; Keys "{ENTER}"; Keys "{ENTER}"
    for ($i = 0; $i -lt 150 -and (Restarted-Pid) -eq $np; $i++) { Start-Sleep -Milliseconds 200 }
    if ((Active) -eq "0.90.0-aaaaaaaaaaaa") { Ok "Roll back: the junction points at 0.90.0 again" } else { Bad "rollback" (Active); return }
    $np = Restarted-Pid; Start-Sleep 3
    $v = Version-Of $LINK
    if ($v -match "0\.90\.0 \(commit aaaaaaaaaaaa") { Ok "0.90.0 runs again" } else { Bad "version after rollback" $v }
    if ((Shortcut-Version) -match "0\.90\.0 \(commit aaaaaaaaaaaa") { Ok "the Start menu shortcut starts 0.90.0 again" } else { Bad "shortcut after rollback" (Shortcut-Version) }
    if (-not (Take-Window $np)) { Bad "rolled-back window" "pid $np has no window"; return }
    if (Wait-Log "update check: 0.90.2 available" 100) { Ok "0.90.2 found" } else { Bad "check 0.90.2" (Last-Log "update check"); return }
    $n = Count-Log "update install: "
    Updates-Row 3; Keys "{ENTER}"
    $line = Install-Line $n
    if ($line -notmatch "update install: 0\.90\.2-cccccccccccc installed") { Bad "install 0.90.2" $line; Keys "{ESC}"; return }
    Keys "{ESC}"; Updates-Row 3; Keys "{ENTER}"; Keys "{ENTER}"
    if (Wait-Log "restart: the new version ended" 250) { Ok "0.90.2 ended at its start: noticed within 20 s" } else { Bad "watch" (Last-Log "restart:") }
    for ($i = 0; $i -lt 100 -and (Active) -ne "0.90.0-aaaaaaaaaaaa"; $i++) { Start-Sleep -Milliseconds 200 }
    if ((Active) -eq "0.90.0-aaaaaaaaaaaa") { Ok "rolled back to 0.90.0 by itself" } else { Bad "auto rollback" (Active) }
    Start-Sleep 3
    $note = if (Test-Path "$W\state\update\note") { Get-Content "$W\state\update\note" -Raw } else { "" }
    if ($note -match "rolled back to 0\.90\.0" -or (Select-String -Path "$W\gui.log" -Pattern "the new version ended" -Quiet)) { Ok "the reason is kept for the panel" } else { Bad "note" "-" }
}
# --- Form (b) ------------------------------------------------------------------------------------------------
function Form-B {
    Release "0.90.1" $B $New
    Remove-Item -Recurse -Force "$W\state\update" -ErrorAction SilentlyContinue
    Copy-Item $Old "$W\single\haetty.exe"
    $script:Exe = "$W\single\haetty.exe"   # lib.ps1's Status and Stop-Gui ask this path
    $checks = Count-Log "update check: 0.90.1 available"
    Start-Window $script:Exe @("cmd.exe", "/k")
    $win = $script:Gui
    for ($i = 0; $i -lt 100 -and (Count-Log "update check: 0.90.1 available") -eq $checks; $i++) { Start-Sleep -Milliseconds 200 }
    if ((Count-Log "update check: 0.90.1 available") -eq $checks) { Bad "(b) check" (Last-Log "update check"); return }
    $n = Count-Log "update install: "
    Updates-Row 3; Keys "{ENTER}"
    $line = Install-Line $n
    if ($line -match "update install: single installed") { Ok "(b) the single executable replaced while it runs" } else { Bad "(b) install" $line; Keys "{ESC}"; return }
    $v = Version-Of $script:Exe
    if ($v -match "bbbbbbbbbbbb") { Ok "(b) the file is 0.90.1: $v" } else { Bad "(b) version" $v }
    if (Test-Path "$W\single\haetty.exe.old") { Ok "(b) the running file was renamed to haetty.exe.old" } else { Bad "(b) old" "missing" }
    if (-not $win.HasExited) { Ok "(b) the window of the old file keeps running" } else { Bad "(b) running" "window gone" }
    Stop-Everything
    # The next start removes haetty.exe.old.
    Start-Window $script:Exe @("cmd.exe", "/k")
    Start-Sleep 2
    if (-not (Test-Path "$W\single\haetty.exe.old")) { Ok "(b) haetty.exe.old is gone after the next start" } else { Bad "(b) old kept" "still there" }
}
try {
    $script:Exe = $LINK
    Form-A
    Stop-Everything   # whatever state form (a) ended in: its windows and daemons go before form (b) starts
    Form-B
}
finally {
    Stop-Everything
    Stop-Server
    # Nothing real touched.
    if ((Real-State) -eq $real0) { Ok "the real install folder and Start menu are as before" } else { Bad "real" "changed" }
    Get-ChildItem $OPT -Recurse -File -Force -ErrorAction SilentlyContinue | ForEach-Object { $_.IsReadOnly = $false }
    Total "update.ps1"
}
exit $script:F
