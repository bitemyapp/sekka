"""Shares the screen as a browser does: a ScreenCast portal session on
what the person picks (in Aizome, with its picker), started, and its
PipeWire stream read through the remote the portal opens; then counts
the frames that came. It fails with fewer than MIN_FRAMES (the stream
that froze after one).

    aizome-screencast-check SECONDS MIN_FRAMES [TYPES]

TYPES is the portal's source types: 1 a display (the default), 2 a
window, 3 either. A share the person refuses exits with the portal's
response (1: cancelled).
"""

import re
import subprocess
import sys

import dbus
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

seconds, least = int(sys.argv[1]), int(sys.argv[2])
types = int(sys.argv[3]) if len(sys.argv) > 3 else 1
DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()
portal = bus.get_object("org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop")
screencast = dbus.Interface(portal, "org.freedesktop.portal.ScreenCast")
sender = bus.get_unique_name()[1:].replace(".", "_")
tokens = iter(range(1, 1000))


def request(call, *args, options):
    """A portal request, and the results of its Response."""
    token = f"aizome{next(tokens)}"
    path = f"/org/freedesktop/portal/desktop/request/{sender}/{token}"
    loop = GLib.MainLoop()
    out = {}

    def response(code, results):
        out["code"], out["results"] = int(code), results
        loop.quit()

    bus.add_signal_receiver(response, "Response", "org.freedesktop.portal.Request", None, path)
    options = dict(options, handle_token=token)
    call(*args, dbus.Dictionary(options, signature="sv"))
    GLib.timeout_add_seconds(30, loop.quit)
    loop.run()
    if out.get("code") != 0:
        sys.exit(f"{call._method_name}: response {out.get('code')}")
    return out["results"]


session = request(screencast.CreateSession, options={"session_handle_token": "aizome"})["session_handle"]
# What may be shared (the portal asks the person), the pointer in the
# picture.
request(screencast.SelectSources, session, options={"types": dbus.UInt32(types), "cursor_mode": dbus.UInt32(2)})
streams = request(screencast.Start, session, "", options={})["streams"]
print(f"portal: PipeWire node {int(streams[0][0])}, {dict(streams[0][1])}", flush=True)
fd = screencast.OpenPipeWireRemote(session, dbus.Dictionary({}, signature="sv")).take()
# Through the portal's remote the stream is the only video source; a
# video input, as browsers' streams are, for WirePlumber to link it. Not
# live, so frames are taken as they come, as a browser takes them: a live
# pipewiresrc holds each buffer until the clock reaches its timestamp,
# counted from the first buffer's, and the portal's first can come with
# none (pts 0, when its stream pauses as this one links while that
# capture is in flight). The next would then wait as long as the machine
# has been up, and the stream stall after one frame.
pipeline = (
    f"pipewiresrc fd={fd} do-timestamp=true"
    " stream-properties=props,media.class=(string)Stream/Input/Video,stream.is-live=(string)false"
    " ! identity silent=false ! fakesink sync=false enable-last-sample=false"
)
gst = subprocess.run(
    # SIGINT for an EOS; SIGKILL if a stuck pipeline never finishes.
    ["timeout", "-k", "5", "-s", "INT", str(seconds), "gst-launch-1.0", "-e", "-v", *pipeline.split()],
    pass_fds=[fd],
    stdout=subprocess.PIPE,
    stderr=subprocess.STDOUT,
    text=True,
)
frames = re.findall(r"last-message = chain.*?\((\d+) bytes", gst.stdout)
caps = next((l.split("caps = ", 1)[1] for l in gst.stdout.splitlines() if "pipewiresrc" in l and "caps = " in l), "")
print(f"negotiated: {caps[:200]}")
print(f"frames: {len(frames)} in {seconds} s")
if len(frames) < least:
    print("\n".join(gst.stdout.splitlines()[-20:]))
    sys.exit(f"only {len(frames)} frames in {seconds} s (at least {least} expected)")
