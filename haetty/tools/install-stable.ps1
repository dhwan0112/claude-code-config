<#
Install a stable haetty on Windows.

  install-stable.ps1 REF                  build REF (commit or tag; a tag haetty-vX.Y.Z must carry crate version
                                          X.Y.Z, else refused before building) in its own git worktree in WSL (cross-build,
                                          dist profile, via install-stable.sh --build-only), install, activate
  install-stable.ps1 -List                installed versions (* = active)
  install-stable.ps1 -Activate VERSION    make an installed version the active one (rollback)
  Options: -Prefix DIR (default %LOCALAPPDATA%), -Distro NAME and -WslRepo /path (default: taken from where
           this script lives, \\wsl.localhost\<distro>\...), -NoPath, -NoShortcut

Layout:
  <Prefix>\Programs\haetty\<version>\   haetty.exe, LICENSE, THIRD-PARTY-NOTICES.md, README.md, INSTALLED;
                                        every file with the read-only attribute; never changed again
  <Prefix>\Programs\haetty\current      directory junction to the active version: the one fixed path
                                        (PATH, the Start menu shortcut, a taskbar pin all use current\haetty.exe)
<version> = <crate version>-<12-digit commit>. Older versions stay; rollback = -Activate VERSION (re-points
the junction). Nothing running is stopped: an open stable haetty keeps its version until all its windows are
closed and its daemon has exited.

Test isolation: with HAETTY_TEST_TMP or HAETTY_RUN set, every path written must lie in the test run directory
(scripts/test-guard.ps1), and the user PATH is never changed.
#>
param(
    [Parameter(Position = 0)][string]$Ref,
    [switch]$List,
    [string]$Activate,
    [string]$Prefix = $env:LOCALAPPDATA,
    [string]$Distro,
    [string]$WslRepo,
    [switch]$NoPath,
    [switch]$NoShortcut
)
$ErrorActionPreference = 'Stop'

$Testing = [bool]($env:HAETTY_TEST_TMP -or $env:HAETTY_RUN)
if ($Testing) {
    . (Join-Path $PSScriptRoot "..\scripts\test-guard.ps1")
    [void](Initialize-HaettyGuard -Name HAETTY_TEST_TMP)
}
function Guard([string]$p) { if ($Testing) { Assert-HaettyPath $p } }

$Root = Join-Path $Prefix "Programs\haetty"
$Current = Join-Path $Root "current"
# In a test, refuse a real prefix before building anything.
Guard $Root

function Get-Active {
    $i = Get-Item -LiteralPath $Current -Force -ErrorAction SilentlyContinue
    if ($i -and ($i.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return Split-Path ([string]$i.Target) -Leaf }
    $null
}

function Show-List {
    if (-not (Test-Path $Root)) { "nothing installed in $Root"; return }
    $active = Get-Active
    Get-ChildItem -LiteralPath $Root -Directory | Where-Object { $_.Name -ne "current" -and -not $_.Name.StartsWith(".") } | ForEach-Object {
        $info = Get-Content -LiteralPath (Join-Path $_.FullName "INSTALLED") -ErrorAction SilentlyContinue | Where-Object { $_ -match '^(ref|installed)=' }
        "{0} {1}  {2}" -f $(if ($_.Name -eq $active) { "*" } else { " " }), $_.Name, ($info -join " ")
    }
}

function Set-Active([string]$v) {
    $target = Join-Path $Root $v
    if (-not (Test-Path (Join-Path $target "haetty.exe"))) { throw "version $v is not installed (see -List)" }
    Guard $Current
    $prev = Get-Active
    $i = Get-Item -LiteralPath $Current -Force -ErrorAction SilentlyContinue
    if ($i) {
        if (-not ($i.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "$Current is a real folder, not the junction; move it away first" }
        cmd /c rmdir "$Current" | Out-Null      # removes the junction only, never its target
    }
    New-Item -ItemType Junction -Path $Current -Target $target | Out-Null
    "active: $v  ($Current -> $target)"
    if ($prev -and $prev -ne $v) { "previous: $prev  (roll back: install-stable.ps1 -Activate $prev)" }
}

function Get-RepoLocation {
    if ($WslRepo -and $Distro) { return @($Distro, $WslRepo) }
    $m = [regex]::Match($PSScriptRoot, '^\\\\wsl(?:\.localhost|\$)\\([^\\]+)(\\.*)\\haetty\\tools$')
    if (-not $m.Success) { throw "cannot tell the WSL repository from $PSScriptRoot; give -Distro and -WslRepo" }
    @($m.Groups[1].Value, ($m.Groups[2].Value -replace '\\', '/'))
}

function ConvertTo-WslPath([string]$p) {
    $full = [IO.Path]::GetFullPath($p)
    if ($full -notmatch '^([A-Za-z]):\\(.*)$') { throw "not a drive path: $full" }
    "/mnt/" + $Matches[1].ToLower() + "/" + ($Matches[2] -replace '\\', '/')
}

function Show-Suggestion {
    # Claude Code matches Windows paths in POSIX form: C:\Users\x -> //c/Users/x (an absolute rule).
    $full = [IO.Path]::GetFullPath($Root)
    $rule = if ($full -match '^([A-Za-z]):\\(.*)$') { "//" + $Matches[1].ToLower() + "/" + ($Matches[2] -replace '\\', '/') } else { $full -replace '\\', '/' }
    @"

Suggested Claude Code settings (printed only - nothing was changed; put them into your own
%USERPROFILE%\.claude\settings.json yourself if you want them):

  {
    "permissions": {
      "deny": [
        "PowerShell(*Stop-Process*haetty*)",
        "PowerShell(*taskkill*haetty*)",
        "Bash(*taskkill*haetty*)",
        "Bash(*pkill*haetty*)",
        "Bash(*killall*haetty*)",
        "Edit(${rule}/**)"
      ],
      "blockWritesOutsideWorkingDirectories": true
    }
  }

- Deny rules apply in every permission mode, bypassPermissions included. The command patterns match the
  command text only (not "Stop-Process -Id <pid>"); they also refuse e.g. a search for those words.
- Edit deny rules cover Claude's file tools, ">" redirections and tee - not Copy-Item/cp in a shell
  command; the installed files also carry the read-only attribute. Native Windows has no Claude Code
  sandbox (it needs WSL2), so nothing stronger is available for shell commands here.
- blockWritesOutsideWorkingDirectories (Claude Code 2.1.314+) makes Write/Edit/NotebookEdit refuse paths
  outside the working directories in every mode; it does not limit shell commands, and it also removes
  the EnterWorktree/ExitWorktree tools (worktrees then have to be made with git yourself).
"@
}

if ($List) { Show-List; return }
if ($Activate) { Set-Active $Activate; return }
if (-not $Ref) { Get-Content $PSCommandPath -TotalCount 14 | Select-Object -Skip 1; exit 2 }

# 1. Build in WSL (worktree, dist profile, Windows target).
$distro, $repo = Get-RepoLocation
$base = if ($Testing) { $env:HAETTY_RUN } else { $env:TEMP }
$stage = Join-Path $base ("haetty-install-" + [IO.Path]::GetRandomFileName().Replace(".", ""))
Guard $stage
New-Item -ItemType Directory -Force $stage | Out-Null
try {
    # --exec: no login shell in between (zsh would re-split the arguments).
    & wsl.exe -d $distro --exec bash "$repo/haetty/tools/install-stable.sh" --build-only --target x86_64-pc-windows-gnu --out (ConvertTo-WslPath $stage) $Ref
    if ($LASTEXITCODE -ne 0) { throw "build failed (exit $LASTEXITCODE)" }
    $info = @{}; Get-Content (Join-Path $stage "BUILDINFO") | ForEach-Object { $k, $v = $_ -split '=', 2; $info[$k] = $v }
    $ver = $info.version; $commit = $info.commit

    # 2. Install (unless that version is already there).
    $dest = Join-Path $Root $ver
    Guard $dest
    if (Test-Path $dest) {
        "$ver is already installed; activating it"
    } else {
        $partial = Join-Path $Root ".$ver.partial"
        Guard $partial
        New-Item -ItemType Directory -Force $Root | Out-Null
        if (Test-Path $partial) { Get-ChildItem $partial -File | ForEach-Object { $_.IsReadOnly = $false }; Remove-Item -Recurse -Force $partial }
        New-Item -ItemType Directory $partial | Out-Null
        foreach ($f in "haetty.exe", "LICENSE", "THIRD-PARTY-NOTICES.md", "README.md") {
            if (Test-Path (Join-Path $stage $f)) { Copy-Item (Join-Path $stage $f) $partial }
        }
        @("version=$ver", "ref=$Ref", "commit=$commit", "installed=$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))", "source=$($info.source)") |
            Set-Content -Path (Join-Path $partial "INSTALLED") -Encoding utf8
        Get-ChildItem $partial -File | ForEach-Object { $_.IsReadOnly = $true }
        Rename-Item -LiteralPath $partial -NewName $ver
        "installed: $dest (files read-only)"
    }
} finally {
    Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
}

# 3. Activate, shortcut, PATH.
Set-Active $ver
$exe = Join-Path $Current "haetty.exe"
if (-not $NoShortcut) {
    $lnk = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\haetty.lnk"
    Guard $lnk
    New-Item -ItemType Directory -Force (Split-Path $lnk) | Out-Null
    $s = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
    $s.TargetPath = $exe; $s.WorkingDirectory = "%USERPROFILE%"; $s.Description = "haetty"; $s.Save()
    "Start menu: $lnk -> $exe"
}
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if (($userPath -split ';') -notcontains $Current) {
    if ($Testing -or $NoPath) { "PATH: not changed ($Current is not on the user PATH)" }
    else { [Environment]::SetEnvironmentVariable("PATH", ($userPath.TrimEnd(';') + ";" + $Current), "User"); "PATH: added $Current (new shells)" }
}

# 4. Check, desktop integration, a running stable daemon.
$saved = $env:HAETTY_INSTANCE; $env:HAETTY_INSTANCE = $null
try {
    (& $exe install-desktop 2>&1 | Out-String).Trim()
    $v = (& $exe --version 2>&1 | Out-String).Trim()
    "check: $v"
    if ($v -match "commit $($commit.Substring(0, 12)),.*instance stable\)") { "check: OK (commit $($commit.Substring(0, 12)), stable instance)" }
    else { "check: the installed binary does not report commit $($commit.Substring(0, 12)) (older than --version build info?)" }
    if (-not $Testing) {
        $st = (& $exe status --json 2>&1 | Out-String)
        if ($st -match '"daemon_pid":\d+') { "note: a stable haetty daemon is running; open windows keep the old version until all are closed" }
    }
} finally { $env:HAETTY_INSTANCE = $saved }
Show-Suggestion
