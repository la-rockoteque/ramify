# ramify — oh-my-zsh plugin. `ramify_prompt_info` prints the ramify line of the current directory:
# its worktree, branch, stack, workflow step, and the Claude Code and Codex sessions working in it.
# Any theme can call it. powerlevel10k gets the `ramify` segment from ramify.p10k.zsh.
#
# `ramify` must be on the PATH, or RAMIFY_BIN must name it.

typeset -g _RAMIFY_PROMPT_LINE=''

# Runs once per prompt, so `ramify_prompt_info` costs nothing however often a theme calls it.
_ramify_precmd() {
  _RAMIFY_PROMPT_LINE=''
  local bin="${RAMIFY_BIN:-ramify}"
  bin="${${bin:c}:A}"
  [[ -x $bin ]] || return
  # bin/ramify builds the Rust binary when it is missing: that takes minutes, so a prompt does
  # not start it.
  local home="${bin:h:h}"
  if [[ -d $home/cli && ! -x $home/cli/target/release/ramify && ! -x $home/cli/target/debug/ramify ]]; then
    return
  fi
  _RAMIFY_PROMPT_LINE="$("$bin" prompt 2>/dev/null)"
}

# `%` is a prompt escape, and a branch name may hold one.
ramify_prompt_info() {
  [[ -n $_RAMIFY_PROMPT_LINE ]] && print -r -- "${_RAMIFY_PROMPT_LINE//\%/%%}"
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd _ramify_precmd

source "${${(%):-%x}:A:h}/ramify.p10k.zsh"
