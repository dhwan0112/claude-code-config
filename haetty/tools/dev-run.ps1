# Builds this working tree for Windows (in WSL, in a heavy-job slot) and starts it as the dev instance
# (HAETTY_INSTANCE=dev) beside the stable one (docs/SELF-HOSTING.md). Run with pwsh from the WSL checkout:
#   pwsh -ExecutionPolicy Bypass -File \\wsl.localhost\<distro>\…\haetty\tools\dev-run.ps1 [-Debug] [-NoBuild] [-Check] [-- HAETTY-ARGS…]
# Every run prints what the dev instance uses next to the stable one — pipe, state and config directories, app
# id, window title prefix, binary — and refuses to start when anything would be shared: an override
# (HAETTY_SOCKET / _STATE_DIR / _CONFIG_DIR / _LOG_FILE) or a path in the dev config
# that points at the stable instance. The dev binary is copied to %LOCALAPPDATA%\haetty-dev\bin, never into the
# stable install (%LOCALAPPDATA%\Programs\haetty). Never touches the stable daemon.
$DebugBuild = $false; $NoBuild = $false; $CheckOnly = $false; $rest = @()
for ($i = 0; $i -lt $args.Count; $i++) {
    switch -Exact ($args[$i]) {
        "-Debug" { $DebugBuild = $true } "-NoBuild" { $NoBuild = $true } "-Check" { $CheckOnly = $true }
        "--" { $rest = @($args | Select-Object -Skip ($i + 1)); $i = $args.Count }
        default { $rest = @($args | Select-Object -Skip $i); $i = $args.Count }
    }
}
function Die([string]$m) { Write-Host "dev-run: REFUSED: $m"; exit 3 }
$p = (Resolve-Path -LiteralPath $PSScriptRoot).ProviderPath
if ($p -notmatch '^\\\\wsl(?:\.localhost|\$)\\([^\\]+)(\\.*)\\tools$') { Die "run it from the WSL checkout (\\wsl.localhost\<distro>\…\haetty\tools)" }
$distro = $Matches[1]; $proj = ($Matches[2] -replace '\\', '/'); $projUnc = Split-Path $p
# Parent markers out (a dev window started from a stable tab or a Claude Code session is not their child).
Get-ChildItem env: | Where-Object { $_.Name -match '^(HAETTY_SESSION_ID|CLAUDE_CODE_.*|CLAUDECODE|CLAUDE_.*SESSION.*)$' } |
    ForEach-Object { Remove-Item "env:$($_.Name)" }
$env:HAETTY_INSTANCE = "dev"
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$stable = @("$env:LOCALAPPDATA\haetty", "$env:APPDATA\haetty", "$env:LOCALAPPDATA\ccterm", "$env:APPDATA\ccterm", "$env:LOCALAPPDATA\Programs\haetty")
function Inside([string]$x) {
    $x = $x.TrimEnd('\'); if ($x -ieq "\\.\pipe\haetty-$sid" -or $x -ieq "\\.\pipe\ccterm-$sid") { return $true }
    foreach ($s in $stable) { if ($x -ieq $s -or $x.StartsWith("$s\", [StringComparison]::OrdinalIgnoreCase)) { return $true } }
    $false
}
foreach ($n in "SOCKET", "STATE_DIR", "CONFIG_DIR", "LOG_FILE") {
    foreach ($pre in "HAETTY_") {
        $v = [Environment]::GetEnvironmentVariable("$pre$n")
        if ($v) { $full = if ($v.StartsWith('\\.\')) { $v } else { [IO.Path]::GetFullPath($v) }; if (Inside $full) { Die "$pre$n=$v points at the stable instance" } }
    }
}
function Pick([string]$a, [string]$def) { $v = [Environment]::GetEnvironmentVariable($a); if ($v) { return $v }; $def }
$pipe = Pick "HAETTY_SOCKET" "\\.\pipe\haetty-dev-$sid"
$state = Pick "HAETTY_STATE_DIR" "$env:LOCALAPPDATA\haetty-dev"
$cfg = Pick "HAETTY_CONFIG_DIR" "$env:APPDATA\haetty-dev"
if (Test-Path "$cfg\config") {
    foreach ($s in $stable) {
        $hit = Select-String -LiteralPath "$cfg\config" -SimpleMatch $s | Where-Object { $_.Line -notmatch '^\s*#' } | Select-Object -First 1
        if ($hit) { Die "$cfg\config line $($hit.LineNumber) names $s (the stable instance)" }
    }
}
$prof = if ($DebugBuild) { "debug" } else { "release" }
$built = "$projUnc\target\x86_64-pc-windows-gnu\$prof\haetty.exe"
$bin = "$state\bin\haetty.exe"
if (-not $NoBuild -and -not $CheckOnly) {
    $flag = if ($DebugBuild) { "" } else { "--release" }
    & wsl.exe -d $distro --exec bash -c "cd '$proj' && . ~/.cargo/env 2>/dev/null; command -v zig >/dev/null || PATH=`$HOME/.local/bin:`$PATH; tools/heavy.sh --label 'dev-run.ps1 build' -- cargo build -q $flag -p haetty-gui --target x86_64-pc-windows-gnu"
    if ($LASTEXITCODE -ne 0) { Write-Host "dev-run: build failed"; exit 2 }
    New-Item -ItemType Directory -Force "$state\bin" | Out-Null
    Copy-Item -LiteralPath $built $bin -Force
}
if (-not (Test-Path $bin)) { Die "no dev build at $bin (run without -NoBuild)" }
if (Inside $bin) { Die "the binary $bin is in the stable install" }
$ver = (& $bin --version | Out-String).Trim()
if ($ver -notmatch 'instance dev') { Die "the build does not report the dev instance: $ver" }
$stableBin = "$env:LOCALAPPDATA\Programs\haetty\current\haetty.exe"
$rows = @(
    @("pipe", $pipe, "\\.\pipe\haetty-$sid"), @("state", $state, "$env:LOCALAPPDATA\haetty"), @("config", $cfg, "$env:APPDATA\haetty"),
    @("app id", "io.github.dhwan0112.haetty.dev", "io.github.dhwan0112.haetty"), @("window title", '"[dev] …"', '"…"'),
    @("binary", $bin, $(if (Test-Path $stableBin) { $stableBin } else { "-" })))
"{0,-14} {1,-60} {2}" -f "", "dev (this run)", "stable (untouched)"
foreach ($r in $rows) { "{0,-14} {1,-60} {2}" -f $r[0], $r[1], $r[2] }
"version: $ver"
if ($pipe -ieq "\\.\pipe\haetty-$sid") { Die "same pipe as the stable instance" }
if ($CheckOnly) { "dev-run: checks passed (-Check: not started)"; exit 0 }
$g = Start-Process -FilePath $bin -ArgumentList $rest -PassThru
$name = Split-Path -Leaf $pipe
for ($i = 0; $i -lt 40 -and -not ([IO.Directory]::GetFiles('\\.\pipe\') -contains "\\.\pipe\$name"); $i++) { Start-Sleep -Milliseconds 250 }
if ([IO.Directory]::GetFiles('\\.\pipe\') -contains "\\.\pipe\$name") { "started: dev window pid $($g.Id), dev daemon on $pipe" }
else { Write-Host "dev-run: the dev daemon pipe did not appear"; exit 4 }
