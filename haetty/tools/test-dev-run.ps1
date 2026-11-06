# Self-test of tools/dev-run.ps1 (-Check only: no window, no build): LOCALAPPDATA and APPDATA point into the test
# run, so the "stable" and "dev" directories are fakes there. Needs a Windows haetty.exe (-Exe; the build of this
# tree) — copied to the fake dev bin folder. Run with pwsh from the WSL checkout:
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"; pwsh -File tools\test-dev-run.ps1 -Exe <haetty.exe>
param([Parameter(Mandatory = $true)][string]$Exe)
. (Join-Path $PSScriptRoot "..\scripts\test-guard.ps1")
$run = Initialize-HaettyGuard -Name HAETTY_TEST_TMP
$F = Join-Path $run "dev-run"; Assert-HaettyPath $F
$env:LOCALAPPDATA = "$F\local"; $env:APPDATA = "$F\roaming"
Get-ChildItem env: | Where-Object { $_.Name -match '^(HAETTY_|CCTERM_)' } | ForEach-Object { Remove-Item "env:$($_.Name)" }
New-Item -ItemType Directory -Force "$F\local\haetty-dev\bin", "$F\roaming\haetty-dev" | Out-Null
Copy-Item $Exe "$F\local\haetty-dev\bin\haetty.exe"
$D = Join-Path $PSScriptRoot "dev-run.ps1"
$sid = [System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$pass = 0; $fail = 0
function Ok([bool]$c, [string]$m) { if ($c) { $script:pass++; "PASS $m" } else { $script:fail++; "FAIL $m" } }
function Run { $o = & pwsh -NoProfile -File $D -NoBuild -Check 2>&1 | Out-String; @{ rc = $LASTEXITCODE; out = $o } }
function Refused([string]$name, [string]$value) {
    Set-Item "env:$name" $value; $r = Run; Remove-Item "env:$name"
    ($r.rc -eq 3) -and ($r.out -match 'REFUSED')
}
$r = Run
Ok ($r.rc -eq 0) "checks pass with nothing shared (exit $($r.rc))"
Ok ($r.out -match [regex]::Escape("\\.\pipe\haetty-dev-$sid") -and $r.out -match [regex]::Escape("\\.\pipe\haetty-$sid")) "pipe: dev and stable both shown"
Ok ($r.out -match [regex]::Escape("$F\local\haetty-dev") -and $r.out -match 'instance dev') "state haetty-dev; the build reports instance dev"
Ok (Refused "HAETTY_STATE_DIR" "$F\local\haetty") "refused: HAETTY_STATE_DIR = the stable state directory"
Ok (Refused "HAETTY_CONFIG_DIR" "$F\roaming\haetty\sub") "refused: HAETTY_CONFIG_DIR inside the stable config directory"
Ok (Refused "CCTERM_SOCKET" "\\.\pipe\haetty-$sid") "refused: the old CCTERM_SOCKET = the stable pipe"
Ok (Refused "HAETTY_LOG_FILE" "$F\local\Programs\haetty\x.log") "refused: a path in the stable install"
"theme = $F\roaming\haetty\themes\mine" | Set-Content "$F\roaming\haetty-dev\config"
$r = Run; Ok ($r.rc -eq 3 -and $r.out -match 'names') "refused: the dev config names a stable path"
Remove-Item "$F\roaming\haetty-dev\config"
"TOTAL dev-run.ps1: $pass passed, $fail failed"
exit $fail
