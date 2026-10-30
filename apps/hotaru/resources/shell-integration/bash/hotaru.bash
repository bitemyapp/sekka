# Hotaru cwd/title integration, also safe to source manually after .bashrc.
# SPDX-License-Identifier: MIT
[[ $- == *i* && -z ${_hotaru_bash_loaded-} ]] || return
_hotaru_bash_loaded=1

_hotaru_prompt() {
    builtin local "result=$?" "path=${PWD-}" "host=${HOSTNAME-}"
    builtin local LC_ALL=C encoded= char hex index
    if [[ $path == /* && $path != "${_hotaru_last_pwd-}" && ${#path} -le 4096 && $host != *[!a-zA-Z0-9.-]* && -n $host ]]; then
        # Percent-encode bytes, including '%' and control bytes. URI text can
        # never terminate OSC or inject another terminal control sequence.
        for ((index=0; index<${#path}; index++)); do
            char=${path:index:1}
            case $char in
                [a-zA-Z0-9/._~-]) encoded+=$char ;;
                *) builtin printf -v hex '%%%02X' "'$char"; encoded+=$hex ;;
            esac
        done
        builtin printf '\033]7;file://%s%s\007' "$host" "$encoded" > /dev/tty
        _hotaru_last_pwd=$path
    fi
    return "$result"
}

# Run first and return the incoming status, so existing prompt commands see the
# same $? and retain their original order. Do not change PS1/PS0 or DEBUG traps.
_hotaru_prompt_declaration=$(builtin declare -p PROMPT_COMMAND 2>/dev/null)
if [[ $_hotaru_prompt_declaration =~ ^declare\ -[^[:space:]]*[rA] ]]; then
    : # A readonly or associative prompt setting is left alone.
elif [[ $_hotaru_prompt_declaration =~ ^declare\ -[^[:space:]]*a ]]; then
    PROMPT_COMMAND=('_hotaru_prompt' "${PROMPT_COMMAND[@]}")
else
    PROMPT_COMMAND="_hotaru_prompt${PROMPT_COMMAND:+; $PROMPT_COMMAND}"
fi
builtin unset _hotaru_prompt_declaration
