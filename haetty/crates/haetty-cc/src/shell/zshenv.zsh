# haetty's zsh startup file (DESIGN.md §13.8): written by haetty's daemon into its run folder for zsh tabs.
if [[ -n ${HAETTY_ZSH_ZDOTDIR+set} ]]; then
  builtin export ZDOTDIR=$HAETTY_ZSH_ZDOTDIR
  builtin unset HAETTY_ZSH_ZDOTDIR
else
  builtin unset ZDOTDIR
fi
if [[ -r ${ZDOTDIR-$HOME}/.zshenv ]]; then
  builtin source -- ${ZDOTDIR-$HOME}/.zshenv
fi
if [[ -o interactive && -n ${HAETTY_PROXY_DIR-} ]]; then
  _haetty_path_first() {
    builtin emulate -L zsh
    [[ ${PATH%%:*} != "$HAETTY_PROXY_DIR" ]] || return 0
    path=("$HAETTY_PROXY_DIR" "${(@)path:#${(b)HAETTY_PROXY_DIR}}")
  }
  builtin typeset -ga precmd_functions
  precmd_functions=("${(@)precmd_functions:#_haetty_path_first}" _haetty_path_first)
fi
