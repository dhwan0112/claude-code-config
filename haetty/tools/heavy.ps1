# Windows side of tools/heavy.sh: a heavy Windows job (an installer self-test, a Windows end-to-end or soak run,
# native test binaries) takes one of the slots WSL's heavy.sh hands out, so Linux and Windows jobs together stay
# within $HAETTY_HEAVY_SLOTS (default 2).
#
#   pwsh -File tools\heavy.ps1 -Label TEXT -- CMD [ARGS]      run a command in a slot (waits for one)
#   . tools\heavy.ps1; Enter-HaettyHeavy TEXT                 this PowerShell process holds a slot until it exits
#
# The slot is held by `wsl.exe … heavy.sh --hold` whose stdin is a pipe from this process: when the job ends —
# or this process dies — the pipe closes and the slot is free. HAETTY_HEAVY_SLOT (passed into WSL via WSLENV)
# makes nested heavy.ps1 / heavy.sh calls run directly.
# heavy.sh is found next to this file when it lives in the WSL checkout (\\wsl.localhost\<distro>\…\tools), else
# from $env:HAETTY_HEAVY_SH = "<distro>:<linux path of tools/heavy.sh>" (copies staged outside the checkout).
# No param() block: the command's own switches (-NoProfile, …) must not be bound as this script's parameters.
$HeavyLabel = ""; $HeavyCmd = @($args)
if ($HeavyCmd.Count -ge 2 -and $HeavyCmd[0] -eq '-Label') { $HeavyLabel = $HeavyCmd[1]; $HeavyCmd = @($HeavyCmd | Select-Object -Skip 2) }

function Get-HaettyHeavySh {
    if ($env:HAETTY_HEAVY_SH -match '^([^:]+):(/.+)$') { return @($Matches[1], $Matches[2]) }
    $p = (Resolve-Path -LiteralPath $PSScriptRoot -ErrorAction SilentlyContinue).ProviderPath
    if ($p -match '^\\\\wsl(?:\.localhost|\$)\\([^\\]+)(\\.*)$') {
        $sh = ($Matches[2] -replace '\\', '/') + "/heavy.sh"
        return @($Matches[1], $sh)
    }
    throw "heavy.ps1: tools/heavy.sh not found (run from the WSL checkout, or set HAETTY_HEAVY_SH=<distro>:<path>)"
}

function Enter-HaettyHeavy([string]$Text) {
    if ($env:HAETTY_HEAVY_SLOT) { return }
    $d, $sh = Get-HaettyHeavySh
    $psi = New-Object System.Diagnostics.ProcessStartInfo "wsl.exe"
    # One argument string (Windows PowerShell 5.1 has no ArgumentList); quotes are taken out of the label.
    $t = $Text -replace '["\\`]', ''
    $psi.Arguments = "-d $d --exec bash `"$sh`" --hold `"windows: $t`""
    $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true
    $psi.RedirectStandardInput = $true; $psi.RedirectStandardOutput = $true
    $w = @(($env:WSLENV -split ':') | Where-Object { $_ })
    foreach ($v in "HAETTY_HEAVY_DIR", "HAETTY_HEAVY_SLOTS") { if ((Test-Path "env:$v") -and $w -notcontains $v) { $w += $v } }
    $psi.Environment["WSLENV"] = $w -join ':'
    $p = [System.Diagnostics.Process]::Start($psi)
    $line = $p.StandardOutput.ReadLine()   # blocks while every slot is in use
    if ($line -notmatch '^SLOT (\d+) WAITED (\d+)$') { throw "heavy.ps1: no slot from heavy.sh ($line)" }
    if ([int]$Matches[2] -gt 0) { Write-Host "heavy: slot $($Matches[1]) after $($Matches[2]) s: $Text" }
    $env:HAETTY_HEAVY_SLOT = $Matches[1]
    if (($env:WSLENV -split ':') -notcontains "HAETTY_HEAVY_SLOT") { $env:WSLENV = (@(($env:WSLENV -split ':') | Where-Object { $_ }) + "HAETTY_HEAVY_SLOT") -join ':' }
    $global:HaettyHeavyHold = $p
}

function Exit-HaettyHeavy {
    if ($global:HaettyHeavyHold) {
        $global:HaettyHeavyHold.StandardInput.Close(); [void]$global:HaettyHeavyHold.WaitForExit(15000)
        $global:HaettyHeavyHold = $null; Remove-Item env:HAETTY_HEAVY_SLOT -ErrorAction SilentlyContinue
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    $c = @($HeavyCmd); if ($c.Count -and $c[0] -eq '--') { $c = @($c | Select-Object -Skip 1) }
    if ($c.Count -eq 0) { Write-Host "usage: heavy.ps1 -Label TEXT -- CMD [ARGS]"; exit 2 }
    if (-not $HeavyLabel) { $HeavyLabel = $c -join ' ' }
    Enter-HaettyHeavy $HeavyLabel
    try { & $c[0] @($c | Select-Object -Skip 1); $rc = $LASTEXITCODE } finally { Exit-HaettyHeavy }
    exit $rc
}
