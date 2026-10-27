//! Status items' menus: `com.canonical.dbusmenu`, as libdbusmenu, Qt and
//! Electron serve it. The host reads the whole tree with `GetLayout` when
//! the menu opens and sends `Event(id, "clicked", …)` for the row picked.

use crate::bus::{Props, PropsExt, unwrap_variant, value_dict, value_i64};
use std::collections::HashMap;
use zbus::zvariant::{OwnedValue, Value};

pub const MENU_IFACE: &str = "com.canonical.dbusmenu";

/// A check box or radio button's state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Toggle {
    #[default]
    None,
    Check(bool),
    Radio(bool),
}

/// One row of a menu, and its submenu's rows.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct MenuItem {
    pub id: i32,
    /// The label without its mnemonic underscores.
    pub label: String,
    pub separator: bool,
    pub enabled: bool,
    pub toggle: Toggle,
    /// An icon by name, from the icon theme.
    pub icon_name: Option<String>,
    /// The submenu's visible rows.
    pub children: Vec<MenuItem>,
}

impl MenuItem {
    pub fn is_submenu(&self) -> bool {
        !self.children.is_empty()
    }
}

/// A label with GTK's mnemonics taken out: `_Quit` is `Quit`, `__` a `_`.
pub fn strip_mnemonic(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '_' {
            if chars.peek() == Some(&'_') {
                chars.next();
                out.push('_');
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A row from its id, properties and children (each an `(ia{sv}av)`);
/// `None` for an invisible one.
fn item(id: i32, props: &impl PropsExt, children: &[&Value<'_>]) -> Option<MenuItem> {
    if props.opt_bool("visible") == Some(false) {
        return None;
    }
    let toggle_on = props.opt_i64("toggle-state") == Some(1);
    Some(MenuItem {
        id,
        label: strip_mnemonic(&props.str("label")),
        separator: props.str("type") == "separator",
        enabled: props.opt_bool("enabled") != Some(false),
        toggle: match props.str("toggle-type").as_str() {
            "checkmark" => Toggle::Check(toggle_on),
            "radio" => Toggle::Radio(toggle_on),
            _ => Toggle::None,
        },
        icon_name: props.opt_str("icon-name").filter(|n| !n.is_empty()),
        children: children.iter().filter_map(|c| node(c)).collect(),
    })
}

/// A row from a variant holding `(ia{sv}av)`.
fn node(value: &Value<'_>) -> Option<MenuItem> {
    let Value::Structure(s) = unwrap_variant(value) else { return None };
    let [id, props, children] = s.fields() else { return None };
    let id = i32::try_from(value_i64(id)?).ok()?;
    let props = owned_dict(props);
    let children: Vec<&Value<'_>> = match unwrap_variant(children) {
        Value::Array(a) => a.iter().collect(),
        _ => Vec::new(),
    };
    item(id, &props, &children)
}

/// An `a{sv}` of any lifetime as the properties [`PropsExt`] reads.
fn owned_dict(value: &Value<'_>) -> HashMap<String, Value<'static>> {
    let Ok(value) = value.try_to_owned() else { return HashMap::new() };
    let value: Value<'static> = value.into();
    value_dict(&value).into_iter().map(|(k, v)| (k, v.try_clone().unwrap_or(Value::Bool(false)))).collect()
}

/// The root of `GetLayout`'s reply: its id, properties and children.
pub fn from_layout(id: i32, props: &Props, children: &[OwnedValue]) -> MenuItem {
    let children: Vec<&Value<'_>> = children.iter().map(|c| &**c).collect();
    let mut root = item(id, props, &children).unwrap_or_default();
    root.children = tidy(std::mem::take(&mut root.children));
    root
}

/// Separators only between rows: none leading, trailing or doubled, at
/// every level.
fn tidy(items: Vec<MenuItem>) -> Vec<MenuItem> {
    let mut out: Vec<MenuItem> = Vec::with_capacity(items.len());
    for mut i in items {
        if i.separator && out.last().is_none_or(|l| l.separator) {
            continue;
        }
        i.children = tidy(std::mem::take(&mut i.children));
        out.push(i);
    }
    while out.last().is_some_and(|l| l.separator) {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use zbus::zvariant::{StructureBuilder, Value};

    fn row(id: i32, props: &[(&str, Value<'static>)], children: Vec<Value<'static>>) -> Value<'static> {
        let dict: HashMap<String, Value<'static>> = props.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect();
        let children: Vec<Value<'static>> = children.into_iter().map(|c| Value::Value(Box::new(c))).collect();
        let array = zbus::zvariant::Array::from(children);
        StructureBuilder::new().add_field(id).add_field(dict).append_field(Value::Array(array)).build().unwrap().into()
    }

    #[test]
    fn mnemonics_come_out() {
        assert_eq!(strip_mnemonic("_Quit"), "Quit");
        assert_eq!(strip_mnemonic("Save __as"), "Save _as");
        assert_eq!(strip_mnemonic("Plain"), "Plain");
    }

    #[test]
    fn a_layout_reads_into_rows() {
        let children = vec![
            row(0, &[("type", "separator".into())], vec![]),
            row(1, &[("label", "_Library".into())], vec![]),
            row(2, &[("label", "Hidden".into()), ("visible", false.into())], vec![]),
            row(3, &[("type", "separator".into())], vec![]),
            row(4, &[("type", "separator".into())], vec![]),
            row(
                5,
                &[("label", "Friends".into()), ("children-display", "submenu".into())],
                vec![row(
                    6,
                    &[("label", "Online".into()), ("toggle-type", "checkmark".into()), ("toggle-state", 1i32.into())],
                    vec![],
                )],
            ),
            row(7, &[("label", "Exit".into()), ("enabled", false.into())], vec![]),
            row(8, &[("type", "separator".into())], vec![]),
        ];
        let children: Vec<OwnedValue> =
            children.into_iter().map(|c| OwnedValue::try_from(Value::Value(Box::new(c))).unwrap()).collect();
        let root = from_layout(0, &Props::new(), &children);
        let labels: Vec<(&str, bool)> = root.children.iter().map(|i| (i.label.as_str(), i.separator)).collect();
        assert_eq!(labels, [("Library", false), ("", true), ("Friends", false), ("Exit", false)]);
        assert_eq!(root.children[2].children[0].toggle, Toggle::Check(true));
        assert!(root.children[2].is_submenu());
        assert!(!root.children[3].enabled);
        assert_eq!(root.children[0].id, 1);
    }
}
