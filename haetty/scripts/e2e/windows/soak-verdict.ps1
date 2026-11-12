# The verdict of a Windows soak series (soak.ps1). The Linux twin is scripts/e2e/soak-verdict.py: the same rule and
# numbers; scripts/e2e/soak-cases holds synthetic series that both must judge alike (test-soak-verdict.ps1 here).
#
# Rule (F0, 2026-11-08):
# - threads last <= threads at 5 min + 3 (Windows' own pool threads); handles last <= handles at 5 min + 20;
#   descendants and sessions: the last sample equals the first; task worktrees 0 at the end.
# - Full, per session: a session is full from the first sample from which its scrollback rows no longer grow by
#   more than max(100 rows, 1 %) over the sample it is at (samples where it is alive). A session that never grew
#   by more than 100 rows does not count. Baseline = the latest full time over the sessions that grew. A session
#   still growing at the last sample: no baseline (FAIL).
# - From the baseline on, the window must be at least half the run, at most 1800 s (P1c: 300 s for a 600 s run), and the GUI's and the daemon's private working set at
#   the last sample at most 4 MB above their values at the baseline sample.
# - Operations: the counts of the run are done (200/50/20/10 at 3600 s, scaled as soak.ps1 scales them); the window
#   and the daemon are alive at the end.
# A series without the sessions_rows column predates this rule: not judged, FAIL.

$script:SoakTolRows = 100
$script:SoakTolFrac = 0.01
$script:SoakWindowMaxS = 1800
$script:SoakBarKb = 4096
$script:SoakOps = @(@("tabs_done", 200), @("detach_done", 50), @("tasks_done", 20), @("themes_done", 10))

# The rows of a series: numbers as ints, sessions_rows as a hashtable id -> rows; Cols: the header's names.
function Read-SoakSeries([string]$Path) {
    $raw = @(Import-Csv -LiteralPath $Path)
    $cols = if ($raw.Count) { @($raw[0].PSObject.Properties.Name) } else { @() }
    $rows = foreach ($r in $raw) {
        $o = @{}
        foreach ($p in $r.PSObject.Properties) {
            if ($p.Name -eq "sessions_rows") {
                $m = @{}
                foreach ($part in @($p.Value -split ';' | Where-Object { $_ })) {
                    $i = $part.LastIndexOf(':')
                    $m[$part.Substring(0, $i)] = [int]$part.Substring($i + 1)
                }
                $o[$p.Name] = $m
            } else {
                $o[$p.Name] = [int]$p.Value
            }
        }
        $o
    }
    return @{ Cols = $cols; Rows = @($rows) }
}

# { Tb = baseline seconds, or $null when a session is still growing at the last sample; Note }
function Get-SoakBaseline($Rows) {
    $ids = New-Object System.Collections.Generic.List[string]
    foreach ($r in $Rows) {
        foreach ($k in $r.sessions_rows.Keys) { if (-not $ids.Contains($k)) { $ids.Add($k) } }
    }
    $tb = 0
    $notes = @()
    foreach ($sid in $ids) {
        $idx = @()
        for ($i = 0; $i -lt $Rows.Count; $i++) { if ($Rows[$i].sessions_rows.ContainsKey($sid)) { $idx += $i } }
        $vals = @($idx | ForEach-Object { [int]$Rows[$_].sessions_rows[$sid] })
        $grew = ($vals | Measure-Object -Maximum).Maximum - $vals[0]
        if ($grew -le $script:SoakTolRows) { continue }
        $fill = -1
        for ($k = 0; $k -lt $idx.Count; $k++) {
            $tol = [Math]::Max($script:SoakTolRows, $script:SoakTolFrac * $vals[$k])
            $allBelow = $true
            for ($j = $k; $j -lt $vals.Count; $j++) {
                if ($vals[$j] -gt $vals[$k] + $tol) { $allBelow = $false; break }
            }
            if ($allBelow) { $fill = $k; break }
        }
        if (($fill -eq ($idx.Count - 1)) -and ($idx.Count -gt 1)) {
            return @{ Tb = $null; Note = "$sid still growing at the last sample ($($vals[-1]) rows)" }
        }
        $tf = [int]$Rows[$idx[$fill]].t_s
        $notes += "$sid full at $tf s ($($vals[-1]) rows)"
        $tb = [Math]::Max($tb, $tf)
    }
    $note = if ($notes.Count) { $notes -join "; " } else { "no session grew by more than 100 rows" }
    return @{ Tb = $tb; Note = $note }
}

function Get-SoakVerdict($Series, [int]$Dur, [string]$Alive) {
    $lines = New-Object System.Collections.Generic.List[string]
    $script:SoakOk = $true
    function Add-SoakCheck([string]$Name, [bool]$Cond, [string]$Detail) {
        $script:SoakOk = $script:SoakOk -and $Cond
        $tag = if ($Cond) { "PASS " } else { "FAIL " }
        $lines.Add($tag + $Name + ": " + $Detail)
    }
    $rows = $Series.Rows
    if ($rows.Count -eq 0) {
        Add-SoakCheck "series" $false "no samples"
    } elseif (-not ($Series.Cols -contains "sessions_rows")) {
        Add-SoakCheck "scrollback" $false "no sessions_rows column: this series predates the rule, not judged"
    } else {
        $first = $rows[0]
        $last = $rows[-1]
        $at5 = @($rows | Sort-Object { [Math]::Abs($_.t_s - 300) })[0]
        if ($rows.Count -lt 3) { $at5 = $first }
        Add-SoakCheck "gui_threads" ($last.gui_threads -le $at5.gui_threads + 3) "$($at5.gui_threads) at $($at5.t_s) s -> $($last.gui_threads)"
        Add-SoakCheck "daemon_threads" ($last.daemon_threads -le $at5.daemon_threads + 3) "$($at5.daemon_threads) at $($at5.t_s) s -> $($last.daemon_threads)"
        Add-SoakCheck "gui_handles" ($last.gui_handles -le $at5.gui_handles + 20) "$($at5.gui_handles) at $($at5.t_s) s -> $($last.gui_handles)"
        Add-SoakCheck "daemon_handles" ($last.daemon_handles -le $at5.daemon_handles + 20) "$($at5.daemon_handles) at $($at5.t_s) s -> $($last.daemon_handles)"
        Add-SoakCheck "daemon_descendants" ($last.daemon_descendants -eq $first.daemon_descendants) "$($first.daemon_descendants) -> $($last.daemon_descendants)"
        Add-SoakCheck "sessions" ($last.sessions -eq $first.sessions) "$($first.sessions) -> $($last.sessions)"
        Add-SoakCheck "worktrees" ($last.worktrees -eq 0) "$($last.worktrees)"
        $f = [Math]::Min(1.0, $Dur / 3600)
        foreach ($o in $script:SoakOps) {
            $want = [Math]::Max(1, [int][Math]::Round($o[1] * $f, [MidpointRounding]::ToEven))
            Add-SoakCheck $o[0] ($last.($o[0]) -eq $want) "$($last.($o[0])) of $want"
        }
        Add-SoakCheck "alive" ($Alive -eq "yes") "alive at the end: $Alive"
        $b = Get-SoakBaseline $rows
        if ($null -eq $b.Tb) {
            Add-SoakCheck "scrollback_full" $false $b.Note
        } else {
            $window = $last.t_s - $b.Tb
            $lines.Add("INFO baseline: " + $b.Note + "; baseline $($b.Tb) s")
            $need = [Math]::Min($script:SoakWindowMaxS, [Math]::Floor($Dur / 2))
            Add-SoakCheck "window" ($window -ge $need) "$window s after the baseline (need $need)"
            $base = @($rows | Where-Object { $_.t_s -eq $b.Tb })[0]
            Add-SoakCheck "gui_private_ws" ($last.gui_private_ws_kb -le $base.gui_private_ws_kb + $script:SoakBarKb) "$($base.gui_private_ws_kb) kB at $($b.Tb) s -> $($last.gui_private_ws_kb) kB"
            Add-SoakCheck "daemon_private_ws" ($last.daemon_private_ws_kb -le $base.daemon_private_ws_kb + $script:SoakBarKb) "$($base.daemon_private_ws_kb) kB at $($b.Tb) s -> $($last.daemon_private_ws_kb) kB"
        }
    }
    return @{ Lines = @($lines); Ok = $script:SoakOk }
}
