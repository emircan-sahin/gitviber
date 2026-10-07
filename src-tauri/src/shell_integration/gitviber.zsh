# GitViber's shell integration for zsh, loaded by the .zshenv beside it. It marks where each prompt
# starts (OSC 133;A), where a command's output starts (C, with the command line) and how it ended
# (D;status), as in
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
    # The first prompt, before the hook moves to the end for the next one (or no % in PS1): marked
    # once zle reads the line, so a command waiting for it (a worktree's Run) isn't echoed above it.
    if (( ${+functions[add-zle-hook-widget]} )); then
      add-zle-hook-widget line-init _gitviber_first
    else
      builtin print -n '\e]133;A\a'
    fi
    [[ -o prompt_percent ]] && precmd_functions=(${precmd_functions:#_gitviber_precmd} _gitviber_precmd)
  fi
}

_gitviber_first() {
  builtin print -n '\e]133;A\a'
  add-zle-hook-widget -d line-init _gitviber_first
}

# PS1 as the theme left it, unless something set a new one since.
_gitviber_unmark() {
  [[ -n $_gitviber_marked && $PS1 == "$_gitviber_marked" ]] && PS1=$_gitviber_ps1
  _gitviber_marked=
}

_gitviber_preexec() {
  # zsh's own options here (ksh_arrays would count from 0), put back on return.
  builtin emulate -L zsh
  _gitviber_unmark
  _gitviber_ran=1
  # A line kept out of the history (hist_ignore_space) isn't named either.
  if [[ $1 == ' '* ]]; then
    builtin print -n '\e]133;C\a'
    builtin return
  fi
  # The line as typed, for "pnpm test finished after 2m": cut short, controls made spaces, then
  # percent-encoded a byte at a time as fish's cmdline_url, so no byte of it can end the mark
  # (under LC_ALL=C a pasted C1 control isn't [[:cntrl:]]). Arithmetic, so nothing forks.
  # Cut and cleaned by characters, then encoded by bytes: bytewise, [[:cntrl:]] takes the 0x80-0x9F
  # of a UTF-8 sequence for controls and splits its characters.
  builtin local line=${${1:0:200}//[[:cntrl:]]/ } url= c
  builtin setopt no_multibyte
  for c in ${(s::)line}; do
    if [[ $c == [A-Za-z0-9._~/-] ]]; then
      url+=$c
    else
      url+=%${(l:2::0:)$(( [##16] #c ))}
    fi
  done
  builtin print -rn -- $'\e]133;C;cmdline_url='$url$'\a'
}

# GitViber's `gitviber` command, last on PATH (pty.rs). Again at the first prompt, after .zshrc and
# .zlogin: one of them may have set PATH afresh.
_gitviber_path() {
  builtin emulate -L zsh
  add-zsh-hook -d precmd _gitviber_path
  [[ -z $GITVIBER_BIN_DIR || :$PATH: == *:"$GITVIBER_BIN_DIR":* ]] || PATH+=:$GITVIBER_BIN_DIR
}

builtin autoload -Uz add-zsh-hook add-zle-hook-widget
add-zsh-hook precmd _gitviber_precmd
add-zsh-hook preexec _gitviber_preexec
add-zsh-hook precmd _gitviber_path
