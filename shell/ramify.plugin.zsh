# ramify — oh-my-zsh plugin. `ramify_prompt_info` prints the ramify line of the current directory:
# its worktree, branch, stack, workflow step, and the Claude Code and Codex sessions working in it.
# Any theme can call it. powerlevel10k gets the `ramify` segment from ramify.p10k.zsh.
#
# `ramify` must be on the PATH, or RAMIFY_BIN must name it.

typeset -g _RAMIFY_PROMPT_LINE=''

# Runs once per prompt, so `ramify_prompt_info` costs nothing however often a theme calls it.
_ramify_precmd() {
  local bin="${RAMIFY_BIN:-ramify}"
  if (( $+commands[$bin] )) || [[ -x $bin ]]; then
    _RAMIFY_PROMPT_LINE="$("$bin" prompt 2>/dev/null)"
  else
    _RAMIFY_PROMPT_LINE=''
  fi
}

ramify_prompt_info() {
  [[ -n $_RAMIFY_PROMPT_LINE ]] && print -r -- "$_RAMIFY_PROMPT_LINE"
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd _ramify_precmd

source "${${(%):-%x}:A:h}/ramify.p10k.zsh"
