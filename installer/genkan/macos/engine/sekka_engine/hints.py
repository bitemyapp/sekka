# SPDX-License-Identifier: GPL-3.0-or-later
"""What macOS was set to, offered to Sekka's first-boot setup as hints.

Each value is mapped to what Linux uses (a glibc locale, an XKB layout, an
IANA zone) or left out when there is no faithful mapping: setup asks the
person anyway, and a wrong guess is worse than none. The owner's own
preferences are read from their home folder, since the engine runs as root.
"""

import logging
import plistlib
import re

from . import contract as c

log = logging.getLogger("sekka_engine.hints")

# macOS keyboard layouts (com.apple.keylayout.NAME) whose XKB layout is the
# same without a variant. Layouts that need a variant (Dvorak, Colemak,
# Czech QWERTY, ...) are deliberately absent.
KEYBOARD_LAYOUTS = {
    "US": "us",
    "ABC": "us",
    "USInternational-PC": "us",
    "British": "gb",
    "British-PC": "gb",
    "Irish": "ie",
    "Australian": "au",
    "Canadian-CSA": "ca",
    "German": "de",
    "Austrian": "at",
    "SwissGerman": "ch",
    "French": "fr",
    "French-PC": "fr",
    "Belgian": "be",
    "Spanish": "es",
    "Spanish-ISO": "es",
    "Italian": "it",
    "Italian-Pro": "it",
    "Portuguese": "pt",
    "Brazilian": "br",
    "Brazilian-ABNT2": "br",
    "Dutch": "nl",
    "Danish": "dk",
    "Finnish": "fi",
    "Norwegian": "no",
    "Swedish": "se",
    "Swedish-Pro": "se",
    "Polish": "pl",
    "PolishPro": "pl",
    "Czech": "cz",
    "Hungarian": "hu",
    "Russian": "ru",
    "RussianWin": "ru",
    "Ukrainian": "ua",
    "Greek": "gr",
    "Hebrew": "il",
    "Turkish": "tr",
    "Turkish-QWERTY-PC": "tr",
    "Icelandic": "is",
    "Slovenian": "si",
    "Croatian": "hr",
    "Estonian": "ee",
    "Latvian": "lv",
    "Lithuanian": "lt",
    "Romanian": "ro",
    "Slovak": "sk",
}

_LOCALE = re.compile(r"([a-z]{2,3})(?:-[A-Za-z]{4})?_([A-Z]{2})(?:@.*)?\Z")
_ZONE = re.compile(r"[A-Za-z][A-Za-z0-9_+-]*(?:/[A-Za-z0-9_+-]+){0,2}\Z")


def glibc_locale(apple_locale):
    """`en_US` (or `en_US@rg=gbzzzz`, `zh-Hans_CN`) to `en_US.UTF-8`."""
    if not isinstance(apple_locale, str):
        return None
    match = _LOCALE.fullmatch(apple_locale)
    if not match:
        return None
    return f"{match.group(1)}_{match.group(2)}.UTF-8"


def xkb_layout(input_source_id):
    """`com.apple.keylayout.German` to `de`."""
    prefix = "com.apple.keylayout."
    if not isinstance(input_source_id, str) or not input_source_id.startswith(prefix):
        return None
    return KEYBOARD_LAYOUTS.get(input_source_id[len(prefix) :])


def iana_zone(localtime_target):
    """/etc/localtime's target (`/var/db/timezone/zoneinfo/America/Chicago`)
    to `America/Chicago`."""
    if not isinstance(localtime_target, str) or "zoneinfo/" not in localtime_target:
        return None
    zone = localtime_target.split("zoneinfo/", 1)[1]
    return zone if _ZONE.fullmatch(zone) and ".." not in zone else None


def _plist(host, path):
    try:
        return plistlib.loads(host.read_file(path))
    except Exception:
        return None


def _keyboard_source(prefs):
    if not isinstance(prefs, dict):
        return None
    current = prefs.get("AppleCurrentKeyboardLayoutInputSourceID")
    if isinstance(current, str):
        return current
    for key in ("AppleSelectedInputSources", "AppleEnabledInputSources"):
        for source in prefs.get(key) or []:
            if isinstance(source, dict) and source.get("InputSourceKind") == "Keyboard Layout":
                name = source.get("KeyboardLayout Name")
                if isinstance(name, str):
                    return "com.apple.keylayout." + name.replace(" ", "")
    return None


def _clean(value, limit=255):
    if not isinstance(value, str):
        return None
    value = c.plain_text(value, limit=limit, keep="start")
    return value or None


def read_hints(host, owner, full_name):
    """The handoff's SetupHints for the owner `owner`."""
    home = f"/Users/{owner}"
    user_global = _plist(host, f"{home}/Library/Preferences/.GlobalPreferences.plist")
    system_global = _plist(host, "/Library/Preferences/.GlobalPreferences.plist")
    locale = None
    for prefs in (user_global, system_global):
        if isinstance(prefs, dict) and locale is None:
            locale = glibc_locale(prefs.get("AppleLocale"))
    keyboard = xkb_layout(_keyboard_source(_plist(host, f"{home}/Library/Preferences/com.apple.HIToolbox.plist")))
    if keyboard is None:
        keyboard = xkb_layout(_keyboard_source(_plist(host, "/Library/Preferences/com.apple.HIToolbox.plist")))
    try:
        timezone = iana_zone(host.readlink("/etc/localtime"))
    except OSError:
        timezone = None
    computer_name = None
    result = host.run(["scutil", "--get", "ComputerName"], timeout=30)
    if result.returncode == 0:
        computer_name = _clean((result.stdout or b"").decode("utf-8", "replace"))
    hints = {
        "locale": locale,
        "keyboard": keyboard,
        "timezone": timezone,
        "computer_name": computer_name,
        "full_name": _clean(full_name),
    }
    log.info("setup hints: %s", {k: v is not None for k, v in hints.items()})
    return hints
