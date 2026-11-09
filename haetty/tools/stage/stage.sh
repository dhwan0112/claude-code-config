# Stage helper.  stage.sh start NAME "remaining work"   — on main: commit the HANDOFF "In progress" line (gated),
#                                                        push; then a worktree + branch haetty/NAME from main.
#                stage.sh finish                         — fast-forward main to the stage branch, push, mirror,
#                                                        remove the worktree.
set -e
M=$HOME/projects/claude-code-config; S=$(dirname "$0"); WTROOT=$HOME/projects/claude-code-config-wt
case "$1" in
start)
  NAME=$2; REM=$3
  [ -z "$(git -C $M status --porcelain | grep -v -e " haetty/docs/HANDOFF.md$" -e " haetty/.gitignore$")" ] || { echo "main checkout not clean"; exit 1; }
  rm -f $HOME/m4/current-wt
  python3 -I - "$M/haetty/docs/HANDOFF.md" "$NAME" "$REM" <<'PY'
import sys, pathlib, re
p = pathlib.Path(sys.argv[1]); s = p.read_text(encoding="utf-8")
s = re.sub(r"\n> \*\*In progress:\*\*.*?\n(?=\n)", "\n", s, flags=re.S)
s = re.sub(r"\n{3,}## 1\. ", "\n\n## 1. ", s)
line =f"\n> **In progress:** stage {sys.argv[2]} (branch `haetty/{sys.argv[2]}`, worktree `~/projects/claude-code-config-wt/{sys.argv[2]}`). Remaining: {sys.argv[3]}\n"
i = s.index("\n## 1. ")
s = s[:i] + line + s[i:]
p.write_text(s, encoding="utf-8", newline="\n")
PY
  printf "HANDOFF: in progress - stage %s

Remaining: %s
(Also commits HANDOFF records pending on main, if any.)
" "$NAME" "$REM" > /tmp/stage-msg.txt
  bash $S/c.sh /tmp/stage-msg.txt | grep -E "GATE|NOT COMMITTED|^[0-9a-f]{7} "
  bash $S/push.sh
  mkdir -p $WTROOT
  git -C $M worktree add -q -b haetty/$NAME $WTROOT/$NAME main
  echo "$WTROOT/$NAME/haetty" > $HOME/m4/current-wt
  echo "worktree: $(cat $HOME/m4/current-wt)"
  ;;
finish)
  WT=$(cat $HOME/m4/current-wt); BR=$(git -C "$WT" rev-parse --abbrev-ref HEAD)
  [ -z "$(git -C "$WT" status --porcelain)" ] || { echo "worktree not clean"; exit 1; }
  git -C $M merge -q --ff-only "$BR" && git -C $M log -1 --format='%h %s'
  bash $S/push.sh
  rm -f $HOME/m4/current-wt
  git -C $M worktree remove "${WT%/haetty}" && git -C $M branch -d "$BR" && echo "removed worktree and branch $BR"
  ;;
esac
