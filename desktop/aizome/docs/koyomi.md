# Koyomi 暦 (Calendar)

Koyomi keeps calendars as iCalendar files that any program can read,
shows them in month, week, day and year views, and fires their alerts as
desktop notifications through a small user service. The crate is
`apps/koyomi` (library `koyomi`, binary `aizome-koyomi`, app id
`org.aizome.Koyomi`).

## Files

| What | Where |
|---|---|
| Calendars | `$XDG_DATA_HOME/aizome/koyomi/<id>.ics`, one calendar per file (RFC 5545) |
| Removed calendars | `$XDG_DATA_HOME/aizome/koyomi/.trash/<id>-<unix time>.ics` |
| Settings | `$XDG_CONFIG_HOME/aizome/koyomi.toml` |
| Window state (view, hidden calendars, sidebar width) | `$XDG_STATE_HOME/aizome/koyomi/state.json` |
| The agent's memory (alerts shown, snoozed) | `$XDG_STATE_HOME/aizome/koyomi/agent.json` |

A calendar's id is its file name without `.ics`. Its name is
`X-WR-CALNAME`; its colour is one of the theme's dyes, kept as
`X-AIZOME-COLOR` (`hanada`, `ai`, `kamenozoki`, `cyan`, `green`, `red`,
`violet`, `kohaku`) with `X-APPLE-CALENDAR-COLOR` beside it for other
programs; a file from elsewhere gets the dye nearest its Apple, Google or
Outlook colour. Koyomi writes a whole file at a time (to a temporary file,
then renamed over the old one) and reads a file again when another program
changes it. Dropping an `.ics` file into the folder adds a calendar.

On first start, an empty folder gets two calendars, Home and Work.

### What Koyomi reads and writes

- `VEVENT` with `DTSTART`, `DTEND` or `DURATION` (or neither: a day for
  dates, an instant otherwise), all-day dates, floating times, UTC times
  and `TZID`s. A `TZID` is resolved as an IANA zone however it's dressed
  (`/mozilla.org/…/Europe/Berlin`, quoted), through the file's
  `VTIMEZONE` and its `X-LIC-LOCATION`, as a Windows zone name (Outlook,
  Exchange), and failing those as the `VTIMEZONE`'s own rules (read as a
  POSIX rule, or a fixed offset). An unknown zone is read as floating time.
- `RRULE` with `FREQ` (`DAILY`, `WEEKLY`, `MONTHLY`, `YEARLY`, and
  `HOURLY`/`MINUTELY`), `INTERVAL`, `COUNT`, `UNTIL`, `BYDAY` (with
  ordinals: `-1FR`, `20MO`), `BYMONTHDAY` (negative too), `BYMONTH`,
  `BYYEARDAY`, `BYSETPOS`, `BYHOUR`, `BYMINUTE` and `WKST`. `BYWEEKNO` and
  `BYSECOND` are kept and written back but not expanded. Recurring events
  repeat in their own zone's wall-clock time across DST changes.
- `RDATE`, `EXDATE`, and changed instances (`RECURRENCE-ID`); a cancelled
  instance (`STATUS:CANCELLED`) hides its occurrence.
- `VALARM`: `TRIGGER` relative to the start or the end, or at a set time;
  `ACTION` `DISPLAY`, `AUDIO` (both alert), `EMAIL` (kept), Apple's `NONE`
  (ignored). An all-day event's alerts are relative to its day's midnight
  on the wall clock (`-PT15H` is 09:00 the day before).
- Everything else (attendees, organisers, `X-` properties, to-dos,
  journal entries) is kept as read and written back unchanged. Text is
  escaped, lines are folded at 75 octets, and each zone the events use is
  written as a `VTIMEZONE` (the file's own, or made from the IANA rules
  with `X-LIC-LOCATION`).

The tests read real exports from Google Calendar, Apple Calendar and
Outlook (Exchange) and a file of quirks (`apps/koyomi/tests/fixtures`),
check where their events fall, and write them back and read them again
unchanged.

## Command line

```
aizome-koyomi [FILE.ics…]          open the calendar (and import the files)
aizome-koyomi --show YYYY-MM-DD    open the calendar at a day (or "today")
aizome-koyomi --new                open the editor for a new event
aizome-koyomi --upcoming [--json] [--days N] [--limit N]
aizome-koyomi --list [--json]      the calendars
aizome-koyomi --export ID          a calendar as iCalendar, on standard output
aizome-koyomi --import FILE --calendar ID
aizome-koyomi --agent              the alert agent
```

A second launch of the window hands its arguments to the first (through
`$XDG_RUNTIME_DIR/aizome-app.koyomi.sock`) and exits.

### `--upcoming --json`

For the shell's clock panel and for scripts: the events from now to the
end of the day `N − 1` days on (today counts; `N` is 7 unless given), of
the calendars not hidden in the window, under way ones included, at most
`--limit` (100).

```json
{
  "version": 1,
  "now": "2026-10-17T14:20:00+01:00",
  "time_zone": "Europe/London",
  "days": 7,
  "events": [
    {
      "title": "Dinner at Hélène's",
      "start": "2026-10-17T19:30:00+01:00",
      "end": "2026-10-17T22:00:00+01:00",
      "date": "2026-10-17",
      "all_day": false,
      "in_progress": false,
      "location": "Bermondsey",
      "calendar": "home",
      "calendar_name": "Home",
      "dye": "kohaku",
      "color": "#e0af68",
      "uid": "dinner@test",
      "recurring": false
    }
  ]
}
```

- Times are RFC 3339 with the local offset; an all-day event starts at
  its first midnight and ends at the midnight after its last day.
- `date` is the local day it starts on; events are in order of day,
  all-day ones first, then by start.
- `dye` is the calendar's dye name and `color` its Night colour; the
  shell draws dyes in its own theme.
- `version` changes when a field changes meaning; fields may be added.

Without `--json` it prints the same as lines a person reads.

## Alerts

`aizome-koyomi --agent` runs as the systemd user service
`apps/koyomi/data/aizome-koyomi-agent.service` (wanted by
`graphical-session.target`; the package replaces `@bindir@` and `@systemctl@`
in it). A start-time condition requires an active graphical session without
keeping that target alive after logout. `PartOf` stops the agent with the
session, including when the user manager persists. It
reads the calendars (again whenever their files change, looking every 30
seconds), sleeps until the next alert, and posts it to
`org.freedesktop.Notifications` with the event's title, its time ("In 10
min · 09:30–09:45 · Room 4.12"), `desktop-entry` `org.aizome.Koyomi`, no
expiry, and two actions:

- **Snooze** puts it off for the settings' snooze length (10 minutes) and
  posts it again then;
- **Open** (or a click on the notification) runs `aizome-koyomi --show
  <its day>`.

It remembers which alerts it showed, so a restart doesn't repeat them;
after a sleep it shows those it missed within the last six hours if their
events aren't over.

`nix build .#aizome-checks.koyomi-session` exercises normal GDM
Xfce→GNOME→Aizome logins and logouts with one persistent user manager. It
checks that the agent runs in each session, stops after logout and refuses
a manual start outside a graphical session.

## Settings

`koyomi.toml`, all optional (the window's Settings sheet writes it):

| Key | Default | |
|---|---|---|
| `first_day` | `"locale"` | `"monday"`, `"sunday"`, `"saturday"`, or as `LC_TIME` (`LC_ALL`, `LANG`) has it |
| `week_numbers` | `true` | Shown in the month's left column and the week's header; numbered as the locale does (ISO 8601 in most of Europe, the week with 1 January elsewhere) |
| `clock` | `"system"` | `"24h"`, `"12h"`, or as the menu bar's clock reads |
| `default_alert` | `10` | Minutes before a new timed event (omit for none) |
| `default_all_day_alert` | `900` | Minutes before a new all-day event's day (900 is 09:00 the day before) |
| `snooze_minutes` | `10` | |
| `day_starts` | `8` | The hour the week and day views show at the top |
| `default_calendar` | the first shown | The id of the calendar new events go in |

## Keys

| | |
|---|---|
| ← → | Previous, next day, week, month or year (❖← ❖→ from anywhere) |
| T, ❖T | Today |
| ❖1 ❖2 ❖3 ❖4 | Day, week, month, year |
| ❖N, ⇧❖N | New event, new calendar |
| ↩, ❖I | Edit the selected event |
| ⌫ ⌦, ❖⌫ | Delete it (one of a series: this one, this and later, or all) |
| ❖D | Duplicate it |
| ❖Z, ⇧❖Z | Undo, redo (moves, edits, deletions, calendars) |
| ⇧❖I | Import |
| ⌥❖S | Show or hide the sidebar |
