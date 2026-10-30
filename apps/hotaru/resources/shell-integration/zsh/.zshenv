# Hotaru default-shell bootstrap. SPDX-License-Identifier: MIT
# Restore the original lookup directory before user startup files run. This
# adds no user files and does not alter the normal profile/rc/login sequence.
if [[ ${HOTARU_ZSH_ZDOTDIR_SET-0} == 1 ]]; then
    builtin export ZDOTDIR=$HOTARU_ZSH_ZDOTDIR
else
    builtin unset ZDOTDIR
fi
builtin unset HOTARU_ZSH_ZDOTDIR HOTARU_ZSH_ZDOTDIR_SET
if [[ -r ${ZDOTDIR-$HOME}/.zshenv ]]; then
    builtin source -- "${ZDOTDIR-$HOME}/.zshenv"
fi
if [[ -o interactive && -r $HOTARU_RESOURCES_DIR/shell-integration/zsh/hotaru.zsh ]]; then
    builtin source -- "$HOTARU_RESOURCES_DIR/shell-integration/zsh/hotaru.zsh"
fi
