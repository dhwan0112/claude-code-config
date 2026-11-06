<#
  Checks every commit of a range on its own, from Windows: runs tools/check-range.sh in WSL (the repository and
  its builds live there; the Windows target is checked with clippy for x86_64-pc-windows-gnu).

    pwsh -File \\wsl.localhost\Ubuntu\home\<you>\projects\claude-code-config\haetty\tools\check-range.ps1 haetty-v0.1.1..HEAD

  Options: -Distro NAME, -WslRepo /path (default: taken from where this script lives, \\wsl.localhost\<distro>\...),
           -WslTestTmp /tmp/... (the WSL test base, default /tmp/haetty-test), and check-range.sh's own options
           (--no-windows, --no-test, --keep). Exit code: the number of failing commits.
#>
param(
    [Parameter(Mandatory = $true, Position = 0)][string]$Range,
    [string]$Distro,
    [string]$WslRepo,
    [string]$WslTestTmp = "/tmp/haetty-test",
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)
$ErrorActionPreference = "Stop"
if (-not $Distro -or -not $WslRepo) {
    $m = [regex]::Match($PSScriptRoot, '^\\\\wsl(?:\.localhost|\$)\\([^\\]+)(\\.*)\\haetty\\tools$')
    if (-not $m.Success) { throw "check-range.ps1: give -Distro and -WslRepo (script not under \\wsl.localhost\...)" }
    if (-not $Distro) { $Distro = $m.Groups[1].Value }
    if (-not $WslRepo) { $WslRepo = $m.Groups[2].Value -replace '\\', '/' }
}
if (-not $WslTestTmp.StartsWith("/")) { throw "check-range.ps1: -WslTestTmp must be an absolute WSL path" }
$cmd = "export HAETTY_TEST_TMP='$WslTestTmp'; . ~/.cargo/env 2>/dev/null; cd '$WslRepo/haetty' && tools/check-range.sh $($Rest -join ' ') '$Range'"
& wsl.exe -d $Distro --exec bash -c $cmd
exit $LASTEXITCODE
