# Self-test of install-stable.ps1 (Windows). Installs two commits under a prefix inside the test run directory
# (never the real %LOCALAPPDATA%; APPDATA, for the Start menu shortcut, points into the run too) and checks the
# read-only files, the `current` junction, --version, the shortcut, rollback, reinstall, that the user PATH and
# the real install folder are untouched, and that a real prefix is refused.
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"; pwsh -File tools\test-install-stable.ps1 REF1 REF2
# PowerShell may refuse to dot-source from \\wsl.localhost inside -File: run a local copy of tools\ and scripts\.
param(
    [Parameter(Mandatory, Position = 0)][string]$Ref1,
    [Parameter(Mandatory, Position = 1)][string]$Ref2,
    [string]$Distro = "Ubuntu",
    [string]$WslRepo = "/home/denny/projects/claude-code-config"
)
. (Join-Path $PSScriptRoot "..\scripts\test-guard.ps1")
$run = Initialize-HaettyGuard -Name HAETTY_TEST_TMP
$inst = Join-Path $PSScriptRoot "install-stable.ps1"
$script:pass = 0; $script:fail = 0
function Ok([string]$m) { $script:pass++; "PASS $m" }
function Bad([string]$m, [string]$why) { $script:fail++; "FAIL ${m}: $why" }
function Inst([string[]]$a) {
    $out = & pwsh -NoProfile -ExecutionPolicy Bypass -File $inst -Distro $Distro -WslRepo $WslRepo @a 2>&1 | Out-String
    @{ rc = $LASTEXITCODE; out = $out }
}
function Commit([string]$r) { (& wsl.exe -d $Distro --exec git -C $WslRepo rev-parse "$r^{commit}").Trim() }
$realRoot = Join-Path $env:LOCALAPPDATA "Programs\haetty"
$realLnk = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\haetty.lnk"
$realState = { "{0}|{1}|{2}" -f (Test-Path $realRoot), (Test-Path $realLnk), [Environment]::GetEnvironmentVariable("PATH", "User") }
$before = & $realState
$savedAppData = $env:APPDATA
$c1 = Commit $Ref1; $c2 = Commit $Ref2
$v1 = "0.1.0-" + $c1.Substring(0, 12); $v2 = "0.1.0-" + $c2.Substring(0, 12)
$P = Join-Path $run "local"; $Root = Join-Path $P "Programs\haetty"; $Cur = Join-Path $Root "current"
try {
    # 1. A real prefix is refused before anything is built or written.
    $r = Inst @("-Prefix", $env:LOCALAPPDATA, $Ref1)
    if ($r.rc -ne 0 -and $r.out -match "refusing" -and (& $realState) -eq $before) { Ok "real prefix refused, nothing written" } else { Bad "real prefix" "rc=$($r.rc) $($r.out)" }

    # 1b. A release tag whose crate version differs is refused before building (a throwaway tag in a
    #     clone in WSL's /tmp; the real repository gets no tag).
    $clone = "/tmp/haetty-test-tagclone-$PID"
    $head = (& wsl.exe -d $Distro --exec git -C $WslRepo rev-parse HEAD).Trim()
    & wsl.exe -d $Distro --exec bash -c "git clone -q '$WslRepo' '$clone' && git -C '$clone' checkout -q $head && git -C '$clone' tag haetty-v9.9.9 HEAD"
    $have = (& wsl.exe -d $Distro --exec bash -c "sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *`"\([^`"]*\)`".*/\1/p' '$clone/haetty/Cargo.toml' | head -1").Trim()
    $out = & pwsh -NoProfile -ExecutionPolicy Bypass -File $inst -Distro $Distro -WslRepo $clone -Prefix $P haetty-v9.9.9 2>&1 | Out-String
    $rc = $LASTEXITCODE
    & wsl.exe -d $Distro --exec rm -rf $clone
    if ($rc -ne 0 -and $out -match "crate version is $([regex]::Escape($have)), not 9\.9\.9" -and $out -notmatch "(?m)^building" -and -not (Test-Path $Root)) { Ok "tag haetty-v9.9.9 on a $have commit refused before building" } else { Bad "tag/version mismatch" "rc=$rc $out" }

    $env:APPDATA = Join-Path $run "roaming"; New-Item -ItemType Directory -Force $env:APPDATA | Out-Null
    # 2. First install.
    $r1 = Inst @("-Prefix", $P, $Ref1)
    if ($r1.rc -eq 0) { Ok "install $Ref1" } else { Bad "install $Ref1" "rc=$($r1.rc) $($r1.out)" }
    $files = Get-ChildItem (Join-Path $Root $v1) -File
    if ($files.Count -ge 4 -and -not ($files | Where-Object { -not $_.IsReadOnly })) { Ok ("files read-only: " + (($files.Name) -join " ")) } else { Bad "read-only" (($files | ForEach-Object { "$($_.Name)=$($_.IsReadOnly)" }) -join " ") }
    $j = Get-Item $Cur -Force
    if (($j.Attributes -band [IO.FileAttributes]::ReparsePoint) -and (Split-Path ([string]$j.Target) -Leaf) -eq $v1) { Ok "current -> $v1 (junction)" } else { Bad "junction" "$($j.Attributes) $($j.Target)" }
    $saved = $env:HAETTY_INSTANCE; $env:HAETTY_INSTANCE = $null
    $ver = (& (Join-Path $Cur "haetty.exe") --version 2>&1 | Out-String).Trim()
    $env:HAETTY_INSTANCE = $saved
    if ($ver -match "commit $($c1.Substring(0,12)),.*instance stable\)") { Ok "--version: $ver" } else { Bad "--version" $ver }
    if ($r1.out -match "check: OK") { Ok "installer's own check OK" } else { Bad "installer check" $r1.out }
    $lnk = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\haetty.lnk"
    $t = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk).TargetPath
    if ($t -eq (Join-Path $Cur "haetty.exe")) { Ok "Start menu shortcut -> current\haetty.exe" } else { Bad "shortcut" $t }
    if ($r1.out -match "PATH: not changed") { Ok "user PATH left alone in a test" } else { Bad "PATH" $r1.out }
    $rule = "Edit(//" + ($Root.Substring(0, 1).ToLower()) + "/" + ($Root.Substring(3) -replace '\\', '/') + "/**)"
    if ($r1.out.Contains($rule) -and $r1.out -match "blockWritesOutsideWorkingDirectories") { Ok "printed suggestion with $rule" } else { Bad "suggestion" "no $rule" }

    # 3. Second version.
    $r2 = Inst @("-Prefix", $P, $Ref2)
    $j = Get-Item $Cur -Force
    if ($r2.rc -eq 0 -and (Split-Path ([string]$j.Target) -Leaf) -eq $v2 -and (Test-Path (Join-Path $Root "$v1\haetty.exe"))) { Ok "install ${Ref2}: current -> $v2, $v1 kept" } else { Bad "install $Ref2" "rc=$($r2.rc) $($j.Target) $($r2.out)" }
    if ($r2.out -match "previous: $v1") { Ok "installer names the previous version" } else { Bad "previous" $r2.out }
    $l = (Inst @("-Prefix", $P, "-List")).out
    if ($l -match "\* $v2" -and $l -match "  $v1") { Ok "-List marks $v2 active" } else { Bad "-List" $l }

    # 4. Rollback.
    $ra = Inst @("-Prefix", $P, "-Activate", $v1)
    $env:HAETTY_INSTANCE = $null
    $ver = (& (Join-Path $Cur "haetty.exe") --version 2>&1 | Out-String).Trim()
    $env:HAETTY_INSTANCE = $saved
    if ($ra.rc -eq 0 -and $ver -match $c1.Substring(0, 12)) { Ok "-Activate ${v1}: rolled back ($ver)" } else { Bad "rollback" "rc=$($ra.rc) $ver" }
    $rx = Inst @("-Prefix", $P, "-Activate", "nonexistent")
    if ($rx.rc -ne 0) { Ok "unknown version refused" } else { Bad "unknown version" "accepted" }

    # 5. Reinstall: only activates.
    $r3 = Inst @("-Prefix", $P, $Ref2)
    if ($r3.out -match "already installed" -and (Split-Path ([string](Get-Item $Cur -Force).Target) -Leaf) -eq $v2) { Ok "reinstall ${Ref2}: activated, not copied again" } else { Bad "reinstall" $r3.out }
} finally {
    $env:APPDATA = $savedAppData
    if (Test-Path $Root) { Get-ChildItem $Root -Recurse -File -Force | ForEach-Object { $_.IsReadOnly = $false } }
}
# 6. The real install folder, Start menu and user PATH untouched; no worktree left in WSL.
if ((& $realState) -eq $before) { Ok "real %LOCALAPPDATA%\Programs\haetty, Start menu, user PATH unchanged" } else { Bad "real state" "changed" }
$wt = (& wsl.exe -d $Distro --exec git -C $WslRepo worktree list) -join "`n"
if ($wt -notmatch "haetty-install") { Ok "temporary worktrees removed" } else { Bad "worktrees" $wt }
"TOTAL install-stable.ps1: $script:pass passed, $script:fail failed"
if ($script:fail) { exit 1 }
