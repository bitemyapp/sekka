# The shell protocol

Apps built with `aizome-kit` publish their menus to the global menu bar
over the shell's socket, and the shell tells them which item was chosen.
The compositor and `aizomectl` use the same socket to ask the shell itself
for things (the palette, the lock screen, the OSD). The types and a small
blocking client are in `crates/aizome-shell-proto`.

## The socket

`$AIZOME_SHELL_SOCKET`, set by the session (`$XDG_RUNTIME_DIR/aizome-shell.$WAYLAND_DISPLAY.sock`).
A Unix stream socket carrying newline-delimited JSON: one message per
line, each an object with a `"type"`. Unknown types and fields are
ignored, so either side may add to the protocol; fields left out take
their defaults.

When the variable isn't set (another desktop), kit apps draw their menu bar
along each window's top instead, and nothing here happens.

## An app's session

1. Connect, and send `hello` once:

   ```json
   {"type":"hello","app_id":"org.aizome.Soroban","pid":4242,"name":"Soroban"}
   ```

   The shell keys an app's menus by `pid`: the bar shows the menus of the
   process whose window has the focus.

2. Send `menus`, whole, then again whenever anything in them changes. The
   kit debounces (40 ms) and sends only when the menus' JSON changed.

   ```json
   {"type":"menus","menus":[
     {"id":1073741912,"title":"Edit","items":[
       {"id":16384,"kind":"action","label":"Undo","shortcut":{"key":"Z","command":true},"enabled":true,"role":"undo"},
       {"id":0,"kind":"separator","enabled":false},
       {"id":1073745001,"kind":"submenu","label":"Find","enabled":true,"items":[ … ]}
     ]}
   ]}
   ```

3. Read `activate`, `about-to-show` and `services-text` from the shell
   until it hangs up.

## Menus and items

A menu: `id` (number), `title`, `items`. The first menu is the app menu
(its title is the app's name).

An item:

| Field | Type | Default | Meaning |
|---|---|---|---|
| `id` | number | — | Stable while the item is: the app's own if it gave one, else derived from its role (`copy` is always the same id), else from its menu's title, label and place. Submenus' ids have bit 30 set. Separators and headers are 0. |
| `kind` | `action`, `separator`, `submenu`, `header` | `action` | A header is a section's name: text that does nothing. |
| `label` | string | `""` | |
| `shortcut` | object | none | `key` (a character such as `"N"` or `","`, or a name: `Enter`, `Tab`, `Space`, `Escape`, `Backspace`, `Delete`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`, `PageUp`, `PageDown`, `F1`…), and `command` (❖, Super), `shift`, `alt`, `ctrl`, each `false` if left out. Printed ⌃ ⌥ ⇧ ❖ then the key. |
| `enabled` | bool | `true` | The kit settles the Edit items' states from the focused widget (Copy is enabled while there's a selection). |
| `checked` | bool | `false` | |
| `role` | string | none | What the item is to the system: `about`, `settings`, `services`, `hide`, `hide-others`, `show-all`, `quit`, `undo`, `redo`, `cut`, `copy`, `paste`, `paste-as-plain-text`, `delete`, `select-all`, `find`, `find-next`, `find-previous`, `use-selection-for-find`, `replace`, `minimise`, `fill`, `stack-with`, `bring-all-to-front`, `close-window`, `window-list`, `help-search` (the app asks the shell for its palette on its commands), `help`, `tour` (the app starts `aizome-annai`). The shell may do some itself (Hide, Minimise) and the palette shows them. |
| `icon` | string | none | A Lucide icon's name (`wifi`, `folder-open`). |
| `items` | array | `[]` | A submenu's items. |

The Window menu's list of windows comes as items with the role
`window-list`, one per window, checked for the focused one.

## What the shell sends

| Message | Meaning | The app answers |
|---|---|---|
| `{"type":"activate","id":N}` | Item `N` was chosen (from the bar, a menu at the pointer, a torn-off palette or the palette). | It carries the item out as if chosen from its own menus: the Edit roles go to the focused widget, the others to the program. |
| `{"type":"about-to-show","menu":N}` | Menu `N` (a menu's or a submenu's id) is about to show. | It asks the program for its menus again and sends them at once if they changed. |
| `{"type":"services-text","serial":S}` | The selected text, for a service. | `{"type":"services-text","serial":S,"text":"…"}` with the focused widget's selection (no `text` if there's none). |

## Requests to the shell

The compositor (for its key bindings) and `aizomectl` send
`{"type":"request","request":…}` without a `hello`, and get one
`{"type":"reply","ok":true|false,"message":"…"}`:

| Request | Fields | |
|---|---|---|
| `palette` | `scope` (`commands`, `windows`, `files`, `apps`, `settings`), `query` | Open the command palette (or close it). |
| `toolchest` | | Open the Toolchest. |
| `app-menu` | `x`, `y` | The focused app's whole menu at a point (NeXT's menu at the pointer). |
| `lock` | | Lock the screen. |
| `osd` | `kind` (`volume`, `mute`, `brightness`, `keyboard`), `value` 0–1 | Show the on-screen display. |
| `notify` | `app`, `summary`, `body` | Post a notification. |
| `deskbar` | `collapsed` | Fold or unfold the Deskbar (toggle if left out). |
| `open` | `what` | Open a systray panel (`network`, `sound`, `bluetooth`, `display`, `battery`, `clock`) under its widget in the main display's bar (under the clock where the bar leaves the widget out), or a bar menu by its title. Asked again, the panel closes. |
| `dismiss` | | Close whatever transient surface is open. |
| `state` | | What the shell shows, as JSON in the reply's `message`. |
| `menu-bar` | `output` | The menu bar takes the keyboard (F10, ❖⌥M): the focused app's menu opens with its first row lit; ← → go across the titles. |
| `bell` | `output` | The system bell rang: the screen's edge flashes if `[sound] visual_alert` is on. |

## The client

```rust
use aizome_shell_proto::{Client, AppMessage, ShellMessage};

let mut c = Client::hello(&aizome_shell_proto::socket_path().unwrap(), "org.example.App", "App")?;
c.send(&AppMessage::Menus { menus })?;
let mut sender = c.sender()?; // to send from another thread
while let Some(msg) = c.recv()? {
    match msg {
        ShellMessage::Activate { id } => { /* … */ }
        _ => {}
    }
}
```

`aizome_shell_proto::request(path, ShellRequest::Palette { … })` sends one
request and waits for its reply.
