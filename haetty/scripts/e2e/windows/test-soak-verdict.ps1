# Runs the Windows soak verdict on the synthetic series (scripts/e2e/soak-cases/windows) and compares each with expect.txt.
. (Join-Path $PSScriptRoot "soak-verdict.ps1")
$cases = Join-Path $PSScriptRoot "..\soak-cases"
$bad = 0
foreach ($line in Get-Content (Join-Path $cases "expect.txt")) {
    if (-not $line.Trim()) { continue }
    $name, $want = $line -split '\s+'
    $r = Get-SoakVerdict (Read-SoakSeries (Join-Path $cases "windows\$name.csv")) 3600 "yes"
    $got = if ($r.Ok) { "PASS" } else { "FAIL" }
    if ($got -ne $want) { $bad++ }
    "{0,-9} {1,-16} want {2} got {3}" -f $(if ($got -eq $want) { "ok" } else { "MISMATCH" }), $name, $want, $got
}
"mismatches: $bad"
exit $bad
