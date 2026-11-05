# Test isolation for PowerShell scripts. Dot-source it, then
#
#   Initialize-HaettyGuard -Name HAETTY_TEST_TMP   # the NAME of the environment variable holding the base
#   Assert-HaettyPath $path                        # before writing or removing $path
#
# Initialize-HaettyGuard throws when the variable is unset, empty or not a
# fully qualified path (a relative base, or `C:foo`, lands in the current
# folder), or when
# the base is, or is inside, the real %USERPROFILE%, ~\.config, ~\.local,
# ~\.claude, %APPDATA% or %LOCALAPPDATA% (so %TEMP% is refused: use e.g.
# $env:PUBLIC\haetty-test). It creates the run directory $env:HAETTY_RUN
# (base\haetty-run.<random>). Assert-HaettyPath throws for every path
# outside it. Same rules: crates/haetty-testkit, scripts/test-guard.sh.

if (-not $env:HAETTY_REAL_HOME) { $env:HAETTY_REAL_HOME = $env:USERPROFILE }

function Get-HaettyNormPath([string]$p) {
    if (-not $p) { return "" }
    [System.IO.Path]::GetFullPath($p).TrimEnd('\', '/').ToLowerInvariant()
}

function Get-HaettyProtected {
    $h = $env:HAETTY_REAL_HOME
    @($h, "$h\.config", "$h\.local", "$h\.claude", $env:APPDATA, $env:LOCALAPPDATA, $env:HOME) |
        Where-Object { $_ } | ForEach-Object { Get-HaettyNormPath $_ }
}

function Test-HaettyInside([string]$path, [string]$dir) {
    $p = Get-HaettyNormPath $path; $d = Get-HaettyNormPath $dir
    ($p -eq $d) -or $p.StartsWith($d + '\')
}

function Initialize-HaettyGuard([string]$Name) {
    $base = [Environment]::GetEnvironmentVariable($Name)
    if (-not $base) { throw "test-guard: refusing: `$env:$Name is unset or empty" }
    if (-not [System.IO.Path]::IsPathFullyQualified($base)) { throw "test-guard: refusing: `$env:$Name=$base is not an absolute path" }
    foreach ($d in Get-HaettyProtected) {
        if (Test-HaettyInside $base $d) { throw "test-guard: refusing: `$env:$Name=$base is (inside) the real $d" }
    }
    # Nested scripts share the run directory of the outer one.
    if ($env:HAETTY_RUN -and (Test-Path $env:HAETTY_RUN) -and (Test-HaettyInside $env:HAETTY_RUN $base) -and
        ((Get-HaettyNormPath $env:HAETTY_RUN) -ne (Get-HaettyNormPath $base))) { return $env:HAETTY_RUN }
    New-Item -ItemType Directory -Force $base | Out-Null
    $run = Join-Path $base ("haetty-run." + [IO.Path]::GetRandomFileName().Replace('.', ''))
    New-Item -ItemType Directory $run | Out-Null
    $env:HAETTY_RUN = $run
    $run
}

function Assert-HaettyPath([string]$Path) {
    if (-not $Path) { throw "test-guard: refusing: empty path" }
    if (-not $env:HAETTY_RUN) { throw "test-guard: refusing: Initialize-HaettyGuard was not called" }
    foreach ($d in Get-HaettyProtected) {
        if (Test-HaettyInside $Path $d) { throw "test-guard: refusing: $Path is (inside) the real $d" }
    }
    if (-not (Test-HaettyInside $Path $env:HAETTY_RUN)) { throw "test-guard: refusing: $Path is outside this run's directory $env:HAETTY_RUN" }
}
