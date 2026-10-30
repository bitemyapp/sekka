# Hotaru cwd/title integration, also safe to source manually after .zshrc.
# SPDX-License-Identifier: MIT
[[ -o interactive ]] || return
# A framework may replace precmd_functions in .zshrc after automatic startup.
# Sourcing this file at the end of that config restores our hook without
# replacing the framework's array or adding duplicate callbacks.
if [[ -n ${_hotaru_zsh_loaded-} ]]; then
    typeset -ga precmd_functions
    if (( ! ${precmd_functions[(Ie)_hotaru_prompt]} )); then
        precmd_functions+=(_hotaru_prompt)
    fi
    return
fi
typeset -g _hotaru_zsh_loaded=1

_hotaru_prompt() {
    local result=$?
    emulate -L zsh -o no_aliases -o no_multibyte
    local LC_ALL=C cwd=$PWD host=$HOST encoded= char hex
    local -i index
    if [[ $cwd == /* && $cwd != ${_hotaru_last_pwd-} && ${#cwd} -le 4096 && $host != *[^a-zA-Z0-9.-]* && -n $host ]]; then
        for ((index=1; index<=${#cwd}; index++)); do
            char=$cwd[index]
            case $char in
                [a-zA-Z0-9/._~-]) encoded+=$char ;;
                *) builtin printf -v hex '%%%02X' "'$char"; encoded+=$hex ;;
            esac
        done
        builtin printf '\033]7;file://%s%s\007' "$host" "$encoded" > /dev/tty
        typeset -g _hotaru_last_pwd=$cwd
    fi
    return $result
}
typeset -ga precmd_functions
precmd_functions+=(_hotaru_prompt)
