# GitViber's shell integration for bash 4.4+. It marks where each prompt starts (OSC 133;A), where
# a command's output starts (C) and how it ended (D;status), as in
# https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md
# Loaded as Ghostty loads its own: bash starts as `bash --login --posix` with ENV naming this file,
# which POSIX mode reads in place of the startup files; this ends POSIX mode and reads those.

builtin unset ENV
builtin set +o posix
builtin shopt -u inherit_errexit 2>/dev/null
# Set only so POSIX mode's ~/.sh_history wasn't taken; bash doesn't export it.
builtin export -n HISTFILE
# A login shell's files, in bash's order (INVOCATION in bash(1)).
if [ -r /etc/profile ]; then builtin source /etc/profile; fi
for __gitviber_file in ~/.bash_profile ~/.bash_login ~/.profile; do
  if [ -r "$__gitviber_file" ]; then
    builtin source "$__gitviber_file"
    break
  fi
done
builtin unset __gitviber_file
# GitViber's `gitviber` command, last on PATH (pty.rs). Again here, after the user's files: one of
# them may have set PATH afresh.
if [[ -n ${GITVIBER_BIN_DIR-} && :$PATH: != *:"$GITVIBER_BIN_DIR":* ]]; then
  PATH=$PATH:$GITVIBER_BIN_DIR
fi

if ((BASH_VERSINFO[0] > 4 || (BASH_VERSINFO[0] == 4 && BASH_VERSINFO[1] >= 4))); then
  # D follows an empty line too, but no C came before it (bash prints PS0 only for a command),
  # so the terminal doesn't take it for a command's end.
  __gitviber_prompt() {
    local ret=$?
    builtin printf '\033]133;D;%s\007\033]133;A\007' "$ret"
    return "$ret"
  }
  # First, to read the command's status, which the rest of PROMPT_COMMAND still gets.
  PROMPT_COMMAND="__gitviber_prompt${PROMPT_COMMAND:+$'\n'$PROMPT_COMMAND}"
  # Printed as a command starts.
  PS0="${PS0-}"$'\033]133;C\007'
fi
