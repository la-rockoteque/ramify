# ramify — the `ramify` segment for powerlevel10k. Add `ramify` to
# POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS (or the left ones) in ~/.p10k.zsh. The oh-my-zsh plugin
# sources this file.

prompt_ramify() {
  local line="$(ramify_prompt_info)"
  [[ -n $line ]] || return
  p10k segment -t "$line"
}
