# Windows: stage S2 tab features end to end (the Linux tabs.sh): repo:branch names, rename, diff views, closed
# tabs reopened with --resume (never twice), detach and attach. Run from this folder's copy with test-guard.ps1:
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"; powershell -ExecutionPolicy Bypass -File tabs.ps1 -Exe C:\...\haetty.exe
# The haetty window must stay in the foreground while it runs (keys are sent to it); a check that cannot focus it
# fails instead of typing elsewhere.
param([Parameter(Mandatory = $true)][string]$Exe)
. (Join-Path $PSScriptRoot "lib.ps1")
$W = New-HaettyRun "tabs"
if (-not $W) { throw "tabs.ps1: no run folder (lib.ps1 failed)" }
$R = "$W\proj"; New-Item -ItemType Directory $R | Out-Null
RepoGit $R init -q -b main; Set-Content "$R\a.txt" "one`ntwo`nthree" -NoNewline -Encoding ascii
RepoGit $R add .; RepoGit $R commit -qm one; RepoGit $R checkout -qb feat/x
$fc = Fake-Claude $W
Start-Gui $fc[0] $fc[1] $R
$A = (Running)[0].id
# 1. Branch names; Claude Code's session name first.
if ((LastNames) -eq "proj:feat/x|0") { Ok "Claude Code tab in a repository: named proj:feat/x" } else { Bad "branch name" (LastNames) }
RepoGit $R checkout -qb other; Start-Sleep 3.5
Report $A statusline '{"session_id":"aaaaaaaa-0000-4000-8000-000000000001"}'; Start-Sleep 0.5
Keys "^+t" 1500
$B = ((Running) | Where-Object { $_.id -ne $A })[0].id
if ((LastNames) -eq "proj:other|0;proj:other|0") { Ok "branch change followed: proj:other" } else { Bad "branch follow" (LastNames) }
Report $A statusline '{"session_id":"aaaaaaaa-0000-4000-8000-000000000001","session_name":"fix parser"}'; Start-Sleep 0.5
Keys "%1"
if ((LastNames) -eq "fix parser|0;proj:other|0") { Ok "session name wins over repo:branch" } else { Bad "session name" (LastNames) }
# 2. Rename (Hangul through SendInput's Unicode keys); the name is not logged.
Keys "^+r"; Keys "my tab 한" 300; Keys "{ENTER}"
if ((LastNames) -eq "my tab 한|1;proj:other|0") { Ok "Ctrl+Shift+R renames, saved with custom=1" } else { Bad "rename" (LastNames) }
if (Select-String -Path "$W\gui.log" -Pattern "my tab" -Quiet) { Bad "log" "the typed name is in the log" } else { Ok "the typed name is not in the debug log" }
Palette "rename"; Keys ("{BS}" * 12) 300; Keys "{ENTER}"
if ((LastNames) -eq "fix parser|0;proj:other|0") { Ok "empty name: the automatic name again" } else { Bad "auto name" (LastNames) }
# 3. Diff views: an Edit hook pair on a.txt and an untracked file.
$edit = '{"session_id":"x","tool_name":"Edit","tool_input":{"file_path":"' + ("$R\a.txt" -replace '\\', '\\') + '","old_string":"two","new_string":"TWO"}}'
Report $A "hook PreToolUse" $edit
Set-Content "$R\a.txt" "one`nTWO`nthree" -NoNewline -Encoding ascii
Report $A "hook PostToolUse" $edit
Set-Content "$R\bash-made.txt" "made by bash" -Encoding ascii
Start-Sleep 0.5; Keys "^+d"; Keys "m" 1000
if (Select-String -Path "$W\gui.log" -Pattern "diff view vs HEAD: .* to collect" -Quiet) { Ok "m: vs HEAD" } else { Bad "head view" "no log line" }
Keys "m" 1500
$t = Select-String -Path "$W\gui.log" -Pattern "diff view working tree: .* to collect" | Select-Object -Last 1
if ($t) { Ok "m: working tree ($($t.Line -replace '.*diff view ', ''))" } else { Bad "tree view" "no log line" }
Keys "m"; Keys "{ESC}"
# 4. Closed tabs: tab 2 with a conversation and a transcript; reopen with --resume, never twice.
$SB = "bbbbbbbb-0000-4000-8000-000000000002"
Keys "%2"; Report $B statusline "{`"session_id`":`"$SB`"}"; Start-Sleep 0.5
# Claude Code's project key: every character that is not a letter or digit becomes '-'.
$pk = Join-Path "$W\claude\projects" ($R -replace '[^A-Za-z0-9]', '-'); New-Item -ItemType Directory -Force $pk | Out-Null; Set-Content "$pk\$SB.jsonl" "{}"
Keys "^+w"; Keys "^+w" 1000
$c = @(Get-Content "$env:HAETTY_STATE_DIR\tabs\closed.tsv" | Where-Object { $_ -and -not $_.StartsWith("#") })
if ($c.Count -eq 1 -and ($c[0] -split "`t")[4] -eq $SB) { Ok "closed tab kept with its conversation" } else { Bad "closed list" ($c -join " / ") }
Palette "reopen"; Keys "{ENTER}" 2500
if ((Get-Content "$W\claude-starts.log" -Raw) -match [regex]::Escape("[--resume][$SB]") -and (Running).Count -eq 2) { Ok "reopened with --resume $SB" } else { Bad "reopen" ((Get-Content "$W\claude-starts.log") -join " / ") }
[IO.File]::WriteAllText("$env:HAETTY_STATE_DIR\tabs\closed.tsv", "# t`nold`t$($R -replace '\\', '\\')`t$($fc[0] -replace '\\', '\\')`t`t$SB`t0`t0`n")
Palette "reopen"; Keys "{ENTER}" 2000
$n = ([regex]::Matches((Get-Content "$W\claude-starts.log" -Raw), [regex]::Escape("[--resume][$SB]"))).Count
if ((Running).Count -eq 2 -and $n -eq 1) { Ok "the running conversation is not resumed twice" } else { Bad "duplicate resume" "running $((Running).Count), resumes $n" }
# 5. Detach and attach.
Keys "%1"; Palette "detach"
$r = Running
if ($r.Count -eq 2 -and @($r | Where-Object { $_.attached -eq 0 }).Count -eq 1 -and (Last).Count -eq 1) { Ok "detach: the session runs unattached" } else { Bad "detach" "running $($r.Count), last $((Last).Count)" }
Palette "detach"
if ((Last).Count -eq 1 -and -not $script:Gui.HasExited) { Ok "the only tab is not detached" } else { Bad "detach only tab" (LastNames) }
Palette "attach detached"; Start-Sleep 1.5
if ((Last).Count -eq 2 -and @(Running | Where-Object { $_.attached -eq 0 }).Count -eq 0) { Ok "Attach detached sessions brings it back" } else { Bad "attach" (LastNames) }
Stop-Gui
Total "tabs (Windows)"
exit $script:F
