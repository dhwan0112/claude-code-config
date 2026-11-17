# Windows: the settings button and the settings panel end to end (the Linux settings.sh; stage PS, DESIGN.md 2.4 and
# 3.16). A shell tab whose program records every key it reads and asks for mouse reports when told to; a Claude Code
# tab (the fake claude) asked for a permission through the hooks. Checks: the button over a shell tab's corner only
# near the pointer, gone on a key and under mouse reports; a click on the button's cell opens the panel and a click
# beside it does not (at the display's own scale, which is printed: run it once at 150 % too); Ctrl+, and the
# palette; nothing reaching the program while the panel is open; the theme preview and Esc; a font-size change (one
# session resize; saved into a config file with CRLF line ends, its comment and an unknown key kept; config.bak);
# kept after a restart; a permission-waiting tab unchanged; the real config file untouched.
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"; pwsh -File settings.ps1 -Exe C:\...\haetty.exe
# The haetty window must stay in the foreground while it runs; the pointer is moved (lib.ps1: Move-To, Click-At).
param([Parameter(Mandatory = $true)][string]$Exe)
. (Join-Path $PSScriptRoot "lib.ps1")
$W = New-HaettyRun "settings"
if (-not $W) { throw "settings.ps1: no run folder (lib.ps1 failed)" }
$realCfg = Join-Path $env:HAETTY_REAL_APPDATA "haetty\config"
function Real-Stamp { if (Test-Path -LiteralPath $realCfg) { $i = Get-Item -LiteralPath $realCfg; "$($i.LastWriteTimeUtc.Ticks) $($i.Length)" } else { "none" } }
$real0 = Real-Stamp
$env:HAETTY_DUMP_FRAME = "$W\f.ppm"
$Cols = 80; $Rows = 20
$ps = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"
# The program of the shell tab: every key it reads goes to -Log; once -Flag exists it asks for mouse reports (?1000h).
@'
param([string]$Log, [string]$Flag)
Write-Host "ready"
$asked = $false
while ($true) {
    if ([Console]::KeyAvailable) { $k = [Console]::ReadKey($true); [IO.File]::AppendAllText($Log, [string]$k.KeyChar) }
    else { Start-Sleep -Milliseconds 100 }
    if (-not $asked -and (Test-Path $Flag)) { [Console]::Write([string][char]27 + "[?1000h"); $asked = $true }
}
'@ | Set-Content -Path "$W\rec.ps1" -Encoding ascii
# A config file with CRLF line ends, a comment and a key haetty does not know.
$orig = "# my settings`r`nbogus-key = 1`r`n"
[IO.File]::WriteAllText("$W\cfg\config", $orig); [IO.File]::WriteAllText("$W\config.orig", $orig)

function Start-Settings([string]$Program, [string[]]$Arguments) {
    $al = @("--cols", "$Cols", "--rows", "$Rows", "--restore-tabs", "false") + (Engine-Args) + @($Program) + $Arguments
    $script:Gui = Start-Process -FilePath $Exe -ArgumentList $al -WorkingDirectory $W -PassThru
    for ($i = 0; $i -lt 60; $i++) { Start-Sleep -Milliseconds 250; $script:Gui.Refresh(); if ($script:Gui.MainWindowHandle -ne 0) { break } }
    Start-Sleep 2
    Focus-Diag "window up"
}
# The newest frame (HAETTY_DUMP_FRAME, a PPM "P6 w h 255"), read whole; retried while the window is writing it.
function Frame {
    Start-Sleep -Milliseconds 600
    for ($t = 0; $t -lt 10; $t++) {
        try {
            $b = [IO.File]::ReadAllBytes("$W\f.ppm")
            $i = 0; $tok = @()
            while ($tok.Count -lt 4) {
                $s = $i; while ($b[$i] -gt 32) { $i++ }
                $tok += [Text.Encoding]::ASCII.GetString($b, $s, $i - $s)
                if ($tok.Count -lt 4) { while ($b[$i] -le 32) { $i++ } }
            }
            $i++   # the one separator after the maximum value
            $w = [int]$tok[1]; $h = [int]$tok[2]
            if ($b.Length -eq $i + 3 * $w * $h) { return @{ w = $w; h = $h; off = $i; px = $b } }
        } catch {}
        Start-Sleep -Milliseconds 200
    }
    throw "settings.ps1: no complete frame in $W\f.ppm"
}
# Pixels that are not the background (the frame's top-left pixel) in the bottom-right cell's area.
function Ink {
    $f = Frame; $cw = [int][Math]::Floor($f.w / $Cols); $ch = [int][Math]::Floor($f.h / $Rows)
    $px = $f.px; $o = $f.off; $n = 0
    for ($y = $f.h - $ch - 6; $y -lt $f.h; $y++) {
        for ($x = $f.w - $cw - 10; $x -lt $f.w; $x++) {
            $i = $o + 3 * ($y * $f.w + $x)
            if ($px[$i] -ne $px[$o] -or $px[$i + 1] -ne $px[$o + 1] -or $px[$i + 2] -ne $px[$o + 2]) { $n++ }
        }
    }
    $n
}
function BgCol { $f = Frame; "#{0:x2}{1:x2}{2:x2}" -f $f.px[$f.off], $f.px[$f.off + 1], $f.px[$f.off + 2] }
function Count-Log([string]$Pattern) { @(Select-String -Path "$W\gui.log" -Pattern $Pattern -SimpleMatch -ErrorAction SilentlyContinue).Count }
function Size-Of([string]$File) { if (Test-Path $File) { (Get-Item $File).Length } else { 0 } }
function Session-Of([string]$Name) { @((Status).sessions | Where-Object { $_.program -match $Name }) | Select-Object -Last 1 }
function Wheel([int]$Notches) {
    if (Someone-Active -or -not (Front)) { Write-Host "STOP: input from somebody else or the window is not in front; the wheel is not turned"; Stop-Gui; Total "aborted"; exit 2 }
    $d = if ($Notches -ge 0) { [uint32](120 * $Notches) } else { [uint32](4294967296 + 120 * $Notches) }
    [HW]::mouse_event(0x0800, 0, 0, $d, [UIntPtr]::Zero); $script:LastSent = Last-Input; Start-Sleep -Milliseconds 200
}

# --- A shell tab ---------------------------------------------------------------------------------------------
Start-Settings $ps @("-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", "$W\rec.ps1", "-Log", "$W\shell-in.log", "-Flag", "$W\mouse-on")
$cs = Client-Size; $WW = $cs[0]; $WH = $cs[1]
$f0 = Frame; $cellW = [int][Math]::Floor($f0.w / $Cols)
$scale = (Select-String -Path "$W\gui.log" -Pattern 'at scale ([0-9.]+)' | Select-Object -Last 1 | ForEach-Object { $_.Matches[0].Groups[1].Value })
Write-Host "window: client area ${WW}x$WH px, frame $($f0.w)x$($f0.h) px, cell $cellW px wide, scale $scale"
function Corner { Move-To ($WW - 3) ($WH - 3) }
function Away { Move-To ([int]($WW / 2)) ([int]($WH / 3)) }
Away
$n = Ink; if ($n -eq 0) { Ok "shell tab: no button while the pointer is away" } else { Bad "button away" "$n px" }
Corner; $n = Ink
if ($n -gt 0) { Ok "shell tab: the button near the corner ($n px)" } else { Bad "button near" "0 px" }
Keys "a" 300
$n = Ink; if ($n -eq 0) { Ok "a key hides the button" } else { Bad "key hides" "$n px" }
Away; Corner
$r0 = Count-Log "resize session"
# A click one cell beside the button's cell is not the button (two cells to the left of the last one).
Click-At ($WW - 3 - 2 * $cellW) ($WH - 3)
if ((Count-Log "settings: opened") -eq 0) { Ok "a click beside the button's cell does not open the panel (scale $scale)" } else { Bad "click beside" "the panel opened" }
Away; Corner
# 1. Open by the button.
$b0 = Size-Of "$W\shell-in.log"
Click-At ($WW - 3) ($WH - 3)
if ((Count-Log "settings: opened") -eq 1) { Ok "a click on the button's cell opens the panel (scale $scale)" } else { Bad "button click" "$(Count-Log 'settings: opened') opens" }
# Typed into the panel: nothing reaches the program.
Keys "abc/xyz" 300; Keys "{BS}"; Keys "{ESC}"; Keys "{DOWN}"; Keys "{UP}"; Keys "{TAB}"; Keys "+{TAB}"
Click-At ($WW - 3) ($WH - 3) 300; Wheel 1; Wheel -1
Keys "{ESC}"
if ((Count-Log "settings: closed") -ge 1) { Ok "Esc closes the panel" } else { Bad "Esc" "no close" }
$b1 = Size-Of "$W\shell-in.log"
if ($b1 -eq $b0) { Ok "nothing reached the program while the panel was open ($b0 bytes before and after)" } else { Bad "bytes" "$b0 -> $b1" }
# 2. Ctrl+, and 3. the palette.
Keys "^,"; if ((Count-Log "settings: opened") -eq 2) { Ok "Ctrl+, opens the panel" } else { Bad "Ctrl+," "$(Count-Log 'settings: opened')" }
Keys "^,"; if ((Count-Log "settings: closed") -eq 2) { Ok "Ctrl+, closes it again" } else { Bad "Ctrl+, close" "$(Count-Log 'settings: closed')" }
Palette "settings"; if ((Count-Log "settings: opened") -eq 3) { Ok "the palette's Settings opens the panel" } else { Bad "palette" "$(Count-Log 'settings: opened')" }
# Theme preview: the list moves, the window shows it; Esc goes back; nothing written.
$c0 = BgCol
Keys "{ENTER}"; Keys "{DOWN}"; $c1 = BgCol
Keys "{ESC}"; $c2 = BgCol
if ($c1 -ne $c0 -and $c2 -eq $c0) { Ok "theme preview ($c0 -> $c1) and Esc back ($c2)" } else { Bad "preview" "$c0 $c1 $c2" }
if ([IO.File]::ReadAllText("$W\cfg\config") -ceq $orig) { Ok "a preview writes nothing" } else { Bad "preview wrote" ([IO.File]::ReadAllText("$W\cfg\config") -replace "`r", '\r' -replace "`n", '\n') }
# Font size: three steps quickly; one resize; the file keeps its other lines and its CRLF line ends.
$cols0 = (Session-Of "powershell").cols
Keys "{TAB}"; Keys "{DOWN}"; Keys "{RIGHT}{RIGHT}{RIGHT}" 1200
$cols1 = (Session-Of "powershell").cols
$r1 = Count-Log "resize session"
if ($cols1 -lt $cols0) { Ok "font size 11 -> 14: the grid $cols0 -> $cols1 columns" } else { Bad "grid" "$cols0 -> $cols1" }
if (($r1 - $r0) -eq 1) { Ok "three quick steps: one session resize" } else { Bad "resizes" "$($r1 - $r0)" }
$text = [IO.File]::ReadAllText("$W\cfg\config")
$shown = $text -replace "`r", '\r' -replace "`n", '\n'
if ($text -match '(?m)^font-size = 14\r$' -and $text -match '(?m)^# my settings\r$' -and $text -match '(?m)^bogus-key = 1\r$') { Ok "saved: font-size = 14, the comment and the unknown key kept" } else { Bad "file" $shown }
if ($text -notmatch '(?<!\r)\n' -and $text.EndsWith("`r`n")) { Ok "every line of the saved file still ends with CRLF" } else { Bad "line ends" $shown }
if ((Test-Path "$W\cfg\config.bak") -and [IO.File]::ReadAllText("$W\cfg\config.bak") -ceq $orig) { Ok "config.bak is the file before the first change" } else { Bad "backup" ((Get-ChildItem "$W\cfg").Name -join ' ') }
Keys "{ESC}"
# Mouse reports: the button stays away from the program's clicks. (ConPTY has to pass ?1000h on: the embedded
# OpenConsole does, the system's older conhost does not — HANDOFF, pitfalls.)
Set-Content "$W\mouse-on" "x"; Start-Sleep -Milliseconds 800; Away; Corner
$n = Ink; if ($n -eq 0) { Ok "no button while the program reads the mouse" } else { Bad "mouse reports" "$n px" }
Stop-Gui
# Kept after a restart: the same 80x20 grid needs a wider window at 14 pt.
Start-Settings $ps @("-NoLogo", "-NoProfile", "-Command", "Start-Sleep 600")
$WW2 = (Client-Size)[0]
if ($WW2 -gt $WW) { Ok "after a restart: font size 14 kept (window $WW -> $WW2 px wide for $Cols columns)" } else { Bad "restart" "$WW -> $WW2" }
Stop-Gui

# --- A Claude Code tab waiting for a permission ---------------------------------------------------------------
$fc = Fake-Claude $W
Start-Settings $fc[0] $fc[1]
$S = (Session-Of "claude").id
Move-To 20 20
$n = Ink
if ($n -gt 0) { Ok "Claude Code tab: the button in the status row without the pointer ($n px)" } else { Bad "claude button" "0 px" }
Report $S "hook PermissionRequest" '{"session_id":"cc-1","tool_name":"Bash"}'; Start-Sleep -Milliseconds 600
$a0 = (Session-Of "claude").attention; $k0 = Size-Of "$W\claude-input.txt"
Keys "^,"; Keys "1y" 300; Keys "{ENTER}"; Keys "{ESC}"; Keys "{ESC}"
$a1 = (Session-Of "claude").attention; $k1 = Size-Of "$W\claude-input.txt"
if ($a0 -eq "Ask" -and $a1 -eq "Ask") { Ok "permission wait: Ask before and after the panel" } else { Bad "attention" "$a0 -> $a1" }
if ($k0 -eq $k1) { Ok "permission wait: nothing reached Claude Code" } else { Bad "claude bytes" "$k0 -> $k1" }
Stop-Gui

if ((Real-Stamp) -eq $real0) { Ok "the real config file is untouched" } else { Bad "real config" "$real0 -> $(Real-Stamp)" }
Total "settings (Windows)"
exit $script:F
