# GitViber's shell integration for bash, started as `bash --init-file <this file>`, as VS Code does.
# It marks where each prompt starts (OSC 133;A), where a command's output starts (C), how it
# ended (D;status) and the folder (OSC 7), as in
# https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/semantic-prompts.md

# --init-file can't be a login shell's, so the login files are read here, in bash's order (as in
# VS Code's shellIntegration-bash.sh).
if [ -r /etc/profile ]; then . /etc/profile; fi
if [ -r ~/.bash_profile ]; then . ~/.bash_profile
elif [ -r ~/.bash_login ]; then . ~/.bash_login
elif [ -r ~/.profile ]; then . ~/.profile
fi

__gitviber_prompt() {
  local ret=$?
  # The folder too, for splits and restores: OSC 7 in kitty's form, which takes the path as it is.
  builtin printf '\033]133;D;%s\007\033]7;kitty-shell-cwd://%s%s\007\033]133;A\007' "$ret" "$HOSTNAME" "$PWD"
  return "$ret"
}
# First, to read the command's status, which the rest of PROMPT_COMMAND still gets.
PROMPT_COMMAND="__gitviber_prompt${PROMPT_COMMAND:+$'\n'$PROMPT_COMMAND}"
# Printed as a command starts, in bash 4.4 and later; older ones get the prompt marks only.
PS0="${PS0-}"$'\033]133;C\007'
