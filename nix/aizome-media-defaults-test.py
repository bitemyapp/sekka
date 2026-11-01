"""Isolated XDG resolution/dispatch; no real player or user settings touched."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

with tempfile.TemporaryDirectory() as temporary:
    root = Path(temporary)
    config = root / "config"
    system = root / "system"
    data = root / "data"
    applications = data / "applications"
    binaries = root / "bin"
    for directory in (config, system, applications, binaries):
        directory.mkdir(parents=True, exist_ok=True)
    desktop = Path(os.environ["desktopFile"]).read_text()
    types = next(line[9:] for line in desktop.splitlines() if line.startswith("MimeType=")).strip(";").split(";")
    defaults = Path(os.environ["defaultsFile"]).read_text()
    for kind in types:
        assert f"{kind}=org.aizome.Utsushi.desktop;" in defaults.splitlines(), kind
    # Explicitly cover the formats omitted by the old separate list.
    for kind in ("video/x-ms-wmv", "video/3gpp2", "video/x-ogm+ogg", "audio/x-aiff",
                 "audio/x-ape", "audio/x-wavpack", "audio/x-ms-wma"):
        assert kind in types, kind
    (system / "aizome-mimeapps.list").write_text(defaults)
    (system / "mimeapps.list").write_text("[Default Applications]\n" + "".join(f"{kind}=mpv.desktop;\n" for kind in types))
    (applications / "org.aizome.Utsushi.desktop").write_text(desktop)
    (applications / "mpv.desktop").write_text(
        "[Desktop Entry]\nType=Application\nName=Other player\nExec=mpv %F\nMimeType=" + ";".join(types) + ";\n"
    )
    record = root / "launched"
    for binary in ("aizome-utsushi", "mpv"):
        path = binaries / binary
        path.write_text(f'#!{os.environ["runtimeShell"]}\nprintf "%s\\n" "{binary}" "$@" > "$UTSUSHI_LAUNCH_FILE"\n')
        path.chmod(0o755)
    env = dict(os.environ, XDG_CONFIG_HOME=str(config), XDG_CONFIG_DIRS=str(system),
               XDG_DATA_HOME=str(data), XDG_DATA_DIRS=os.environ["mimeData"],
               XDG_CURRENT_DESKTOP="Aizome", DISPLAY=":99", PATH=f"{binaries}:{os.environ['PATH']}",
               UTSUSHI_LAUNCH_FILE=str(record))
    subprocess.run(["update-desktop-database", str(applications)], env=env, check=True)

    def query(kind, environment=env):
        return subprocess.check_output(["xdg-mime", "query", "default", kind], env=environment, text=True).strip()

    for kind in types:
        assert query(kind) == "org.aizome.Utsushi.desktop", kind
        assert query(kind, dict(env, XDG_CURRENT_DESKTOP="OtherDesktop")) == "mpv.desktop", kind
    for source, name, mime in (("videoFile", "film with spaces.mp4", "video/mp4"),
                               ("musicFile", "music with spaces.flac", "audio/flac")):
        media = root / name
        shutil.copyfile(os.environ[source], media)
        subprocess.run(["xdg-open", str(media)], env=env, check=True)
        assert record.read_text().splitlines() == ["aizome-utsushi", str(media)]
        (config / "mimeapps.list").write_text(f"[Default Applications]\n{mime}=mpv.desktop;\n")
        assert query(mime) == "mpv.desktop", "the user's choice takes priority"
        subprocess.run(["xdg-open", str(media)], env=env, check=True)
        assert record.read_text().splitlines() == ["mpv", str(media)]
        (config / "mimeapps.list").unlink()
    print(f"Resolved {len(types)} Utsushi media defaults, other-desktop and user overrides, and video/music dispatch")
