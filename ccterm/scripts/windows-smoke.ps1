# ccterm Windows smoke test. Run from the folder that holds ccterm-headless.exe:
#   powershell -ExecutionPolicy Bypass -File windows-smoke.ps1
# Each check prints PASS/FAIL; the full screen of a failed check is shown.
$ErrorActionPreference = "Continue"
$exe = Join-Path $PSScriptRoot "ccterm-headless.exe"
if (-not (Test-Path $exe)) { $exe = "ccterm-headless.exe" }
$fail = 0

function Check($name, [string[]]$hargs) {
    $out = & $exe @hargs 2>&1 | Out-String
    if ($LASTEXITCODE -eq 0) { Write-Host "PASS $name" }
    else { Write-Host "FAIL $name (exit $LASTEXITCODE)"; Write-Host $out; $script:fail++ }
    return $out
}

# 1. Which ConPTY host is used (expect "conpty embedded 1.25...").
& $exe --trace --cols 80 --rows 10 -e "wait-exit 5000" --dump none -- cmd.exe /c "echo hi" 2>&1 | Select-String "\[pty\]"

# 2. Plain output, wide characters, exit code.
Check "cmd echo" @("--cols","80","--rows","10","-e","wait-text ok-한글 5000","-e","wait-exit 5000","--","cmd.exe","/c","echo ok-한글")

# 3. Typing into an interactive shell.
Check "pwsh input" @("--cols","100","--rows","20","-e","wait-idle 1500 15000","-e","type Write-Output (6*7)\n","-e","wait-text 42 10000","--","powershell.exe","-NoLogo","-NoProfile")

# 4. Resize reaches the child.
Check "resize" @("--cols","100","--rows","20","-e","wait-idle 1500 15000","-e","resize 77 21","-e","type `$Host.UI.RawUI.WindowSize.Width\n","-e","wait-text 77 10000","--","powershell.exe","-NoLogo","-NoProfile")

# 5. Claude Code, if installed: fullscreen renderer, modes and keys.
$claude = Get-Command claude -ErrorAction SilentlyContinue
if ($claude) {
    $out = & $exe --cols 120 --rows 36 --timeout 90 --env CLAUDE_CODE_NO_FLICKER=1 `
        -e "wait-idle 3000 60000" -e "modes" -e "print" -e "type 안녕 hello" -e "key shift+Enter" -e "type second line" `
        -e "wait-idle 800 5000" -e "print" -e "key ctrl+c" -e "key ctrl+c" -e "wait-exit 15000" --dump none -- $claude.Source 2>&1 | Out-String
    Write-Host $out
    if ($out -match "alt_screen: true" -and $out -match "mouse_tracking: true") { Write-Host "PASS claude fullscreen modes" }
    else { Write-Host "FAIL claude fullscreen modes (alt screen / mouse tracking not seen)"; $fail++ }
    if ($out -match "second line") { Write-Host "PASS claude multi-line input" } else { Write-Host "FAIL claude multi-line input"; $fail++ }
} else {
    Write-Host "SKIP claude (not on PATH)"
}

Write-Host "`n$fail failure(s)"
exit $fail
