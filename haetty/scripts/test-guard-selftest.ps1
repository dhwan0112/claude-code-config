# Self-test of test-guard.ps1 under both Windows PowerShell 5.1 (powershell.exe, what windows-smoke.ps1 is run
# with) and PowerShell 7 (pwsh): each base below must be refused or accepted as stated. Run:
#   powershell -ExecutionPolicy Bypass -File scripts\test-guard-selftest.ps1
# It writes only in $env:PUBLIC\haetty-test\guard-selftest (an accepted base) and removes it.
$ErrorActionPreference = "Stop"
$guard = Join-Path $PSScriptRoot "test-guard.ps1"
$ok = Join-Path $env:PUBLIC "haetty-test\guard-selftest"
$cases = @(
    @("", "refuse"), @("relative\dir", "refuse"), @("C:relative", "refuse"), @("\rooted-no-drive", "refuse"),
    @("$env:APPDATA\haetty-test", "refuse"), @("$env:LOCALAPPDATA\x", "refuse"), @("$env:USERPROFILE", "refuse"),
    @($ok, "accept")
)
$fail = 0
foreach ($shell in "powershell.exe", "pwsh.exe") {
    if (-not (Get-Command $shell -ErrorAction SilentlyContinue)) { Write-Host "SKIP $shell (not installed)"; continue }
    foreach ($c in $cases) {
        $base, $want = $c
        $env:GUARD_SELFTEST_BASE = $base
        $script = "`$ErrorActionPreference='Continue'; `$env:HAETTY_TEST_TMP=`$env:GUARD_SELFTEST_BASE; Remove-Item env:HAETTY_RUN -ErrorAction SilentlyContinue; . '$guard'; try { `$r = Initialize-HaettyGuard -Name HAETTY_TEST_TMP; 'ACCEPT ' + `$r } catch { 'REFUSE ' + `$_ }"
        $out = & $shell -NoProfile -ExecutionPolicy Bypass -Command $script 2>&1 | Out-String
        $got = if ($out -match '(?m)^ACCEPT ') { "accept" } elseif ($out -match '(?m)^REFUSE test-guard: refusing') { "refuse" } else { "error" }
        $shown = if ($base) { $base } else { "(empty)" }
        if ($got -eq $want) { Write-Host "PASS $shell $want $shown" } else { Write-Host "FAIL $shell $shown -> $got (want $want): $($out.Trim())"; $fail++ }
    }
}
Remove-Item env:GUARD_SELFTEST_BASE
if (Test-Path $ok) { Remove-Item -Recurse -Force $ok }
Write-Host "TOTAL guard self-test: $fail failed"
exit $fail
