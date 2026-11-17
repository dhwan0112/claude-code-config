# Windows: the claude proxy (stage PH, H1; written, NOT run on Windows yet). A proxy folder with claude.cmd as the
# daemon writes it (`"<haetty.exe>" claude-proxy %*`), a fake real claude.cmd that records its arguments, and the
# environment a shell tab gets. Checks what needs no terminal: the real claude runs with the same arguments and its
# exit code comes back, `-p` passes unchanged, a missing real claude gives exit 127 and one line; cmd and PowerShell
# both find claude.cmd first. The injected --settings needs a terminal (stdout): a person check (§6.1, "Windows
# proxy"). Isolation: test-guard.ps1 run directory.
param([string]$Exe)
$ErrorActionPreference = "Continue"
. (Join-Path $PSScriptRoot "..\..\test-guard.ps1")
$run = Initialize-HaettyGuard -Name HAETTY_TEST_TMP
$w = Join-Path $run "proxy"; Assert-HaettyPath $w
foreach ($d in "$w", "$w\bin", "$w\real") { New-Item -ItemType Directory -Force $d | Out-Null }
[IO.File]::WriteAllText("$w\bin\claude.cmd", "@`"$Exe`" claude-proxy %*`r`n")
[IO.File]::WriteAllText("$w\real\claude.cmd", "@echo %* > `"$w\args.txt`"`r`n@exit /b 7`r`n")
[IO.File]::WriteAllText("$w\settings-s1.json", '{"hooks":{},"statusLine":{"type":"command","command":"x"}}')
# PATH: the proxy folder, the fake real claude and Windows itself — nothing of the person's PATH. With it behind
# these, "no real claude" below found the installed Claude Code and started it (stage W1, 2026-11-17).
$env:PATH = "$w\bin;$w\real;$env:SystemRoot\System32;$env:SystemRoot;$env:SystemRoot\System32\WindowsPowerShell\v1.0"
$env:HAETTY_PROXY_DIR = "$w\bin"; $env:HAETTY_CLAUDE_SETTINGS = "$w\settings-s1.json"
$env:USERPROFILE = $w; $env:HOME = $w
Remove-Item Env:HAETTY_PROXIED -ErrorAction SilentlyContinue
$p = 0; $f = 0
function Check($name, $ok, $why) { if ($ok) { $script:p++; Write-Host "PASS $name" } else { $script:f++; Write-Host "FAIL ${name}: $why" } }
foreach ($shell in @("cmd", "powershell")) {
    Remove-Item "$w\args.txt" -ErrorAction SilentlyContinue
    if ($shell -eq "cmd") { cmd /c "claude --model haiku hello" | Out-Null } else { powershell -NoProfile -Command "claude --model haiku hello; exit `$LASTEXITCODE" | Out-Null }   # (-Command alone answers 0 or 1)
    $code = $LASTEXITCODE
    $args_ = (Get-Content "$w\args.txt" -Raw -ErrorAction SilentlyContinue)
    Check "$shell finds claude.cmd in the proxy folder; the real claude ran with the same arguments" ($args_ -match "--model haiku hello") "args: $args_"
    Check "${shell}: the real claude's exit code (7)" ($code -eq 7) "exit $code"
}
Remove-Item "$w\args.txt" -ErrorAction SilentlyContinue
cmd /c "claude -p hi" | Out-Null
Check "-p passes unchanged (no --settings)" (-not ((Get-Content "$w\args.txt" -Raw) -match "--settings")) (Get-Content "$w\args.txt" -Raw)
Remove-Item "$w\real\claude.cmd"
$out = cmd /c "claude 2>&1"; $code = $LASTEXITCODE
Check "no real claude: exit 127" ($code -eq 127) "exit $code"
Check "no real claude: one line" ("$out" -match "haetty: claude: command not found") "$out"
Write-Host "TOTAL proxy.ps1: $p passed, $f failed"
if ($f -gt 0) { exit 1 }
