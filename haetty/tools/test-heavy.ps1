# Self-test of tools/heavy.ps1 with WSL's heavy.sh: Windows and WSL jobs share the same two slots. Run from the
# WSL checkout (\\wsl.localhost\<distro>\…\haetty\tools) with pwsh; slots in a private directory under the WSL
# /tmp, not the real one. No window is opened.
#   pwsh -File tools\test-heavy.ps1
. (Join-Path $PSScriptRoot "heavy.ps1")
$d, $sh = Get-HaettyHeavySh
$env:HAETTY_HEAVY_DIR = "/tmp/haetty-heavy-test-$PID"; $env:HAETTY_HEAVY_SLOTS = "2"
Remove-Item env:HAETTY_HEAVY_SLOT -ErrorAction SilentlyContinue
$env:WSLENV = (@(($env:WSLENV -split ':') | Where-Object { $_ -and $_ -ne 'HAETTY_HEAVY_SLOT' }) + "HAETTY_HEAVY_DIR" + "HAETTY_HEAVY_SLOTS") -join ':'
$pass = 0; $fail = 0
function Ok([bool]$c, [string]$what) { if ($c) { $script:pass++; "PASS $what" } else { $script:fail++; "FAIL $what" } }
function Busy { & wsl.exe -d $d --exec bash $sh --status | Out-Null; $LASTEXITCODE }
function Hidden([string]$exe, [string]$argline) { Start-Process -FilePath $exe -ArgumentList $argline -WindowStyle Hidden -PassThru }
function Waited([string]$label) {
    $f = "\\wsl.localhost\$d" + ($env:HAETTY_HEAVY_DIR -replace '/', '\') + "\heavy.log"
    $l = Get-Content -LiteralPath $f | Where-Object { ($_ -split "`t")[1] -eq 'start' -and ($_ -split "`t")[5] -eq $label } | Select-Object -Last 1
    if ($l -match 'waited (\d+) s') { [int]$Matches[1] } else { -1 }
}
$pw = (Get-Process -Id $PID).Path
$hp = Join-Path $PSScriptRoot "heavy.ps1"

# 1. Two WSL jobs hold both slots; a Windows job waits for one.
$a = Hidden "wsl.exe" "-d $d --exec bash $sh --label wsl-one -- sleep 12"
$b = Hidden "wsl.exe" "-d $d --exec bash $sh --label wsl-two -- sleep 12"
Start-Sleep 3
Ok ((Busy) -eq 2) "two WSL jobs: 2 slots in use"
$t0 = Get-Date
& $pw -NoProfile -File $hp -Label "win-three" -- cmd /c exit 0 | Out-Null
$el = [int]((Get-Date) - $t0).TotalSeconds
Ok ($el -ge 6) "Windows job started only after a WSL job ended ($el s)"
Ok ((Waited "windows: win-three") -ge 6) "log: Windows job waited $(Waited 'windows: win-three') s"
$a.WaitForExit(); $b.WaitForExit()

# 2. Two Windows jobs hold both slots; a WSL job waits.
$c = Hidden $pw "-NoProfile -File `"$hp`" -Label win-one -- powershell -NoProfile -Command Start-Sleep 12"
$e = Hidden $pw "-NoProfile -File `"$hp`" -Label win-two -- powershell -NoProfile -Command Start-Sleep 12"
Start-Sleep 4
Ok ((Busy) -eq 2) "two Windows jobs: 2 slots in use"
$t0 = Get-Date
& wsl.exe -d $d --exec bash $sh --label wsl-three -- true
$el = [int]((Get-Date) - $t0).TotalSeconds
Ok ($el -ge 5) "WSL job started only after a Windows job ended ($el s)"
Ok ((Waited "wsl-three") -ge 5) "log: WSL job waited $(Waited 'wsl-three') s"
$c.WaitForExit(); $e.WaitForExit()

# 3. Nested: a WSL heavy.sh inside a Windows slot takes no second slot (HAETTY_HEAVY_SLOT through WSLENV).
$out = & $pw -NoProfile -File $hp -Label "win-outer" -- wsl.exe -d $d --exec bash $sh --label nested -- bash -c 'echo slot=$HAETTY_HEAVY_SLOT'
Ok (($out -match '^slot=\d') -and (Waited "nested") -eq -1) "nested WSL job ran in the Windows slot ($out)"

# 4. A Windows holder killed (by its PID) frees its slot.
$k = Hidden $pw "-NoProfile -File `"$hp`" -Label win-killed -- powershell -NoProfile -Command Start-Sleep 60"
Start-Sleep 4
Ok ((Busy) -eq 1) "Windows job: 1 slot in use"
$kids = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($k.Id)" | Select-Object -ExpandProperty ProcessId
Stop-Process -Id $k.Id -Force; Start-Sleep 2
Ok ((Busy) -eq 0) "slot free right after its Windows job was killed"
foreach ($p in $kids) { Stop-Process -Id $p -Force -ErrorAction SilentlyContinue }

& wsl.exe -d $d --exec rm -rf $env:HAETTY_HEAVY_DIR
"TOTAL heavy.ps1: $pass passed, $fail failed"
exit $fail
