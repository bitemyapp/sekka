//! Kitty's clipboard protocol (OSC 5522) through the terminal, after pinned
//! Ghostty's `stream_terminal.zig` and `stream_handler.zig` tests: the
//! terminal answers malformed and oversized writes itself and reports whole
//! reads and committed writes, in order with OSC 52's requests.

use std::sync::Arc;

use hotaru_core::kitty_clipboard::{Direction, Location, Request, Status, Terminator};
use hotaru_core::{Terminal, TerminalEvent};

fn terminal() -> Terminal {
    Terminal::new(20, 4, 0)
}

fn requests(terminal: &mut Terminal) -> Vec<Request> {
    terminal
        .take_events()
        .into_iter()
        .filter_map(|event| match event {
            TerminalEvent::KittyClipboard(request) => Some(request),
            _ => None,
        })
        .collect()
}

fn responses(terminal: &mut Terminal) -> String {
    String::from_utf8(terminal.take_responses()).unwrap()
}

fn one(terminal: &mut Terminal) -> Request {
    let mut requests = requests(terminal);
    assert_eq!(requests.len(), 1, "{requests:?}");
    requests.remove(0)
}

fn write(terminal: &mut Terminal) -> hotaru_core::kitty_clipboard::Write {
    match one(terminal) {
        Request::Write(write) => *write,
        other => panic!("not a write: {other:?}"),
    }
}

fn read(terminal: &mut Terminal) -> hotaru_core::kitty_clipboard::Read {
    match one(terminal) {
        Request::Read(read) => *read,
        other => panic!("not a read: {other:?}"),
    }
}

fn contents(write: &hotaru_core::kitty_clipboard::Write) -> Vec<(&str, &str)> {
    write
        .contents
        .iter()
        .map(|(mime, data)| (mime.as_str(), std::str::from_utf8(data).unwrap()))
        .collect()
}

#[test]
fn a_write_transaction_commits_types_chunks_and_aliases_whatever_the_feed_split() {
    let input = concat!(
        "\x1b]5522;type=write:id=42\x1b\\",
        "\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;R2hvc3Q=\x1b\\", // "Ghost"
        "\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;dHk=\x1b\\",     // "ty"
        "\x1b]5522;type=wdata:mime=dGV4dC9odG1s;PGI+aGk8L2I+\x1b\\", // "<b>hi</b>"
        "\x1b]5522;type=walias:mime=dGV4dC9wbGFpbg==;VEVYVCBVVEY4X1NUUklORw==\x1b\\",
        "\x1b]5522;type=wdata\x1b\\",
    );
    for size in [1, 5, 64, 4096] {
        let mut terminal = terminal();
        for chunk in input.as_bytes().chunks(size) {
            terminal.feed(chunk);
        }
        let write = write(&mut terminal);
        assert_eq!(
            contents(&write),
            [
                ("text/plain", "Ghostty"),
                ("text/html", "<b>hi</b>"),
                ("TEXT", "Ghostty"),
                ("UTF8_STRING", "Ghostty")
            ]
        );
        assert_eq!(
            (write.id.as_str(), write.loc, write.terminator),
            ("42", Location::Standard, Terminator::St)
        );
        assert_eq!(write.size(), 7 + 9, "aliases share their target's bytes");
        assert!(!write.granted);
        // The application answers it, once it's on the clipboard.
        assert_eq!(responses(&mut terminal), "");
        let mut done = Vec::new();
        write.status(Status::Done, &mut done);
        assert_eq!(done, b"\x1b]5522;type=write:status=DONE:id=42\x1b\\");
        // The transaction ended with its commit.
        terminal.feed(b"\x1b]5522;type=wdata\x1b\\");
        assert!(requests(&mut terminal).is_empty());
    }
}

#[test]
fn an_empty_commit_clears_and_the_location_and_terminator_carry_through() {
    let mut terminal = terminal();
    terminal.feed(b"\x1b]5522;type=write:loc=primary\x07\x1b]5522;type=wdata\x07");
    let write = write(&mut terminal);
    assert!(write.contents.is_empty());
    assert_eq!(
        (write.loc, write.terminator),
        (Location::Primary, Terminator::Bel)
    );
    let mut done = Vec::new();
    write.status(Status::Done, &mut done);
    assert_eq!(done, b"\x1b]5522;type=write:status=DONE\x07");
}

#[test]
fn a_read_lists_and_names_its_types_in_order() {
    let mut terminal = terminal();
    // ". text/plain text/html"
    terminal.feed(b"\x1b]5522;type=read:loc=primary:id=r1;LiB0ZXh0L3BsYWluIHRleHQvaHRtbA==\x1b\\");
    let read = read(&mut terminal);
    assert_eq!(read.mimes, ["text/plain", "text/html"]);
    assert!(read.list && !read.granted && !read.prompt_exempt());
    assert_eq!(
        (read.loc, read.id.as_str(), read.name.as_str()),
        (Location::Primary, "r1", "")
    );
    let available = ["text/plain".to_string(), "text/html".to_string()];
    let served = [
        ("text/plain".to_string(), Arc::<[u8]>::from(&b"Ghostty"[..])),
        (
            "text/html".to_string(),
            Arc::<[u8]>::from(&b"<b>hi</b>"[..]),
        ),
    ];
    let mut out = Vec::new();
    read.success(&available, &served, &mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "\x1b]5522;type=read:status=OK:loc=primary:id=r1\x1b\\",
            "\x1b]5522;type=read:status=DATA:id=r1:mime=Lg==;dGV4dC9wbGFpbiB0ZXh0L2h0bWwK\x1b\\",
            "\x1b]5522;type=read:status=DATA:id=r1:mime=dGV4dC9wbGFpbg==;R2hvc3R0eQ==\x1b\\",
            "\x1b]5522;type=read:status=DATA:id=r1:mime=dGV4dC9odG1s;PGI+aGk8L2I+\x1b\\",
            "\x1b]5522;type=read:status=DONE:id=r1\x1b\\",
        )
    );

    terminal.feed(b"\x1b]5522;type=read:id=r2;dGV4dC9wbGFpbg==\x07");
    let read = self::read(&mut terminal);
    let mut out = Vec::new();
    read.success(&[], &served[..1], &mut out);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        concat!(
            "\x1b]5522;type=read:status=OK:id=r2\x07",
            "\x1b]5522;type=read:status=DATA:id=r2:mime=dGV4dC9wbGFpbg==;R2hvc3R0eQ==\x07",
            "\x1b]5522;type=read:status=DONE:id=r2\x07",
        )
    );
    let mut out = Vec::new();
    read.status(Status::Eperm, &mut out);
    assert_eq!(out, b"\x1b]5522;type=read:status=EPERM:id=r2\x07");

    terminal.feed(b"\x1b]5522;type=read;Lg==\x1b\\");
    let read = self::read(&mut terminal);
    assert!(read.list && read.mimes.is_empty() && read.prompt_exempt());
}

#[test]
fn a_read_serves_at_most_four_types_but_still_sees_a_later_listing() {
    let mut terminal = terminal();
    // "a/0 a/1 a/2 a/3 a/4 a/5 ."
    terminal.feed(b"\x1b]5522;type=read;YS8wIGEvMSBhLzIgYS8zIGEvNCBhLzUgLg==\x1b\\");
    let read = read(&mut terminal);
    assert_eq!(read.mimes, ["a/0", "a/1", "a/2", "a/3"]);
    assert!(read.list);
}

#[test]
fn read_grants_need_a_name_and_a_listing_never_spends_a_one_time_grant() {
    let mut terminal = terminal();
    // pw="secret", name="app", reading text/plain.
    let secret = b"\x1b]5522;type=read:pw=c2VjcmV0:name=YXBw;dGV4dC9wbGFpbg==\x1b\\";
    terminal.feed(secret);
    let first = read(&mut terminal);
    assert_eq!(
        (first.pw.as_str(), first.name.as_str(), first.granted),
        ("secret", "app", false)
    );
    terminal.kitty_clipboard_grant(&first.pw, Direction::Read);
    terminal.feed(secret);
    assert!(read(&mut terminal).granted);
    terminal.feed(b"\x1b]5522;type=read:pw=b3RoZXI=:name=YXBw;dGV4dC9wbGFpbg==\x1b\\");
    assert!(!read(&mut terminal).granted, "another password");
    terminal.feed(b"\x1b]5522;type=read:pw=c2VjcmV0;dGV4dC9wbGFpbg==\x1b\\");
    let nameless = read(&mut terminal);
    assert!(
        !nameless.granted && nameless.pw.is_empty(),
        "a password without a name is none"
    );
    terminal.feed(b"\x1b]5522;type=write:pw=c2VjcmV0:name=YXBw\x1b\\\x1b]5522;type=wdata\x1b\\");
    assert!(!write(&mut terminal).granted, "directions are separate");

    // A paste event's one-time password survives a listing.
    terminal.feed(b"\x1b[?5522h");
    let available = ["text/plain".to_string()];
    let event = terminal
        .kitty_paste_event(false, &available, &(0..=255).collect::<Vec<u8>>())
        .unwrap();
    let event = String::from_utf8(event).unwrap();
    let pw = event
        .strip_prefix("\x1b]5522;type=read:status=OK:pw=")
        .unwrap();
    let pw = &pw[..pw.find('\x1b').unwrap()];
    let name = "UGFzdGUgZXZlbnQ="; // "Paste event"
    terminal.feed(format!("\x1b]5522;type=read:pw={pw}:name={name};Lg==\x1b\\").as_bytes());
    assert!(!read(&mut terminal).granted);
    let data = format!("\x1b]5522;type=read:pw={pw}:name={name};dGV4dC9wbGFpbg==\x1b\\");
    terminal.feed(data.as_bytes());
    assert!(read(&mut terminal).granted);
    terminal.feed(data.as_bytes());
    assert!(!read(&mut terminal).granted, "once");
}

#[test]
fn write_grants_carry_into_the_commit() {
    let mut terminal = terminal();
    let secret = b"\x1b]5522;type=write:pw=c2VjcmV0:name=YXBw\x1b\\\x1b]5522;type=wdata\x1b\\";
    terminal.feed(secret);
    let first = write(&mut terminal);
    assert!(!first.granted);
    terminal.kitty_clipboard_grant(&first.pw, Direction::Write);
    terminal.feed(secret);
    assert!(write(&mut terminal).granted);
    terminal.feed(b"\x1b]5522;type=write:pw=c2VjcmV0\x1b\\\x1b]5522;type=wdata\x1b\\");
    let nameless = write(&mut terminal);
    assert!(!nameless.granted && nameless.name.is_empty());
}

#[test]
fn malformed_and_orphaned_packets_are_dropped_silently() {
    let mut terminal = terminal();
    for packet in [
        &b"\x1b]5522;loc=primary\x1b\\"[..],
        b"\x1b]5522;type=bobr\x1b\\",
        b"\x1b]5522;type=read:bare\x1b\\",
        b"\x1b]5522;\x1b\\",
        b"\x1b]5522;;\x1b\\",
        b"\x1b]5522;type=wdata:mime=!!!;R2hvc3Q=\x1b\\",
        b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;R2hvc3Q=\x1b\\",
        b"\x1b]5522;type=walias:mime=dGV4dC9wbGFpbg==;VEVYVA==\x1b\\",
        b"\x1b]5522;type=wdata\x1b\\",
        // A read list that isn't padded base64, or isn't UTF-8.
        b"\x1b]5522;type=read;dGV4dC9wbGFpbg\x1b\\",
        b"\x1b]5522;type=read;/w==\x1b\\",
    ] {
        terminal.feed(packet);
        assert!(terminal.take_events().is_empty(), "{packet:?}");
        assert_eq!(responses(&mut terminal), "", "{packet:?}");
    }
    terminal.feed(b"ok");
    assert!(terminal.snapshot().plain_text().starts_with("ok"));
}

#[test]
fn a_new_write_replaces_one_in_flight() {
    let mut terminal = terminal();
    terminal.feed(
        concat!(
            "\x1b]5522;type=write:id=old\x1b\\",
            "\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;b2xk\x1b\\",
            "\x1b]5522;type=write:id=new\x1b\\",
            "\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;bmV3\x1b\\",
            "\x1b]5522;type=wdata\x1b\\",
        )
        .as_bytes(),
    );
    let write = write(&mut terminal);
    assert_eq!(
        (write.id.as_str(), contents(&write)),
        ("new", vec![("text/plain", "new")])
    );
    assert_eq!(responses(&mut terminal), "");
}

#[test]
fn invalid_write_packets_end_the_write_with_einval_once() {
    for packet in [
        // An alias list that decodes to a byte that isn't UTF-8.
        &b"\x1b]5522;type=walias:mime=dGV4dC9wbGFpbg==;/w==\x1b\\"[..],
        // An alias without a target.
        b"\x1b]5522;type=walias;VEVYVA==\x1b\\",
        // Targets and types that aren't UTF-8, or aren't base64.
        b"\x1b]5522;type=walias:mime=//4=;VEVYVA==\x1b\\",
        b"\x1b]5522;type=wdata:mime=//4=;R2hvc3Q=\x1b\\",
        b"\x1b]5522;type=wdata:mime=!!!;R2hvc3Q=\x1b\\",
        // Corrupt data, and data after padding in one packet.
        b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;!!!bad!!!\x1b\\",
        b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;Z29vZA==bW9yZQ==\x1b\\",
    ] {
        let mut terminal = terminal();
        terminal.feed(b"\x1b]5522;type=write:id=w\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;R2hvc3Q=\x1b\\");
        terminal.feed(packet);
        assert_eq!(
            responses(&mut terminal),
            "\x1b]5522;type=write:status=EINVAL:id=w\x1b\\",
            "{packet:?}"
        );
        terminal.feed(
            b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;V29ybGQ=\x1b\\\x1b]5522;type=wdata\x1b\\",
        );
        assert!(terminal.take_events().is_empty());
        assert_eq!(responses(&mut terminal), "");
    }
}

#[test]
fn a_read_that_isnt_valid_leaves_a_write_alone() {
    let mut terminal = terminal();
    terminal.feed(
        b"\x1b]5522;type=write:id=w\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;R2hvc3Q=\x1b\\",
    );
    terminal.feed(b"\x1b]5522;type=read;/w==\x1b\\");
    terminal.feed(b"\x1b]5522;type=read:mime=!!!;dGV4dC9wbGFpbg==\x1b\\");
    terminal.feed(b"\x1b]5522;type=wdata\x1b\\");
    assert_eq!(contents(&write(&mut terminal)), [("text/plain", "Ghost")]);
    assert_eq!(responses(&mut terminal), "");
}

#[test]
fn writes_past_the_limit_end_with_efbig_and_unpadded_ones_with_einval() {
    let mut terminal = terminal();
    terminal.set_kitty_clipboard_write_limit(4);
    terminal.feed(
        b"\x1b]5522;type=write:id=w\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;SGVsbA==\x1b\\",
    );
    assert_eq!(responses(&mut terminal), "");
    terminal.feed(b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;bw==\x1b\\");
    assert_eq!(
        responses(&mut terminal),
        "\x1b]5522;type=write:status=EFBIG:id=w\x1b\\"
    );
    terminal
        .feed(b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;IQ==\x1b\\\x1b]5522;type=wdata\x1b\\");
    assert!(terminal.take_events().is_empty());
    assert_eq!(responses(&mut terminal), "");

    let mut terminal = self::terminal();
    terminal.feed(
        b"\x1b]5522;type=write:id=w\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;SGVsbG8\x1b\\",
    );
    assert_eq!(responses(&mut terminal), "");
    terminal.feed(b"\x1b]5522;type=wdata\x1b\\");
    assert_eq!(
        responses(&mut terminal),
        "\x1b]5522;type=write:status=EINVAL:id=w\x1b\\"
    );
    assert!(terminal.take_events().is_empty());
}

#[test]
fn one_write_can_be_as_large_as_the_osc_capture_allows_per_packet() {
    let mut terminal = terminal();
    let chunk = vec![b'A'; 4 * 1024 * 1024];
    terminal.feed(b"\x1b]5522;type=write\x1b\\");
    for _ in 0..3 {
        terminal.feed(b"\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;");
        terminal.feed(&chunk);
        terminal.feed(b"\x1b\\");
    }
    terminal.feed(b"\x1b]5522;type=wdata\x1b\\");
    let write = write(&mut terminal);
    assert_eq!(write.contents[0].1.len(), 9 * 1024 * 1024);
    assert!(write.contents[0].1.iter().all(|&b| b == 0));
}

#[test]
fn requests_share_osc_52s_ordered_prefix_and_past_it_hear_ebusy() {
    let mut terminal = terminal();
    let mut input = Vec::new();
    for i in 0..70 {
        if i % 2 == 0 {
            input.extend_from_slice(
                format!("\x1b]5522;type=read:id={i};dGV4dC9wbGFpbg==\x1b\\").as_bytes(),
            );
        } else {
            input.extend_from_slice(b"\x1b]52;c;?\x07");
        }
    }
    terminal.feed(&input);
    let events = terminal.take_events();
    assert_eq!(events.len(), 64);
    assert!(
        matches!(&events[0], TerminalEvent::KittyClipboard(Request::Read(read)) if read.id == "0")
    );
    assert!(matches!(
        &events[1],
        TerminalEvent::Clipboard {
            selection: b'c',
            ..
        }
    ));
    assert_eq!(
        responses(&mut terminal),
        concat!(
            "\x1b]5522;type=read:status=EBUSY:id=64\x1b\\",
            "\x1b]52;c;\x1b\\",
            "\x1b]5522;type=read:status=EBUSY:id=66\x1b\\",
            "\x1b]52;c;\x1b\\",
            "\x1b]5522;type=read:status=EBUSY:id=68\x1b\\",
            "\x1b]52;c;\x1b\\",
        )
    );
}

#[test]
fn mode_5522_is_reported_saved_and_reset() {
    let mut terminal = terminal();
    assert!(!terminal.modes().kitty_paste_events);
    terminal.feed(b"\x1b[?5522$p");
    assert_eq!(responses(&mut terminal), "\x1b[?5522;2$y");
    terminal.feed(b"\x1b[?5522h\x1b[?5522$p");
    assert_eq!(responses(&mut terminal), "\x1b[?5522;1$y");
    assert!(terminal.modes().kitty_paste_events);
    terminal.feed(b"\x1b[?5522s\x1b[?5522l");
    assert!(!terminal.modes().kitty_paste_events);
    terminal.feed(b"\x1b[?5522r");
    assert!(terminal.modes().kitty_paste_events);
    terminal.feed(b"\x1bc");
    assert!(!terminal.modes().kitty_paste_events);
    assert_eq!(
        terminal.kitty_paste_event(false, &[], &[0; 64]),
        None,
        "no events once it's off"
    );
}

#[test]
fn paste_events_list_every_type_with_a_fresh_password_each_time() {
    let mut terminal = terminal();
    terminal.feed(b"\x1b[?5522h\x1b[?2004h");
    let random: Vec<u8> = (0..=255).collect();
    let available = ["text/plain".to_string(), "image/png".to_string()];
    let event = String::from_utf8(
        terminal
            .kitty_paste_event(false, &available, &random)
            .unwrap(),
    )
    .unwrap();
    let pw = event
        .strip_prefix("\x1b]5522;type=read:status=OK:pw=")
        .unwrap();
    let pw = &pw[..pw.find('\x1b').unwrap()];
    assert_eq!(pw.len(), 32, "22 characters, encoded");
    assert_eq!(
        event,
        format!(
            "\x1b]5522;type=read:status=OK:pw={pw}\x1b\\\x1b]5522;type=read:status=DATA:mime=Lg==:pw={pw};dGV4dC9wbGFpbiBpbWFnZS9wbmcK\x1b\\\x1b]5522;type=read:status=DONE:pw={pw}\x1b\\"
        )
    );
    assert!(!event.contains("\x1b[200~"));
    let random: Vec<u8> = (0..=255).rev().collect();
    let primary = String::from_utf8(
        terminal
            .kitty_paste_event(true, &available, &random)
            .unwrap(),
    )
    .unwrap();
    assert!(primary.starts_with("\x1b]5522;type=read:status=OK:loc=primary:pw="));
    assert_eq!(primary.matches("loc=").count(), 1, "only on the OK packet");
    assert!(!primary.contains(pw));
    assert_eq!(
        terminal.kitty_paste_event(false, &available, &[255; 64]),
        None,
        "not enough entropy"
    );
}

#[test]
fn a_reset_ends_grants_but_not_answers_requests_or_a_write_already_made() {
    let mut terminal = terminal();
    terminal.feed(b"\x1b]5522;type=read:pw=c2VjcmV0:name=YXBw;dGV4dC9wbGFpbg==\x1b\\");
    let read = read(&mut terminal);
    terminal.kitty_clipboard_grant(&read.pw, Direction::Read);
    terminal.feed(
        b"\x1b]5522;type=write:id=w\x1b\\\x1b]5522;type=wdata:mime=dGV4dC9wbGFpbg==;b2s=\x1b\\",
    );
    // A query and a read before the reset are still answered and reported.
    terminal.feed(b"\x1b[c\x1b]52;c;?\x07\x1bc");
    assert_eq!(responses(&mut terminal), "\x1b[?62;22c");
    assert_eq!(terminal.take_events().len(), 1);
    terminal.feed(b"\x1b]5522;type=read:pw=c2VjcmV0:name=YXBw;dGV4dC9wbGFpbg==\x1b\\");
    assert!(!self::read(&mut terminal).granted);
    terminal.feed(b"\x1b]5522;type=wdata\x1b\\");
    assert_eq!(contents(&write(&mut terminal)), [("text/plain", "ok")]);
    terminal.reset();
    terminal.feed(b"\x1b]5522;type=read:pw=c2VjcmV0:name=YXBw;dGV4dC9wbGFpbg==\x1b\\");
    assert!(!self::read(&mut terminal).granted);
}
