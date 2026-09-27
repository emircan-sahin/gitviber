# GitViber's shell integration for zsh, loaded by the .zshenv beside it. It marks where each prompt
# starts (OSC 133;A), where a command's output starts (C) and how it ended (D;status), as in
# https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md
# The prompt mark goes at the start of PS1, set as the last precmd hook and taken off again for the
# other hooks, as Ghostty's zsh integration does, so it lands on the prompt's first line.

builtin typeset -gi _gitviber_ran=0
builtin typeset -g _gitviber_mark=$'%{\e]133;A\a%}' _gitviber_ps1 _gitviber_marked

_gitviber_precmd() {
  builtin local ret=$?
  # Plugins that redraw the prompt from zle (on cd) run precmd hooks too: no command ended there.
  builtin zle && builtin return
  (( _gitviber_ran )) && builtin print -n "\e]133;D;$ret\a"
  _gitviber_ran=0
  _gitviber_unmark
  if [[ -o prompt_percent && ${precmd_functions[-1]} == _gitviber_precmd ]]; then
    # Last, so a theme that builds PS1 in its own precmd (pure) has done so.
    _gitviber_ps1=$PS1
    PS1=$_gitviber_mark$PS1
    _gitviber_marked=$PS1
  else
    # The first prompt, before the hook moves to the end for the next one (or no % in PS1).
    builtin print -n '\e]133;A\a'
    [[ -o prompt_percent ]] && precmd_functions=(${precmd_functions:#_gitviber_precmd} _gitviber_precmd)
  fi
}

# PS1 as the theme left it, unless something set a new one since.
_gitviber_unmark() {
  [[ -n $_gitviber_marked && $PS1 == "$_gitviber_marked" ]] && PS1=$_gitviber_ps1
  _gitviber_marked=
}

_gitviber_preexec() {
  _gitviber_unmark
  _gitviber_ran=1
  builtin print -n '\e]133;C\a'
}

builtin autoload -Uz add-zsh-hook
add-zsh-hook precmd _gitviber_precmd
add-zsh-hook preexec _gitviber_preexec
