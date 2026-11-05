//! The borrowed-context machine: [`machine!`](crate::machine).

/// Declares a typestate machine whose states borrow one shared context.
///
/// ```
/// aizome_typestate::machine! {
///     /// A pending line wrap over a cursor.
///     pub struct Wrap over Cursor {
///         /// Where the machine is: its state and that state's data.
///         #[derive(Debug, Clone, Copy, PartialEq, Eq)]
///         stored(Copy) WrapState;
///         /// The machine's typed state, holding the cursor.
///         typed WrapAny;
///         states {
///             Normal,
///             Pending,
///         }
///         branches {
///             Ready = Normal | Pending;
///         }
///         with_typed(pub);
///     }
/// }
///
/// pub struct Cursor { col: u16, cols: u16 }
///
/// impl<'s> Normal<'s> {
///     /// Writes a cell; reaching the last column makes the wrap pending.
///     pub fn print(self) -> Ready<'s> {
///         self.ctx.col += 1;
///         if self.ctx.col + 1 == self.ctx.cols {
///             Ready::Pending(Pending::enter(self.ctx, ()))
///         } else {
///             Ready::Normal(self)
///         }
///     }
/// }
///
/// impl<'s> Pending<'s> {
///     /// Wraps to the next line, then writes the cell.
///     pub fn print(self) -> Ready<'s> {
///         self.ctx.col = 0;
///         Normal::enter(self.ctx, ()).print()
///     }
/// }
///
/// let mut wrap = Wrap::new(Cursor { col: 0, cols: 3 });
/// for _ in 0..2 {
///     wrap.with_typed(|state| {
///         let next = match state {
///             WrapAny::Normal(s) => s.print().into(),
///             WrapAny::Pending(s) => s.print().into(),
///         };
///         (next, ())
///     });
/// }
/// assert_eq!(*wrap.stored(), WrapState::Pending(()));
/// ```
///
/// It declares, in the module that invokes it:
///
/// - **One struct per state**, `State<'s> { ctx: &'s mut Context, data: Data }`.
///   A state without data has `data: ()`. Its fields are private to the
///   module, so only the module's transitions make states:
///   `State::enter(ctx, data)` (private). Write the transitions as methods
///   that take `self` and return the next state or a branch.
/// - **The stored state** (`stored`): an enum with one variant per state
///   carrying that state's data. A machine whose states carry no data stores
///   one byte. Write `stored(Copy) Name;` when it derives `Copy`: `with_typed`
///   then copies it instead of storing a placeholder while the typed state
///   runs, one store fewer per event.
/// - **The typed state** (`typed`): an enum with one variant per state
///   struct. Each state converts into it with `From`. `open(stored, ctx)` and
///   `close(self)` (both private) move between the stored and typed states.
/// - **The machine**, `struct Name { ctx, stored }`, starting in the first
///   state listed, which must carry no data. `new(ctx)` (with the machine's
///   visibility, or the one `new(pub(crate));` or `new(private);` gives),
///   `ctx()` and `stored()` (read-only), and `with_typed(f)`: `f` gets the current typed
///   state and must hand back the state it ends in. Its `for<'s>` ties both
///   to this machine's context, so a state can't move between machines.
///   `with_typed` is private unless `with_typed(pub)` or another visibility
///   is given.
/// - **Branches**, if declared: `Ready = Normal | Pending;` declares
///   `enum Ready<'s> { Normal(Normal<'s>), Pending(Pending<'s>) }` for a
///   transition with more than one outcome, with `From<Ready>` for the typed
///   state.
///
/// Everything it declares is `#[inline]` or generic, so a machine compiles
/// only what it uses.
///
/// Code outside the module can't make a state, open a stored state or set
/// the machine's state:
///
/// ```compile_fail,E0451
/// mod m {
///     aizome_typestate::machine! {
///         pub struct M over u8 { stored S; typed A; states { Idle, Armed } with_typed(pub); }
///     }
/// }
/// fn forge<'s>(ctx: &'s mut u8) -> m::Armed<'s> {
///     m::Armed { ctx, data: () }
/// }
/// ```
///
/// ```compile_fail,E0616
/// mod m {
///     aizome_typestate::machine! {
///         pub struct M over u8 { stored S; typed A; states { Idle, Armed } }
///     }
/// }
/// let mut machine = m::M::new(0);
/// machine.stored = m::S::Armed(());
/// ```
///
/// and a state of one machine can't be handed back to another:
///
/// ```compile_fail
/// mod m {
///     aizome_typestate::machine! {
///         pub struct M over u8 { stored S; typed A; states { Idle, Armed } with_typed(pub); }
///     }
/// }
/// let mut a = m::M::new(0);
/// let mut b = m::M::new(0);
/// a.with_typed::<()>(|sa| {
///     b.with_typed::<()>(|_sb| (sa, ()));
///     unreachable!()
/// });
/// ```
#[macro_export]
macro_rules! machine {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident over $ctx:ty {
            $(#[$smeta:meta])*
            stored $(($copy:ident))? $stored:ident;
            $(#[$ameta:meta])*
            typed $any:ident;
            states { $($states:tt)* }
            $(branches { $($branches:tt)* })?
            $(new($($nvis:tt)*);)?
            $(with_typed($($wvis:tt)*);)?
        }
    ) => {
        $crate::machine!(@norm
            [
                [$(#[$meta])*] [$vis] $name [$ctx]
                [$(#[$smeta])*] [$($copy)?] $stored [$(#[$ameta])*] $any
                [$($($branches)*)?] [$($($wvis)*)?]
                [$($($nvis)*)?] [$vis]
            ]
            []
            $($states)*
        );
    };

    // Normalizes the states to `[attributes name data-type]`, `()` for none.
    (@norm $hdr:tt [$($done:tt)*] $(#[$m:meta])* $s:ident ($d:ty) $(, $($rest:tt)*)?) => {
        $crate::machine!(@norm $hdr [$($done)* [[$(#[$m])*] $s [$d]]] $($($rest)*)?);
    };
    (@norm $hdr:tt [$($done:tt)*] $(#[$m:meta])* $s:ident $(, $($rest:tt)*)?) => {
        $crate::machine!(@norm $hdr [$($done)* [[$(#[$m])*] $s [()]]] $($($rest)*)?);
    };
    (@norm $hdr:tt [$($done:tt)*]) => {
        $crate::machine!(@emit $hdr $($done)*);
    };

    (@emit
        [
            [$(#[$meta:meta])*] [$vis:vis] $name:ident [$ctx:ty]
            [$(#[$smeta:meta])*] [$($copy:ident)?] $stored:ident [$(#[$ameta:meta])*] $any:ident
            [$($bname:ident = $($bstate:ident)|+ ;)*] [$($wvis:tt)*]
            [$($nvis:tt)*] [$dvis:vis]
        ]
        [[$(#[$fm:meta])*] $first:ident [$fd:ty]]
        $([[$(#[$m:meta])*] $s:ident [$d:ty]])*
    ) => {
        $crate::machine!(@state [$(#[$fm])*] [$vis] $first [$fd] [$ctx] $any);
        $( $crate::machine!(@state [$(#[$m])*] [$vis] $s [$d] [$ctx] $any); )*

        $(#[$smeta])*
        $vis enum $stored {
            $first($fd),
            $( $s($d), )*
        }

        $(#[$ameta])*
        $vis enum $any<'s> {
            $first($first<'s>),
            $( $s($s<'s>), )*
        }

        impl<'s> $any<'s> {
            /// The typed state for a stored one, holding the context.
            #[inline]
            #[allow(dead_code)]
            fn open(stored: $stored, ctx: &'s mut $ctx) -> Self {
                match stored {
                    $stored::$first(data) => $any::$first($first { ctx, data }),
                    $( $stored::$s(data) => $any::$s($s { ctx, data }), )*
                }
            }

            /// The stored state, releasing the context.
            #[inline]
            #[allow(dead_code)]
            fn close(self) -> $stored {
                match self {
                    $any::$first(state) => $stored::$first(state.data),
                    $( $any::$s(state) => $stored::$s(state.data), )*
                }
            }
        }

        $(#[$meta])*
        $vis struct $name {
            ctx: $ctx,
            stored: $stored,
        }

        impl $name {
            $crate::machine!(@new [$($nvis)*] [$dvis] $name $ctx, $stored::$first(()));

            /// The shared context, read-only.
            #[inline]
            #[allow(dead_code)]
            $vis fn ctx(&self) -> &$ctx {
                &self.ctx
            }

            /// The current state, read-only.
            #[inline]
            #[allow(dead_code)]
            $vis fn stored(&self) -> &$stored {
                &self.stored
            }

            /// Runs typed transitions on the current state. `f` must hand back
            /// the state it ends in: one reached from the state it was given,
            /// since nothing else can make a state of this machine.
            #[inline]
            #[allow(dead_code)]
            $($wvis)* fn with_typed<R>(&mut self, f: impl for<'s> FnOnce($any<'s>) -> ($any<'s>, R)) -> R {
                let stored = $crate::machine!(@take [$($copy)?] self.stored, $stored::$first(()));
                let (next, r) = f($any::open(stored, &mut self.ctx));
                self.stored = next.close();
                r
            }
        }

        $(
            #[allow(dead_code)]
            $vis enum $bname<'s> {
                $( $bstate($bstate<'s>), )+
            }

            impl<'s> ::core::convert::From<$bname<'s>> for $any<'s> {
                #[inline]
                fn from(branch: $bname<'s>) -> Self {
                    match branch {
                        $( $bname::$bstate(state) => $any::$bstate(state), )+
                    }
                }
            }
        )*
    };

    // `new`, with the visibility `new(...)` gave, or the machine's.
    (@new [] [$dvis:vis] $name:ident $ctx:ty, $first:expr) => {
        $crate::machine!(@new_fn [$dvis] $name $ctx, $first);
    };
    (@new [private] [$dvis:vis] $name:ident $ctx:ty, $first:expr) => {
        $crate::machine!(@new_fn [] $name $ctx, $first);
    };
    (@new [$($nvis:tt)+] [$dvis:vis] $name:ident $ctx:ty, $first:expr) => {
        $crate::machine!(@new_fn [$($nvis)+] $name $ctx, $first);
    };
    (@new_fn [$($v:tt)*] $name:ident $ctx:ty, $first:expr) => {
        /// The machine in its first state.
        #[inline]
        #[allow(dead_code)]
        $($v)* fn new(ctx: $ctx) -> Self {
            $name { ctx, stored: $first }
        }
    };

    // Takes the stored state out for `with_typed`: a `Copy` one is copied,
    // any other is replaced by the first state while the typed state runs.
    (@take [Copy] $place:expr, $first:expr) => {
        $place
    };
    (@take [] $place:expr, $first:expr) => {
        ::core::mem::replace(&mut $place, $first)
    };

    (@state [$(#[$m:meta])*] [$vis:vis] $s:ident [$d:ty] [$ctx:ty] $any:ident) => {
        $(#[$m])*
        // A state that never reads the context or its data is fine.
        #[allow(dead_code)]
        $vis struct $s<'s> {
            ctx: &'s mut $ctx,
            data: $d,
        }

        impl<'s> $s<'s> {
            /// Enters this state (only the machine's module can).
            #[inline]
            #[allow(dead_code)]
            fn enter(ctx: &'s mut $ctx, data: $d) -> Self {
                $s { ctx, data }
            }
        }

        impl<'s> ::core::convert::From<$s<'s>> for $any<'s> {
            #[inline]
            fn from(state: $s<'s>) -> Self {
                $any::$s(state)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    crate::machine! {
        struct Door over u32 {
            #[derive(Debug, PartialEq)]
            stored DoorState;
            typed DoorAny;
            states { Closed, Open(u8) }
        }
    }

    impl<'s> Closed<'s> {
        fn open(self, width: u8) -> Open<'s> {
            *self.ctx += 1;
            Open::enter(self.ctx, width)
        }
    }

    impl<'s> Open<'s> {
        fn close(self) -> Closed<'s> {
            Closed::enter(self.ctx, ())
        }
        fn width(&self) -> u8 {
            self.data
        }
    }

    #[test]
    fn data_moves_with_its_state() {
        let mut door = Door::new(0);
        let w = door.with_typed(|s| match s {
            DoorAny::Closed(c) => {
                let o = c.open(9);
                let w = o.width();
                (o.into(), w)
            }
            other => (other, 0),
        });
        assert_eq!(w, 9);
        assert_eq!(*door.stored(), DoorState::Open(9));
        assert_eq!(*door.ctx(), 1);
        door.with_typed(|s| match s {
            DoorAny::Open(o) => (o.close().into(), ()),
            other => (other, ()),
        });
        assert_eq!(*door.stored(), DoorState::Closed(()));
    }

    #[test]
    fn a_unit_machine_stores_one_byte() {
        crate::machine! {
            struct Lamp over () { stored LampState; typed LampAny; states { Off, On } }
        }
        assert_eq!(core::mem::size_of::<LampState>(), 1);
        assert_eq!(core::mem::size_of::<Lamp>(), 1);
    }
}

#[cfg(test)]
mod visibility {
    mod m {
        crate::machine! {
            pub struct Gate over u8 {
                stored GateState;
                typed GateAny;
                states { Shut, Ajar }
                new(private);
                with_typed(pub(crate));
            }
        }
        pub fn gate() -> Gate {
            Gate::new(3)
        }
    }

    #[test]
    fn a_private_constructor_still_builds_inside_the_module() {
        let g = m::gate();
        assert_eq!(*g.ctx(), 3);
    }
}
