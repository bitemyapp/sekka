# Hotaru default, non-login Bash bootstrap. SPDX-License-Identifier: MIT
# Bash loads its compiled system bashrc before this --rcfile. Preserve the
# user's normal rc, its options and prompt; explicit Bash commands bypass this.
[[ $- == *i* ]] || return
if [[ -r $HOME/.bashrc ]]; then
    builtin source "$HOME/.bashrc"
fi
if [[ -r $HOTARU_RESOURCES_DIR/shell-integration/bash/hotaru.bash ]]; then
    builtin source "$HOTARU_RESOURCES_DIR/shell-integration/bash/hotaru.bash"
fi
