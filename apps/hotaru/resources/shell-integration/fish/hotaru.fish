# Hotaru cwd/title integration; loaded after normal Fish configuration.
# SPDX-License-Identifier: MIT
status is-interactive; or return
set -q _hotaru_fish_loaded; and return
set -g _hotaru_fish_loaded 1

function _hotaru_prompt --on-event fish_prompt --on-variable PWD
    set -l result $status
    if status is-command-substitution; or test "$PWD" = "$_hotaru_last_pwd"
        return $result
    end
    if test (string length -- "$PWD") -le 4096; and string match -rq '^/[\s\S]*$' -- "$PWD"; and string match -rq '^[a-zA-Z0-9.-]+$' -- "$hostname"
        set -l encoded (string escape --style=url -- "$PWD")
        printf '\e]7;file://%s%s\a' "$hostname" "$encoded" > /dev/tty
        set -g _hotaru_last_pwd "$PWD"
    end
    return $result
end
