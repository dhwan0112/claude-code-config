# Windows: the stage S3 orchestrator end to end (the Linux orchestrator.sh): a task from the palette, the worktree
# and branch, the worker's folder and arguments, states from hooks, the message gate, worktree removal. Run with
# pwsh (UTF-8) from this folder's copy with test-guard.ps1:
#   $env:HAETTY_TEST_TMP = "$env:PUBLIC\haetty-test"; pwsh -File orchestrator.ps1 -Exe C:\...\haetty.exe
# The haetty window must stay in the foreground while it runs.
param([Parameter(Mandatory = $true)][string]$Exe)
. (Join-Path $PSScriptRoot "lib.ps1")
$W = New-HaettyRun "orch"
if (-not $W) { throw "orchestrator.ps1: no run folder (lib.ps1 failed)" }
$R = "$W\proj"; New-Item -ItemType Directory $R | Out-Null
RepoGit $R init -q -b main; Set-Content "$R\a.txt" "one" -Encoding ascii; RepoGit $R add .; RepoGit $R commit -qm one
$fc = Fake-Claude $W
"worker-command = $($fc[0]) $($fc[1] -join ' ')" | Set-Content "$W\cfg\config" -Encoding utf8
function Worker { (Status).workers | Select-Object -First 1 }
Start-Gui "cmd.exe" @("/k") $R
# 1. New task through the palette: title, description, the prefilled repository and branch.
Palette "new task"; Keys "Fix the parser" 300; Keys "{ENTER}"; Keys "Handle CRLF" 300; Keys "{ENTER}"; Keys "{ENTER}" 3000
$WT = "$W\proj-tasks\t1-fix-the-parser"
if ((Test-Path $WT) -and ((RepoGit $WT branch --show-current) -eq "task/t1-fix-the-parser")) { Ok "worktree beside the repository on task/t1-fix-the-parser" } else { Bad "worktree" (Get-ChildItem "$W\proj-tasks" -ErrorAction SilentlyContinue | Out-String) }
if (Test-Path "$W\state\tasks\t1.task") { Ok "task file tasks\t1.task" } else { Bad "task file" "missing" }
$st = (Get-Content "$W\claude-starts.log" -ErrorAction SilentlyContinue | Select-Object -Last 1)
$parts = "$st" -split '\|', 3
if ($parts.Count -eq 3 -and $parts[1].TrimEnd('\') -eq $WT) { Ok "worker started in the worktree" } else { Bad "worker folder" "$st" }
if ($parts.Count -eq 3 -and $parts[2].StartsWith("[haetty task t1: Fix the parser]") -and $parts[2] -match '\[--settings\]' -and $parts[2] -notmatch 'dangerously|permission-mode') {
    Ok "worker arguments: the task prompt, then only haetty's --settings" } else { Bad "worker args" "$st" }
$S = (Worker).session
if ((Worker).state -eq "no report" -and $S) { Ok "status --json: worker t1, its session, no report yet" } else { Bad "status json" ((Status).workers | ConvertTo-Json -Compress) }
# 2. States from hooks; the tab name carries the state glyph.
Report $S "hook SessionStart" '{"session_id":"cc-1"}'; Report $S "hook UserPromptSubmit" '{"session_id":"cc-1"}'; Start-Sleep 0.5
if ((Worker).state -eq "running") { Ok "UserPromptSubmit -> running" } else { Bad "running" (Worker).state }
Keys "%1"; Keys "%2"
$n = ((Last)[1] -split "`t")[0]
if ($n -eq "… Fix the parser") { Ok "tab name: … Fix the parser" } else { Bad "tab name" $n }
# The worker command's own arguments (here powershell's -File fake.ps1) are saved; the task prompt never is.
$saved = ((Last)[1] -split "`t")[3]
if ($saved -notmatch 'Fix the parser|STATUS') { Ok "the tab set stores no task prompt" } else { Bad "saved args" "the task prompt is in them" }
# 3. A question waits: the orchestrator holds.
Report $S "hook PermissionRequest" '{"session_id":"cc-1","tool_name":"Bash"}'; Start-Sleep 0.4
Palette "message"
$len = if (Test-Path "$W\claude-input.txt") { (Get-Item "$W\claude-input.txt").Length } else { 0 }
if ($len -eq 0) { Ok "question waiting: Message the worker does not open, nothing sent" } else { Bad "held on ask" "$len bytes" }
Keys "{ESC}"
Report $S "hook PostToolUse" '{"session_id":"cc-1","tool_name":"Bash"}'
Report $S "hook Stop" '{"session_id":"cc-1","last_assistant_message":"Done.\nSTATUS: verified\nARTIFACT: src/parser.rs\nEVIDENCE: cargo test -> 12 passed\nBLOCKERS: none\nNEXT_ACTION: review task/t1-fix-the-parser"}'
Start-Sleep 0.6
if ((Worker).state -eq "verified" -and (Worker).evidence -eq "cargo test -> 12 passed") { Ok "Stop report -> verified, evidence in status --json" } else { Bad "verified" "$((Worker).state) / $((Worker).evidence)" }
Keys "%1"; Keys "%2"
$n = ((Last)[1] -split "`t")[0]
if ($n -eq "✓ Fix the parser") { Ok "tab name: ✓ Fix the parser" } else { Bad "tab name verified" $n }
# 4. Ready: one paste and Enter reach the worker.
Palette "message"; Keys "please add tests" 300; Keys "{ENTER}" 1000
$in = if (Test-Path "$W\claude-input.txt") { Get-Content "$W\claude-input.txt" -Raw } else { "" }
if ($in -match "please add tests" -and $in.EndsWith("`r")) { Ok "message delivered (text + Enter)" } else { Bad "message" ($in -replace "`r", '\r') }
foreach ($t in "Fix the parser", "Handle CRLF", "please add tests") { if (Select-String -Path "$W\gui.log" -Pattern $t -SimpleMatch -Quiet) { Bad "log" "typed text in the log: $t" } }
Ok "no task title, description or message in the debug log (unless reported above)"
# 5. Blocked; cleanup on command only.
Report $S "hook UserPromptSubmit" '{"session_id":"cc-1"}'; Report $S "hook Stop" '{"session_id":"cc-1","last_assistant_message":"STATUS: blocked\nBLOCKERS: no network"}'; Start-Sleep 0.5
if ((Worker).state -eq "blocked") { Ok "-> blocked" } else { Bad "blocked" (Worker).state }
Palette "remove a task"; Keys "{ENTER}"
if (Test-Path $WT) { Ok "worker open: worktree not removed" } else { Bad "remove while open" "removed" }
Keys "^+w"; Keys "^+w" 1000
Set-Content "$WT\new.txt" "dirty"; Palette "remove a task"; Keys "{ENTER}" 1500
if ((Test-Path $WT) -and (Test-Path "$W\state\tasks\t1.task")) { Ok "untracked file in the worktree: refused (no --force)" } else { Bad "dirty remove" "removed" }
Remove-Item "$WT\new.txt"; Palette "remove a task"; Keys "{ENTER}" 2000
if (-not (Test-Path $WT) -and -not (Test-Path "$W\state\tasks\t1.task") -and (RepoGit $R rev-parse -q --verify refs/heads/task/t1-fix-the-parser)) { Ok "removed on command; branch kept" } else { Bad "remove" "worktree $(Test-Path $WT)" }
Stop-Gui
Total "orchestrator (Windows)"
exit $script:F
