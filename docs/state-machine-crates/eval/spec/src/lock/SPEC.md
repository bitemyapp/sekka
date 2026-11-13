# `lock`: lock screen lifecycle and logind's sleep delay

Catalog: **SHB-12** (lock screen lifecycle) and **SYB-12** (logind
sleep-delay inhibitor and lock-before-sleep handshake). Source at
`8e0acbe5`: `desktop/aizome/crates/aizome-shell/src/lock.rs` (`Phase`
25–32, `LockState` 47–66; `lock` 307–336, `sync_lock_surfaces` 338–358,
`lock_confirmed` 361–369, `lock_ended` 371–388, `unlock` 390–396,
`lock_msg` 398–445: typing ignored while busy 402–405, Submit 416–420,
Result 422–436 replacing every PAM reason with "That password didn't work"
at 427–428, the shake at 430–432 and 437–441), `pam.rs`
(`Pam::authenticate` 146–186: the module's last message or `pam_strerror`),
`shell.rs` (`session_events` 2041–2073: PrepareForSleep 2052–2058, Resumed
retaking the delay 2059–2066; `start_lock` 2076–2093),
`desktop/aizome/crates/aizome-sys/src/power/logind.rs` (`take_sleep_delay`
381–390, PrepareForSleep handling 268–278, `sleep_ready` 393–396,
Vanished/Appeared 222–231).

## Types

- `Config { lock_before_sleep: true, motion: true, outputs: 0b1 }`.
  `outputs` is the starting set (bit n: output n, `n <= 31`).
- `PamOutcome`: `Accepted`, `WrongPassword`, `Refused(Refusal)` with
  `Refusal`: `AccountLocked`, `PasswordExpired`, `AccountExpired`, `Other`.
- `LockNote`: `WrongPassword`, `Refused(Refusal)`.
- `StateName { lock: LockPhase, delay: DelayPhase }`, with `LockPhase`:
  `Unlocked`, `Locking`, `Locked`, `Checking`, and `DelayPhase`: `NotHeld`,
  `Held`, `SleepPending`.

**Input domain:** any event in any state; output ids `<= 31`; any check id.

## States and data

```mermaid
stateDiagram-v2
    state LockRegion {
        [*] --> Unlocked
        Unlocked --> Lockscreen: Lock | PrepareForSleep [lock_before_sleep] / RequestLock, OpenSurface*
        state Lockscreen {
            [*] --> Locking
            Locking --> Locked: LockConfirmed / LockedHint(true) [, ReleaseDelay]
            state Locked {
                [*] --> Idle
                Idle --> Checking: Submit [has text] / StartCheck(n)
                Checking --> Idle: PamDone(n) refused / ShowNote, shake
            }
        }
        Lockscreen --> Unlocked: LockEnded | UnlockRequested | PamDone(n) Accepted
    }
    --
    state DelayRegion {
        [*] --> NotHeld
        NotHeld --> Held: SessionStarted | LogindAppeared | Resumed [lock_before_sleep] / TakeDelay
        Held --> SleepPending: PrepareForSleep [lock region Unlocked or Locking]
        Held --> NotHeld: PrepareForSleep [Locked or Checking] / ReleaseDelay
        SleepPending --> NotHeld: lock confirmed or lock screen left / ReleaseDelay
        SleepPending --> Held: Resumed (that sleep is over; the fd is still valid)
        Held --> NotHeld: LogindVanished
        SleepPending --> NotHeld: LogindVanished
    }
```

`StateName.lock` is `Locked` for `Locked.Idle`.

| State | Data |
|---|---|
| machine | `outputs: u32` (always tracked), `last_check: u32` (machine-wide, starts at 0) |
| `Lockscreen` (superstate of `Locking` and `Locked`) | `has_text: bool` (the field isn't empty) |
| `Locked` (superstate of `Idle` and `Checking`) | `shake: u8` (0: still; the shake timer runs while > 0) |
| `Checking` | `check: u32` (the PAM check running) |

Timer: the shake's 30 ms timer (`ArmShake` re-arms it, `CancelShake`),
running exactly while `shake > 0`. Entry and exit actions:

- **enter Lockscreen**: `RequestLock`, then `OpenSurface(o)` for every
  output in ascending order; `has_text = false`.
- **enter Locked** (from `Locking`): `LockedHint(true)`; if the delay region
  is `SleepPending`: `ReleaseDelay` (→ `NotHeld`); `shake = 0`.
- **leave Lockscreen** (to `Unlocked`), in order: `CancelShake` if in
  `Locked` with `shake > 0`; `ReleaseLock` if the transition releases the
  lock (`UnlockRequested`, `Accepted`; not `LockEnded`, where the
  compositor already ended it); `LockedHint(false)`; if `SleepPending`:
  `ReleaseDelay` (→ `NotHeld`).

## Transitions

| Event | Where | Effects, in order | Next |
|---|---|---|---|
| `Lock` | `Unlocked` | enter Lockscreen | `Locking` |
| | otherwise | none | same |
| `LockConfirmed` | `Locking` | enter Locked | `Locked` |
| | otherwise | none (late or duplicate) | same |
| `LockEnded` | `Lockscreen` | leave Lockscreen (no `ReleaseLock`) | `Unlocked` |
| `UnlockRequested` | `Lockscreen` | leave Lockscreen with `ReleaseLock` | `Unlocked` |
| `OutputAdded(o)` | any | if `o` is new: add it; then `OpenSurface(o)` if in `Lockscreen` | same |
| `OutputRemoved(o)` | any | if present: remove it; then `CloseSurface(o)` if in `Lockscreen` | same |
| `Typed { empty }` | `Locked.Idle` | `has_text = !empty` | same |
| | otherwise | none (ignored while locking or checking) | same |
| `Escape` | `Locked.Idle` | `has_text = false` | same |
| `Submit` | `Locked.Idle` with `has_text` | `last_check += 1`; `StartCheck { check: last_check }`; `has_text = false` | `Checking` |
| `PamDone { check, Accepted }` | `Checking` with that check | leave Lockscreen with `ReleaseLock` | `Unlocked` |
| `PamDone { check, WrongPassword }` | same | `ShowNote(WrongPassword)`; shake | `Locked.Idle` |
| `PamDone { check, Refused(r) }` | same | `ShowNote(Refused(r))`; shake | `Locked.Idle` |
| `PamDone` | otherwise | none (stale) | same |
| `ShakeTick` | `Locked` with `shake > 0` | `shake += 1`; if `shake >= 9`: `shake = 0`, `ShakeStep(0)`; else `ShakeStep(shake)`, `ArmShake` | same |
| | otherwise | none | same |
| `SessionStarted`, `LogindAppeared` | delay `NotHeld`, `lock_before_sleep` | `TakeDelay` | `Held` |
| `PrepareForSleep` | `!lock_before_sleep` | none | same |
| | `Unlocked` | enter Lockscreen; then delay `Held` → `SleepPending` | `Locking` |
| | `Locking` | delay `Held` → `SleepPending` | |
| | `Locked`/`Checking` | delay `Held`: `ReleaseDelay` → `NotHeld` | |
| `Resumed` | delay `SleepPending` | none | `Held` |
| | delay `NotHeld`, `lock_before_sleep` | `TakeDelay` | `Held` |
| | delay `Held` | none | same |
| `LogindVanished` | delay `Held`/`SleepPending` | none (the fd went with logind) | `NotHeld` |

**shake**: if `motion`: `shake = 1`, `ShakeStep(1)`, `ArmShake` (restarting
a running shake). Without `motion`, nothing.

## Invariants

1. `SleepPending` only while `Locking` (it ends when the lock is confirmed
   or the lock screen is left, or with the sleep: `Resumed`).
2. A PAM result acts only on the check that the current `Checking` started.
3. Only `Checking` has a check in flight from this lock; at most one.
4. `TakeDelay` is only emitted while no delay is held; `ReleaseDelay` only
   while one is.
5. Surfaces exist exactly for the current outputs while in `Lockscreen`.
6. The shake timer runs only in `Locked` and only while `shake > 0`.

## Deliberate differences from the current code

- **SHB-12 fix:** PAM's reason is shown; only a wrong password says "That
  password didn't work".
- **SYB-12 fix:** `Resumed` ends the pending sleep (that sleep is over) and
  doesn't take a second delay while one is held; so a lock confirmed after
  resume can't drop the delay that protects the next sleep.
- `PrepareForSleep` while `Locking` holds the delay until confirmation (the
  current code releases it at once, `shell.rs:2052-2056`).
- PAM results carry the check id (the current code acts on any result,
  even one from an earlier lock); `lock_confirmed` only acts in `Locking`.
- The field takes input only in `Locked.Idle` (the current code accepts a
  submit while `Locking`).
- Leaving the lock screen cancels the shake.
- Simplified away: the clock and the picture, caps lock, the password
  text, notifications, the follower's own double retake (one machine owns
  the delay), Inhibit failures (they're swallowed now; here `TakeDelay`
  succeeds), shutdown.

## Illegal operations (compile-fail probes)

1. **L1 release-delay-before-confirmed:** release the sleep delay for a
   pending sleep before the lock is confirmed (from `Locking` while still
   locking).
2. **L2 stale-pam-result:** act on a PAM result whose check isn't the
   current one (an earlier lock's).
3. **L3 check-while-unlocked:** start a PAM check outside `Locked.Idle`
   (while `Locking`, `Checking`, `Unlocked`).
4. **L4 pending-sleep-after-resume:** keep a pending sleep across `Resumed`
   (so a later confirmation releases a fresh delay). (SYB-12)
5. **L5 refusal-as-wrong-password:** turn a `Refused(r)` into
   `WrongPassword`. (SHB-12)
6. **L6 shake-outside-locked:** arm the shake timer while not locked.
7. **L7 double-delay:** take the delay while one is held.

## Confirmed-bug scenarios (traces, as the correct behaviour)

- **SHB-12** `a-lockout-says-why` and `an-expired-password-says-why`: a
  faillock lockout or an expired password reports PAM's reason (repro
  `repro/out/shell-b-sys-b.md`, SHB-12: all five refusals showed the
  wrong-password text).
- **SYB-12** `lock-confirmed-after-resume-keeps-the-next-delay`: sleep with
  the lock unconfirmed, resume, then the compositor confirms: the delay
  must still be held for the next sleep (repro: no inhibitor held after
  confirmation).
