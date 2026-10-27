# Plymouth: keyboards freed with their renderer

| | |
| --- | --- |
| Sekka's copy | [`keyboard-removal.patch`](../../../installer/genkan/rust/system/boot-splash/plymouth/keyboard-removal.patch), for 26.134.222 |
| Upstream version | [keyboard-removal.patch](keyboard-removal.patch): 049738c7 and 311dbd36, for `main` at 074cc389 |
| Status | In review since 2026-10-25: [plymouth!519](https://gitlab.freedesktop.org/plymouth/plymouth/-/merge_requests/519) ([Submitting](README.md#submitting)) |
| Sent with | [console-owner](console-owner.md) |

## What goes wrong

Plymouth's device manager gives each renderer a keyboard. When a renderer goes
away, as simpledrm's does when the GPU's DRM driver takes over,
`free_keyboards_for_renderer()` frees its keyboards without calling the
manager's `keyboard_removed_handler`. plymouthd never detaches its own handlers
from them, and the splash plugin keeps the keyboard it was given, which is now
freed memory.

The script plugin has a second copy of it. It hands the keyboard to the theme's
`Plymouth` library once, when the script starts, and never updates it. So
`Plymouth.GetCapslockState()`, and on `main` `Plymouth.GetKeyboardLayout()`, go
on reading the freed keyboard, and the plugin's keystroke handler stays on it.

## What it did in Sekka

Sekka's theme calls `Plymouth.GetCapslockState()` on every frame while a prompt
is up, for its "Caps Lock is on" warning. In `encrypted-boot-splash`, plymouthd
crashed with SIGSEGV in 3 of 11 test boots that went through a wrong passphrase
and Esc. A debug build of 26.134.222 showed where:

```
#0  ply_keyboard_get_capslock_state (keyboard=0x555d5fdb14c0) at ../src/libply-splash-core/ply-keyboard.c:702
#1  plymouth_get_capslock_state (state=<optimized out>, user_data=<optimized out>) at ../src/plugins/splash/script/script-lib-plymouth.c:106
#2  script_execute_function_with_parlist (...) at ../src/plugins/splash/script/script-execute.c:624
```

## The fix

Two commits, so that the core change and the script plugin's can be reviewed
separately:

1. **device-manager: Report keyboards freed with their renderer** (049738c7)
   - `free_keyboards_for_renderer()` calls `keyboard_removed_handler` for each
     of the renderer's keyboards, then takes it off the list and frees it.
   - plymouthd's `keyboard_removed()` (`src/daemon/plymouthd-input.c`) unsets
     the splash's keyboard and gives it one of the keyboards that are left, if
     there are any.
   - `ply_boot_splash_unset_keyboard()` forgets the keyboard it unset, so a
     later call can't hand the plugin a freed one.
   - `ply_device_manager_free()` clears the handler first. On `main`,
     `plymouthd_free()` frees the splash before the devices, so reporting the
     keyboards freed along with the manager would reach a splash that's gone.
2. **script: Follow the splash's keyboard** (311dbd36)
   - The script plugin's `set_keyboard` and `unset_keyboard` move its keystroke
     handler while the splash is showing, and pass the keyboard to the library
     through a new `script_lib_plymouth_set_keyboard()`.
   - With no keyboard, `Plymouth.GetCapslockState()` answers off, as
     `Plymouth.GetKeyboardLayout()` already answers with no layout.
   - `tests/test-script-lib-plymouth.c` adds
     `test_keyboard_queries_follow_the_splash_keyboard`. With the keyboard taken
     away, the script's queries answer off and no layout; given a keyboard
     again, they read that one.

## Sekka's copy

[`installer/genkan/rust/system/boot-splash/plymouth/keyboard-removal.patch`](../../../installer/genkan/rust/system/boot-splash/plymouth/keyboard-removal.patch)
makes the same change to 26.134.222 as one patch. It differs from the upstream
version in four ways:

- plymouthd's part is in `on_keyboard_removed()` in `src/main.c`, and walks
  `ply_device_manager_get_keyboards()` itself; `main` has
  `plymouthd_devices_for_each_keyboard()` for that.
- It doesn't clear the handler in `ply_device_manager_free()`. In 26.134.222,
  `quit_program()` frees the device manager while the splash is still there.
- It has no test: `tests/test-script-lib-plymouth.c` arrived on `main` after
  26.134.222, with `Plymouth.GetKeyboardLayout()` (3da66b90).
- `plymouth_get_capslock_state()` keeps its braces.

## Testing

Both branches pass Plymouth's tests, its sanitizer build and its format check
as `main` does, and `encrypted-boot-splash` passes with Plymouth's `main` and
both branches ([Testing](README.md#testing)). The new test,
`test_keyboard_queries_follow_the_splash_keyboard`, passes on this branch.
Across 14 boots of the check, 12 with Sekka's copies on 26.134.222 and 2 with
`main` and both branches, plymouthd didn't crash.

## When Sekka's copy goes

Remove `keyboard-removal.patch` from `boot-splash/default.nix` once Nixpkgs'
Plymouth is a release with these commits or another fix for the freed keyboard,
and run `encrypted-boot-splash`. Its step "after the GPU takes over, the splash
still hears the keyboard" asks for a passphrase on the GPU's device and turns
Caps Lock on, so the theme reads the splash's keyboard after the handover.
