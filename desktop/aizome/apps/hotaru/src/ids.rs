//! Identities are distinct even when their underlying counters coincide.
macro_rules! identity {
    ($($name:ident),+ $(,)?) => {$ (
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u64);
        impl $name {
            pub const fn new(value: u64) -> Self { Self(value) }
            pub const fn get(self) -> u64 { self.0 }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { self.0.fmt(f) }
        }
    )+};
}
identity!(PaneId, TabId, SplitId, RequestId);
impl RequestId {
    pub fn next(self) -> Self {
        Self(self.0.checked_add(1).expect("request ID exhausted"))
    }
}

/// Tab IDs cannot be used to direct input to a pane.
/// ```compile_fail
/// use aizome_hotaru::{app::Msg, ids::TabId};
/// let message = Msg::FocusPane(TabId::new(1));
/// ```
const _: () = ();
