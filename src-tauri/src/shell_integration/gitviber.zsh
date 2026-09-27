# GitViber's shell integration for zsh, loaded by the .zshenv beside it. It marks where each prompt
# starts (OSC 133;A), where a command's output starts (C), how it ended (D;status) and the folder
# (OSC 7), as in
# https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md
# From precmd and preexec hooks, like VS Code's and Ghostty's; PS1 is left as the theme set it.

_gitviber_precmd() {
  builtin local ret=$?
  # Plugins that redraw the prompt from zle (on cd) run precmd hooks too: no command ended there.
  builtin zle && builtin return
  (( _gitviber_ran )) && builtin print -n "\e]133;D;$ret\a"
  _gitviber_ran=0
  # The folder, for splits and restores: OSC 7 in kitty's form, which takes the path as it is.
  builtin print -rn -- $'\e]7;kitty-shell-cwd://'"$HOST$PWD"$'\a'
  builtin print -n '\e]133;A\a'
}

_gitviber_preexec() {
  _gitviber_ran=1
  builtin print -n '\e]133;C\a'
}

builtin typeset -gi _gitviber_ran=0
builtin autoload -Uz add-zsh-hook
add-zsh-hook precmd _gitviber_precmd
add-zsh-hook preexec _gitviber_preexec
