use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::time::Duration;

use aizome_hotaru::app::{Hotaru, Msg};
use aizome_hotaru::ids::{PaneId, TabId};
use aizome_hotaru::panes::Axis;
use aizome_kit::testing::Harness;
use aizome_shell_proto as proto;

fn read_menus(reader: &mut BufReader<UnixStream>) -> Vec<proto::Menu> {
    loop {
        let mut line = String::new();
        assert_ne!(reader.read_line(&mut line).unwrap(), 0, "the app disconnected");
        if let proto::AppMessage::Menus { menus } = proto::decode(&line).unwrap() {
            return menus;
        }
    }
}

fn choose(h: &mut Harness<Hotaru>, writer: &mut UnixStream, id: u32) {
    writer.write_all(proto::encode(&proto::ShellMessage::Activate { id }).as_bytes()).unwrap();
    assert_eq!(h.pump_shell(Duration::from_secs(5)), 1);
    h.advance(Duration::ZERO);
}

fn tab_menu_survives_change(rename: bool) {
    let dir = std::env::temp_dir().join(format!("hotaru-tab-menu-{}-{rename}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("shell.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let mut h = Harness::with_shell(Hotaru::preview(), &socket);
    let window = h.window();
    h.focus_window(window, true);
    let (stream, _) = listener.accept().unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    let first = h.program().active;
    h.send(Msg::Title(pane_for(&h, first), "Same title".into()));
    h.send(Msg::NewTab);
    let second = h.program().active;
    h.send(Msg::Split(Axis::Right));
    let pane = h.program().active_pane().unwrap().id;
    assert_ne!(second.get(), pane.get(), "a tab's identity differs from its focused split");
    h.send(Msg::Title(pane, "Same title".into()));
    h.send(Msg::NewTab);
    let third = h.program().active;
    h.send(Msg::Title(pane_for(&h, third), "Same title".into()));
    h.advance(Duration::from_millis(50));
    let menus = read_menus(&mut reader);
    let tabs: Vec<_> = menus
        .iter()
        .find(|m| m.title == "Window")
        .unwrap()
        .items
        .iter()
        .filter(|item| item.label == "Same title")
        .collect();
    assert_eq!(tabs.len(), 3);
    let ids: Vec<_> = tabs.iter().map(|item| item.id).collect();
    assert_ne!(ids[0], ids[1]);
    assert_ne!(ids[1], ids[2]);
    assert_eq!(tabs.iter().map(|item| item.checked).collect::<Vec<_>>(), [false, false, true]);

    // Use the id actually published to the shell, rather than sending a
    // SelectTab message directly. Duplicate titles still select the right tab.
    choose(&mut h, &mut writer, ids[1]);
    assert_eq!(h.program().active, second);
    assert_eq!(h.program().active_pane().unwrap().id, pane);
    assert_eq!(h.focused_name(window), Some(format!("terminal-{pane}")));
    choose(&mut h, &mut writer, ids[2]);
    assert_eq!(h.program().active, third);

    // The shell's open menu retains its published ids while terminal output
    // changes a title, or another tab closes and shifts the remaining rows.
    if rename {
        h.send(Msg::Title(pane, "New title from OSC 2".into()));
    } else {
        h.send(Msg::SelectTab(first));
        h.send(Msg::CloseTab);
        h.send(Msg::SelectTab(third));
    }
    choose(&mut h, &mut writer, ids[1]);
    assert_eq!(h.program().active, second, "the displayed menu item must still select its original tab");
    assert_eq!(h.program().active_pane().unwrap().id, pane);
    assert_eq!(h.focused_name(window), Some(format!("terminal-{pane}")));

    h.advance(Duration::from_millis(50));
    let menus = read_menus(&mut reader);
    let selected =
        menus.iter().find(|m| m.title == "Window").unwrap().items.iter().find(|item| item.id == ids[1]).unwrap();
    assert!(selected.checked);
    assert_eq!(selected.label, if rename { "New title from OSC 2" } else { "Same title" });

    // A stale id for a closed tab must never target the next row with the
    // same title (or a new tab subsequently added to that row).
    h.send(Msg::CloseTab);
    h.send(Msg::NewTab);
    let replacement = h.program().active;
    h.send(Msg::Title(pane_for(&h, replacement), "Same title".into()));
    choose(&mut h, &mut writer, ids[2]);
    assert_eq!(h.program().active, third);
    choose(&mut h, &mut writer, ids[1]);
    assert_eq!(h.program().active, third, "a closed tab's menu action must be ignored");

    drop(h);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn window_menu_selects_the_same_tab_after_its_title_changes() {
    tab_menu_survives_change(true);
}

#[test]
fn window_menu_selects_the_same_tab_after_a_preceding_tab_closes() {
    tab_menu_survives_change(false);
}

fn pane_for(h: &Harness<Hotaru>, tab: TabId) -> PaneId {
    h.program().tabs.iter().find(|candidate| candidate.id == tab).unwrap().active
}
