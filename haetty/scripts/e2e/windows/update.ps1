# Windows: the in-app update end to end (the Linux update.sh, stage PU) — NOT RUN YET: written on Linux, to be run
# on the Windows PC (docs/dev-commands.md, Windows check list). What it covers that Linux cannot: Schannel and the
# Windows certificate store (the test CA is given through HAETTY_UPDATE_TEST_ROOT), renaming the running haetty.exe
# (form b), the junction `current` (form a), and the Start menu shortcut through `current`.
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"
#   pwsh -File update.ps1 -Old C:\...\haetty-0.90.0.exe -New C:\...\haetty-0.90.1.exe -Server C:\...\fake_release_server.exe -Ca C:\...\TEST-ONLY-ca.pem
# The server is Linux-only (rustls is not in the Windows build): run `fake_release_server` in WSL on the folder this
# script fills ($W\srv, through \\wsl$ or a shared path) and pass -Server as a script that starts it there; WSL2
# forwards localhost, which the TEST-ONLY certificate names.
# Old and New are `--features update-test` builds with HAETTY_BUILD_VERSION 0.90.0 / 0.90.1 and HAETTY_BUILD_COMMIT
# aaaaaaaaaaaa / bbbbbbbbbbbb (the same as update.sh). The haetty window must stay in the foreground while it runs.
param(
    [Parameter(Mandatory = $true)][string]$Old,
    [Parameter(Mandatory = $true)][string]$New,
    [Parameter(Mandatory = $true)][string]$Server,
    [Parameter(Mandatory = $true)][string]$Ca
)
. (Join-Path $PSScriptRoot "lib.ps1")
$W = New-HaettyRun "update"
if (-not $W) { throw "update.ps1: no run folder (lib.ps1 failed)" }
$B = "bbbbbbbbbbbb0000000000000000000000000000"
# The release: haetty.exe of New, LICENSE; a zip; the manifest written by New itself.
$pack = "$W\pack"; $srv = "$W\srv"; New-Item -ItemType Directory $pack, $srv | Out-Null
Copy-Item $New "$pack\haetty.exe"; Set-Content "$pack\LICENSE" "MIT" -Encoding ascii
Compress-Archive -Path "$pack\haetty.exe", "$pack\LICENSE" -DestinationPath "$srv\haetty-0.90.1-x86_64-windows.zip"
& $New release-manifest --version 0.90.1 --commit $B --built 2026-11-20T00:00:00Z --protocol 12 `
    --asset "x86_64-windows=$srv\haetty-0.90.1-x86_64-windows.zip:haetty.exe,LICENSE" | Set-Content "$srv\manifest.json" -Encoding utf8
$sp = Start-Process $Server -ArgumentList $srv -PassThru -WindowStyle Hidden
for ($i = 0; $i -lt 50 -and -not (Test-Path "$srv\port"); $i++) { Start-Sleep -Milliseconds 100 }
$port = (Get-Content "$srv\port").Trim()
$env:HAETTY_UPDATE_TEST = "1"; $env:HAETTY_UPDATE_TEST_FIRST_MS = "1500"; $env:HAETTY_UPDATE_TEST_ROOT = $Ca
"update-url = https://localhost:$port/r" | Set-Content "$W\cfg\config" -Encoding utf8
# Form (b): a single executable in a folder of its own.
$single = "$W\single"; New-Item -ItemType Directory $single | Out-Null; Copy-Item $Old "$single\haetty.exe"
$Exe = "$single\haetty.exe"   # Start-Gui starts $Exe
Start-Gui "cmd.exe" @("/k") $W
Start-Sleep 4
Palette "updates"; Keys "{DOWN}{DOWN}{DOWN}" 300; Keys "{ENTER}" 8000
$v = (& "$single\haetty.exe" --version 2>&1 | Out-String)
if ($v -match "bbbbbbbbbbbb") { Ok "(b) haetty.exe replaced while running: $($v.Trim())" } else { Bad "(b) version" $v }
if (Test-Path "$single\haetty.exe.old") { Ok "(b) the running file was renamed to haetty.exe.old" } else { Bad "(b) old" "missing" }
Stop-Gui
Stop-Process -Id $sp.Id -ErrorAction SilentlyContinue
Total "update.ps1"   # form (a), the junction and the Start menu: by hand (Windows check list)
