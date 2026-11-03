#!/usr/bin/env bash
# Claude Code status line (two lines). Reads the session JSON on stdin.
# Line 1: model · effort | directory | git branch (worktree)
# Line 2: context bar, % and tokens | 5h / 7d usage | session cost estimate
input=$(cat)

IFS=$'\x1f' read -r model effort dir wt pct used size rl5 rl7 cost sub < <(
  jq -r '[
    (.model.display_name // .model.id // "?"),
    (.effort.level // ""),
    (.workspace.current_dir // .cwd // ""),
    (.worktree.name // .workspace.git_worktree // ""),
    ((.context_window.used_percentage // 0) | floor),
    (.context_window.current_usage as $u
      | if $u then (($u.input_tokens // 0) + ($u.cache_creation_input_tokens // 0) + ($u.cache_read_input_tokens // 0))
        else (.context_window.total_input_tokens // 0) end),
    (.context_window.context_window_size // 0),
    (.rate_limits.five_hour.used_percentage // "" | if . == "" then "" else floor end),
    (.rate_limits.seven_day.used_percentage // "" | if . == "" then "" else floor end),
    (.cost.total_cost_usd // 0),
    (if .rate_limits then "1" else "" end)
  ] | map(tostring) | join("\u001f")' <<<"$input" 2>/dev/null
)

R=$'\033[0m'; DIM=$'\033[2m'; CYN=$'\033[36m'; MAG=$'\033[35m'
GRN=$'\033[32m'; YEL=$'\033[33m'; RED=$'\033[31m'

# Short token count: 212345 -> 212k, 1000000 -> 1M
human() {
  local n=${1:-0}
  if (( n >= 1000000 )); then
    if (( n % 1000000 == 0 )); then printf '%dM' $((n / 1000000)); else printf '%d.%dM' $((n / 1000000)) $((n % 1000000 / 100000)); fi
  elif (( n >= 1000 )); then printf '%dk' $((n / 1000))
  else printf '%d' "$n"; fi
}

# Color by threshold: <50 green, <80 yellow, else red
col() { if (( $1 >= 80 )); then printf '%s' "$RED"; elif (( $1 >= 50 )); then printf '%s' "$YEL"; else printf '%s' "$GRN"; fi; }

# Line 1
line1="${CYN}${model}${R}"
[ -n "$effort" ] && line1+="${DIM} · ${R}${effort}"
short_dir=${dir/#$HOME/\~}
line1+="${DIM} │ ${R}${short_dir}"
if [ -n "$dir" ] && branch=$(git -C "$dir" --no-optional-locks symbolic-ref --short -q HEAD 2>/dev/null \
    || git -C "$dir" --no-optional-locks rev-parse --short HEAD 2>/dev/null); then
  line1+="${DIM} │ ${R}${MAG}${branch}${R}"
fi
[ -n "$wt" ] && line1+="${DIM} (wt: ${wt})${R}"

# Line 2
pct=${pct:-0}; (( pct > 100 )) && pct=100
width=10; fill=$(( (pct * width + 50) / 100 ))
bar=""; for ((i = 0; i < width; i++)); do if (( i < fill )); then bar+="█"; else bar+="░"; fi; done
line2="ctx $(col "$pct")${bar}${R} ${pct}% $(human "${used:-0}")/$(human "${size:-0}")"

limits=""
[ -n "$rl5" ] && limits+="5h $(col "$rl5")${rl5}%${R}"
if [ -n "$rl7" ]; then [ -n "$limits" ] && limits+=" "; limits+="7d $(col "$rl7")${rl7}%${R}"; fi
[ -n "$limits" ] && line2+="${DIM} │ ${R}${limits}"

cost_fmt=$(LC_ALL=C printf '%.2f' "${cost:-0}" 2>/dev/null || echo "0.00")
if [ -n "$sub" ]; then
  line2+="${DIM} │ ~\$${cost_fmt} API-equiv (subscription)${R}"
else
  line2+="${DIM} │ ${R}\$${cost_fmt} est"
fi

printf '%s\n%s\n' "$line1" "$line2"
