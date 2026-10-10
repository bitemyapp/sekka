-- Tatami for NixOS, following Omarchy 4 "Quattro" (v4.0.4,
-- https://github.com/basecamp/omarchy, MIT; see ../LICENSE): its
-- default/hypr and config/hypr Lua modules, the Tokyo Night theme, and its
-- key bindings. Configuration calls only, besides loading the monitor state
-- the `tatami` helper writes: dynamic behavior lives in the helper, and
-- session components are systemd user units started
-- with this session. To customize, copy this file (including the keyboard
-- section NixOS appends) to ~/.config/tatami/hyprland.lua.

-------------------------------------------------------------------------------
-- Monitors (config/hypr/monitors.lua)
-------------------------------------------------------------------------------

hl.env("GDK_SCALE", "2")
-- The display settings chosen in the Displays window and with Super+/, then
-- this session's monitor state, which the tatami helper writes before each
-- of its reloads and as it turns a display off: of the rules naming a
-- display the last loaded wins, so a display off for the session stays off,
-- and reloading this configuration turns no monitor on or off. A rule naming
-- a display also wins over the rule for every monitor below.
local state_home = os.getenv("XDG_STATE_HOME")
if not state_home or state_home:sub(1, 1) ~= "/" then
  state_home = (os.getenv("HOME") or "") .. "/.local/state"
end
for _, file in ipairs({
  state_home .. "/tatami/displays.lua",
  (os.getenv("XDG_RUNTIME_DIR") or "") .. "/tatami/monitors.lua",
}) do
  local settings = loadfile(file)
  if settings then
    pcall(settings)
  end
end
hl.monitor({ output = "", mode = "preferred", position = "auto", scale = "auto" })

-------------------------------------------------------------------------------
-- Environment (default/hypr/envs.lua). QT_QPA_PLATFORMTHEME is left unset:
-- Hyprland exports it to the systemd user manager, which outlives this session
-- and would hand GTK styling to the Qt applications of other desktops.
-------------------------------------------------------------------------------

-- Omarchy's cursor is Adwaita, Arch's default theme; NixOS has no default, so
-- Hyprland would draw its own. These stay in Hyprland's environment: they do
-- not reach the systemd user manager, so other desktops keep their cursors.
hl.env("XCURSOR_THEME", "Adwaita")
hl.env("XCURSOR_SIZE", "24")
hl.env("HYPRCURSOR_SIZE", "24")
hl.env("GDK_BACKEND", "wayland,x11,*")
hl.env("QT_QPA_PLATFORM", "wayland;xcb")
hl.env("MOZ_ENABLE_WAYLAND", "1")
hl.env("ELECTRON_OZONE_PLATFORM_HINT", "wayland")
hl.env("OZONE_PLATFORM", "wayland")
hl.env("XDG_SESSION_TYPE", "wayland")
-- XDG_CURRENT_DESKTOP stays Hyprland:Tatami, as the session sets it, so
-- other desktops' autostart entries can be kept out of Tatami.
hl.env("TERMINAL", "xdg-terminal-exec")

hl.config({
  xwayland = {
    force_zero_scaling = true,
  },

  ecosystem = {
    no_update_news = true,
    no_donation_nag = true,
  },
})

-------------------------------------------------------------------------------
-- Look and feel (default/hypr/looknfeel.lua with the Tokyo Night theme's
-- hyprland.lua: a solid accent border)
-------------------------------------------------------------------------------

hl.config({
  general = {
    gaps_in = 5,
    gaps_out = 10,
    border_size = 2,

    col = {
      active_border = "rgb(7aa2f7)",
      inactive_border = "rgba(595959aa)",
    },

    resize_on_border = false,
    allow_tearing = false,
    layout = "dwindle",
  },

  decoration = {
    rounding = 0,

    shadow = {
      enabled = false,
    },

    blur = {
      enabled = false,
    },
  },

  group = {
    col = {
      border_active = "rgb(7aa2f7)",
      border_inactive = "rgba(595959aa)",
    },

    groupbar = {
      font_size = 12,
      font_family = "JetBrainsMono Nerd Font",
      font_weight_active = "ultraheavy",
      font_weight_inactive = "normal",
      indicator_height = 1,
      indicator_gap = 5,
      height = 22,
      gaps_in = 5,
      gaps_out = 0,
      text_color = "rgb(ffffff)",
      text_color_inactive = "rgba(ffffff90)",
      col = {
        active = "rgba(00000040)",
        inactive = "rgba(00000020)",
      },
      gradients = true,
      gradient_rounding = 0,
      gradient_round_only_edges = false,
    },
  },

  animations = {
    enabled = true,
  },
})

hl.curve("easeOutQuint", { type = "bezier", points = { { 0.23, 1 }, { 0.32, 1 } } })
hl.curve("easeInOutCubic", { type = "bezier", points = { { 0.65, 0.05 }, { 0.36, 1 } } })
hl.curve("linear", { type = "bezier", points = { { 0, 0 }, { 1, 1 } } })
hl.curve("almostLinear", { type = "bezier", points = { { 0.5, 0.5 }, { 0.75, 1.0 } } })
hl.curve("quick", { type = "bezier", points = { { 0.15, 0 }, { 0.1, 1 } } })

hl.animation({ leaf = "global", enabled = true, speed = 10, bezier = "default" })
hl.animation({ leaf = "border", enabled = true, speed = 5.39, bezier = "easeOutQuint" })
hl.animation({ leaf = "windows", enabled = true, speed = 3.79, bezier = "easeOutQuint" })
hl.animation({ leaf = "windowsIn", enabled = true, speed = 4.1, bezier = "easeOutQuint", style = "popin 87%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 1.49, bezier = "linear", style = "popin 87%" })
hl.animation({ leaf = "fadeIn", enabled = true, speed = 1.73, bezier = "almostLinear" })
hl.animation({ leaf = "fadeOut", enabled = true, speed = 1.46, bezier = "almostLinear" })
hl.animation({ leaf = "fade", enabled = true, speed = 3.03, bezier = "quick" })
hl.animation({ leaf = "fadeSwitch", enabled = false })
hl.animation({ leaf = "layers", enabled = true, speed = 3.81, bezier = "easeOutQuint" })
hl.animation({ leaf = "layersIn", enabled = true, speed = 4, bezier = "easeOutQuint", style = "fade" })
hl.animation({ leaf = "layersOut", enabled = true, speed = 1.5, bezier = "linear", style = "fade" })
hl.animation({ leaf = "fadeLayersIn", enabled = true, speed = 1.79, bezier = "almostLinear" })
hl.animation({ leaf = "fadeLayersOut", enabled = true, speed = 1.39, bezier = "almostLinear" })
hl.animation({ leaf = "workspaces", enabled = false })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 3, bezier = "easeOutQuint", style = "slidevert" })

hl.config({
  dwindle = {
    preserve_split = true,
    force_split = 2,
  },

  scrolling = {
    column_width = 0.49,
  },

  master = {
    new_status = "master",
  },

  misc = {
    disable_hyprland_logo = true,
    disable_splash_rendering = true,
    disable_scale_notification = true,
    focus_on_activate = true,
    anr_missed_pings = 3,
    on_focus_under_fullscreen = 1,
    initial_workspace_tracking = 0,
    allow_session_lock_restore = true,
    background_color = "rgb(1a1b26)",
    -- uwsm sets XDG_CURRENT_DESKTOP=Hyprland:Tatami on purpose.
    disable_xdg_env_checks = true,
  },

  cursor = {
    hide_on_key_press = true,
    warp_on_change_workspace = 1,
  },

  binds = {
    hide_special_on_workspace_change = true,
  },
})

-------------------------------------------------------------------------------
-- Input (default/hypr/input.lua). The keyboard layout of the installed system
-- is appended below by NixOS, with Omarchy's compose key on Caps Lock and
-- both Shifts together for Caps Lock.
-------------------------------------------------------------------------------

hl.config({
  input = {
    kb_model = "",
    kb_rules = "",
    follow_mouse = 1,
    sensitivity = 0,

    repeat_rate = 40,
    repeat_delay = 250,
    numlock_by_default = true,

    touchpad = {
      natural_scroll = false,
      clickfinger_behavior = true,
      scroll_factor = 0.4,
    },
  },

  misc = {
    key_press_enables_dpms = true,
    mouse_move_enables_dpms = true,
  },
})

-- Scroll nicely in the terminal.
hl.window_rule({ match = { class = "(Alacritty|kitty|foot)" }, scroll_touchpad = 1.5 })
hl.window_rule({ match = { class = "com.mitchellh.ghostty" }, scroll_touchpad = 0.2 })

-------------------------------------------------------------------------------
-- Windows (default/hypr/windows.lua and default/hypr/apps/*.lua, in order)
-------------------------------------------------------------------------------

hl.window_rule({ match = { class = ".*" }, suppress_event = "maximize" })

-- Tag all windows for default opacity (apps can opt out with -default-opacity).
hl.window_rule({ match = { class = ".*" }, tag = "+default-opacity" })

-- Fix some dragging issues with XWayland.
hl.window_rule({
  match = { class = "^$", title = "^$", xwayland = true, float = true, fullscreen = false, pin = false },
  no_focus = true,
})

-- 1Password and Bitwarden.
hl.window_rule({ match = { class = "^(1[p|P]assword)$" }, no_screen_share = true, tag = "+floating-window" })
hl.window_rule({ match = { class = "^(Bitwarden)$" }, no_screen_share = true, tag = "+floating-window" })
-- Bitwarden's Chrome extension opens its popout at the size it chooses (480x630
-- by default) and holds it there: under the floating-window tag's 875x600 Chrome
-- painted only the popout's own width, leaving the rest see-through. It floats,
-- centered, at its own size instead.
hl.window_rule({ match = { class = "chrome-nngceckbapebfimnlniiiahkandclblb-Default" }, no_screen_share = true, float = true, center = true })

-- Battle.net under Proton.
hl.window_rule({ match = { class = "^steam_app_battlenet$", title = "^Battle\\.net$" }, float = true, center = true, size = { 1280, 800 } })
hl.window_rule({ match = { class = "^steam_app_battlenet$", title = "^Battle\\.net Setup$" }, decorate = false, no_blur = true, no_shadow = true })

-- Browsers: opaque when focused, a subtle fade when not.
hl.window_rule({ match = { class = "((google-)?[cC]hrom(e|ium)|[bB]rave-browser|[mM]icrosoft-edge|Vivaldi-stable|helium)" }, tag = "+chromium-based-browser" })
hl.window_rule({ match = { class = "([fF]irefox|zen|librewolf)" }, tag = "+firefox-based-browser" })
hl.window_rule({ match = { tag = "chromium-based-browser" }, tag = "-default-opacity", tile = true, opacity = "1.0 0.985" })
hl.window_rule({ match = { tag = "firefox-based-browser" }, tag = "-default-opacity", opacity = "1.0 0.985" })
hl.window_rule({ match = { class = "(^.+-youtube\\.com__.*$|^.+-app\\.zoom\\.us__wc_home.*$)" }, tag = "-chromium-based-browser" })
hl.window_rule({ match = { class = "(^.+-youtube\\.com__.*$|^.+-app\\.zoom\\.us__wc_home.*$)" }, tag = "-default-opacity" })
-- Hide screen sharing notification windows.
hl.window_rule({ match = { title = ".*is sharing.*" }, workspace = "special silent" })

-- DaVinci Resolve stays opaque for colour-critical work.
hl.window_rule({ match = { class = ".*[Rr]esolve.*" }, float = true, stay_focused = true, tag = "-default-opacity", opacity = "1 1" })
hl.window_rule({ match = { class = ".*[Rr]esolve.*", title = "^DaVinci Resolve( Studio)? - .+$" }, fullscreen = true })
hl.window_rule({ match = { class = ".*[Rr]esolve.*", title = "^(DaVinci Resolve( Studio)? - .+|Project Manager)$" }, stay_focused = false })

hl.window_rule({ match = { class = "GeForceNOW" }, idle_inhibit = "fullscreen" })
hl.window_rule({ match = { class = "^(jetbrains-.*)$" }, no_follow_mouse = true })

-- LocalSend and file sharing.
hl.window_rule({ match = { class = "(Share|localsend)" }, float = true, center = true })
hl.window_rule({ match = { class = "localsend" }, size = { 1100, 700 } })
hl.window_rule({ match = { class = "com.moonlight_stream.Moonlight" }, fullscreen = true, idle_inhibit = "fullscreen" })

-- Shell surfaces pop without compositor fades (apps/omarchy-shell.lua).
hl.layer_rule({ match = { namespace = "waybar" }, no_anim = true, animation = "none" })
hl.layer_rule({ match = { namespace = "walker" }, no_anim = true, animation = "none" })

-- Picture-in-picture overlays.
hl.window_rule({ match = { title = "(Picture.?in.?[Pp]icture)" }, tag = "+pip" })
hl.window_rule({
  match = { tag = "pip" },
  tag = "-default-opacity",
  float = true,
  pin = true,
  size = { 600, 338 },
  keep_aspect_ratio = true,
  border_size = 0,
  opacity = "1 1",
  move = { "(monitor_w-window_w-40)", "(monitor_h*0.04)" },
})
-- Google Meet picture-in-picture uses the meeting title.
hl.window_rule({
  match = { tag = "chromium-based-browser", title = "^Meet - .+" },
  tag = "-default-opacity",
  float = true,
  pin = true,
  size = { 600, 338 },
  keep_aspect_ratio = true,
  border_size = 0,
  opacity = "1 1",
  move = { "(monitor_w-window_w-40)", "(monitor_h-window_h-40)" },
})

hl.window_rule({ match = { class = "qemu" }, tag = "-default-opacity", opacity = "1 1" })
hl.window_rule({ match = { class = "com.libretro.RetroArch" }, fullscreen = true, tag = "-default-opacity", opacity = "1 1", idle_inhibit = "fullscreen" })

-- No border around the slurp region selection used by screenshots.
hl.layer_rule({ match = { namespace = "selection" }, no_anim = true, animation = "none" })

-- Steam.
hl.window_rule({ match = { class = "steam" }, float = true, idle_inhibit = "fullscreen" })
hl.window_rule({ match = { class = "steam", title = "Steam" }, center = true, size = { 1100, 700 } })
hl.window_rule({ match = { class = "steam.*" }, tag = "-default-opacity", opacity = "1 1" })
hl.window_rule({ match = { class = "steam", title = "Friends List" }, size = { 460, 800 } })

-- Floating windows (apps/system.lua), including the panels the helper opens
-- as terminal applications (org.tatami.<program>) and the screenshot editor.
hl.window_rule({ match = { tag = "floating-window" }, float = true })
hl.window_rule({ match = { tag = "floating-window" }, center = true })
hl.window_rule({ match = { tag = "floating-window" }, size = { 875, 600 } })
hl.window_rule({
  match = { class = "(org.tatami.btop|org.tatami.terminal|org.tatami.bash|nm-connection-editor|org.codeberg.dnkl.foot|org.gnome.NautilusPreviewer|org.gnome.Evince|com.gabm.satty|Omarchy|About|TUI.float|imv|mpv)" },
  tag = "+floating-window",
})
hl.window_rule({ match = { class = "xdg-desktop-portal-gtk" }, tag = "+floating-window" })
-- The Displays window (tatami-displays), floating with room for the
-- arrangement: its own size, as the floating-window tag's would win over it.
hl.window_rule({ match = { class = "org.tatami.Displays" }, float = true, center = true, size = { 880, 800 } })
hl.window_rule({
  match = {
    class = "(sublime_text|DesktopEditors|org.gnome.Nautilus)",
    title = "^(Open.*Files?|Open [F|f]older.*|Save.*Files?|Save.*As|Save|All Files|.*wants to [open|save].*|[C|c]hoose.*)",
  },
  tag = "+floating-window",
})

-- Audio and Bluetooth (wiremix, bluetui) open where Omarchy 4's audio and
-- Bluetooth panels drop from the bar: 6px from the right edge, under it.
-- window_w is still the terminal's own width when the move applies, so the
-- move uses the fixed width.
hl.window_rule({
  match = { class = "(org.tatami.wiremix|org.tatami.bluetui)" },
  float = true,
  size = { 540, 360 },
  move = { "(monitor_w-546)", "32" },
})

-- Authentication prompts (hyprpolkitagent) dim the screen around them and
-- keep focus, as Omarchy's prompt does.
hl.window_rule({
  match = { title = "^Hyprland Polkit Agent$" },
  float = true,
  center = true,
  pin = true,
  stay_focused = true,
  dim_around = true,
  tag = "-default-opacity",
  opacity = "1 1",
})

-- The About window needs more columns than the standard float provides.
hl.window_rule({ match = { class = "org.tatami.fastfetch" }, float = true })
hl.window_rule({ match = { class = "org.tatami.fastfetch" }, center = true })
hl.window_rule({ match = { class = "org.tatami.fastfetch" }, size = { 920, 480 } })

-- Calculator (omacalc in Omarchy).
hl.window_rule({ match = { class = "org.gnome.Calculator" }, float = true })

-- No transparency on media windows.
hl.window_rule({
  match = { class = "^(zoom|vlc|mpv|org.kde.kdenlive|com.obsproject.Studio|com.github.PintaProject.Pinta|imv|org.gnome.NautilusPreviewer)$" },
  tag = "-default-opacity",
})
hl.window_rule({
  match = { class = "^(zoom|vlc|mpv|org.kde.kdenlive|com.obsproject.Studio|com.github.PintaProject.Pinta|imv|org.gnome.NautilusPreviewer)$" },
  opacity = "1 1",
})

-- Popped window rounding.
hl.window_rule({ match = { tag = "pop" }, rounding = 8 })

-- Prevent idle while open.
hl.window_rule({ match = { tag = "noidle" }, idle_inhibit = "always" })

-- Telegram must not steal focus on new messages.
hl.window_rule({ match = { class = "org.telegram.desktop" }, focus_on_activate = false })

-- Terminals, including the helper's terminal applications.
hl.window_rule({
  match = { class = "(Alacritty|kitty|com.mitchellh.ghostty|foot|org\\.codeberg\\.dnkl\\.foot|wezterm|org\\.tatami\\..*|TUI\\..*)" },
  tag = "+terminal",
})

-- Default opacity for everything that did not opt out above.
hl.window_rule({ match = { tag = "default-opacity" }, opacity = "0.985 0.96" })

-------------------------------------------------------------------------------
-- Application bindings (default/hypr/bindings/applications.lua). Omarchy
-- binds its preinstalled applications and web apps only while they are
-- installed; none are on this system, as after Omarchy's "remove preinstalls".
-------------------------------------------------------------------------------

hl.bind("SUPER + RETURN", hl.dsp.exec_cmd("tatami terminal"), { description = "Terminal" })
hl.bind("SUPER + SHIFT + RETURN", hl.dsp.exec_cmd("tatami browser"), { description = "Browser" })
hl.bind("SUPER + SHIFT + F", hl.dsp.exec_cmd("tatami files"), { description = "File manager" })
hl.bind("SUPER + ALT + SHIFT + F", hl.dsp.exec_cmd("tatami files --cwd"), { description = "File manager (cwd)" })
hl.bind("SUPER + SHIFT + B", hl.dsp.exec_cmd("tatami browser"), { description = "Browser" })
hl.bind("SUPER + SHIFT + ALT + B", hl.dsp.exec_cmd("tatami browser --private"), { description = "Browser (private)" })
hl.bind("SUPER + SHIFT + N", hl.dsp.exec_cmd("tatami editor"), { description = "Editor" })

-------------------------------------------------------------------------------
-- Media keys (default/hypr/bindings/media.lua)
-------------------------------------------------------------------------------

hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("tatami volume raise"), { locked = true, repeating = true, description = "Volume up" })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("tatami volume lower"), { locked = true, repeating = true, description = "Volume down" })
hl.bind("XF86AudioMute", hl.dsp.exec_cmd("tatami volume mute-toggle"), { locked = true, description = "Mute" })
hl.bind("XF86AudioMicMute", hl.dsp.exec_cmd("tatami mic-mute"), { locked = true, description = "Mute microphone" })
hl.bind("XF86MonBrightnessUp", hl.dsp.exec_cmd("tatami brightness +5%"), { locked = true, repeating = true, description = "Brightness up" })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd("tatami brightness 5%-"), { locked = true, repeating = true, description = "Brightness down" })
hl.bind("SHIFT + XF86MonBrightnessUp", hl.dsp.exec_cmd("tatami brightness 100%"), { locked = true, repeating = true, description = "Brightness maximum" })
hl.bind("SHIFT + XF86MonBrightnessDown", hl.dsp.exec_cmd("tatami brightness 1%"), { locked = true, repeating = true, description = "Brightness minimum" })
hl.bind("XF86KbdBrightnessUp", hl.dsp.exec_cmd("tatami keyboard-brightness up"), { locked = true, repeating = true, description = "Keyboard brightness up" })
hl.bind("XF86KbdBrightnessDown", hl.dsp.exec_cmd("tatami keyboard-brightness down"), { locked = true, repeating = true, description = "Keyboard brightness down" })
hl.bind("XF86KbdLightOnOff", hl.dsp.exec_cmd("tatami keyboard-brightness cycle"), { locked = true, description = "Keyboard backlight cycle" })
hl.bind("XF86TouchpadToggle", hl.dsp.exec_cmd("tatami touchpad toggle"), { locked = true, description = "Toggle touchpad" })
hl.bind("XF86TouchpadOn", hl.dsp.exec_cmd("tatami touchpad on"), { locked = true, description = "Enable touchpad" })
hl.bind("XF86TouchpadOff", hl.dsp.exec_cmd("tatami touchpad off"), { locked = true, description = "Disable touchpad" })

hl.bind("ALT + XF86AudioRaiseVolume", hl.dsp.exec_cmd("tatami volume +1"), { locked = true, repeating = true, description = "Volume up precise" })
hl.bind("ALT + XF86AudioLowerVolume", hl.dsp.exec_cmd("tatami volume -1"), { locked = true, repeating = true, description = "Volume down precise" })
hl.bind("ALT + XF86MonBrightnessUp", hl.dsp.exec_cmd("tatami brightness +1%"), { locked = true, repeating = true, description = "Brightness up precise" })
hl.bind("ALT + XF86MonBrightnessDown", hl.dsp.exec_cmd("tatami brightness 1%-"), { locked = true, repeating = true, description = "Brightness down precise" })

hl.bind("XF86AudioNext", hl.dsp.exec_cmd("tatami media next"), { locked = true, description = "Next track" })
hl.bind("ALT + XF86AudioPlay", hl.dsp.exec_cmd("tatami media next"), { locked = true, description = "Next track" })
hl.bind("XF86AudioPause", hl.dsp.exec_cmd("tatami media play-pause"), { locked = true, description = "Pause" })
hl.bind("XF86AudioPlay", hl.dsp.exec_cmd("tatami media play-pause"), { locked = true, description = "Play" })
hl.bind("XF86AudioPrev", hl.dsp.exec_cmd("tatami media previous"), { locked = true, description = "Previous track" })
hl.bind("ALT + SHIFT + XF86AudioPlay", hl.dsp.exec_cmd("tatami media previous"), { locked = true, description = "Previous track" })
hl.bind("XF86Eject", hl.dsp.exec_cmd("eject"), { locked = true, description = "Eject media" })

-------------------------------------------------------------------------------
-- Clipboard (default/hypr/bindings/clipboard.lua)
-------------------------------------------------------------------------------

hl.bind("SUPER + C", hl.dsp.exec_cmd("tatami clipboard copy"), { description = "Universal copy" })
hl.bind("SUPER + V", hl.dsp.exec_cmd("tatami clipboard paste"), { description = "Universal paste" })
hl.bind("SUPER + X", hl.dsp.exec_cmd("tatami clipboard cut"), { description = "Universal cut" })
hl.bind("SUPER + CTRL + V", hl.dsp.exec_cmd("tatami clipboard history"), { description = "Clipboard manager" })

-------------------------------------------------------------------------------
-- Tiling (default/hypr/bindings/tiling.lua)
-------------------------------------------------------------------------------

hl.bind("SUPER + W", hl.dsp.window.close(), { description = "Close window" })
hl.bind("CTRL + ALT + DELETE", hl.dsp.exec_cmd("tatami window close-all"), { description = "Close all windows" })

hl.bind("SUPER + J", hl.dsp.layout("togglesplit"), { description = "Toggle window split" })
hl.bind("SUPER + P", hl.dsp.window.pseudo(), { description = "Pseudo window" })
hl.bind("SUPER + T", hl.dsp.window.float({ action = "toggle" }), { description = "Toggle window floating/tiling" })
hl.bind("SUPER + F", hl.dsp.window.fullscreen({ mode = "fullscreen" }), { description = "Full screen" })
hl.bind("SUPER + CTRL + F", hl.dsp.exec_cmd("tatami window tiled-fullscreen"), { description = "Tiled full screen" })
hl.bind("SUPER + ALT + F", hl.dsp.window.fullscreen({ mode = "maximized" }), { description = "Full width" })
hl.bind("SUPER + O", hl.dsp.exec_cmd("tatami window pop"), { description = "Pop window out (float & pin)" })
hl.bind("SUPER + L", hl.dsp.exec_cmd("tatami toggle layout"), { description = "Toggle workspace layout" })

hl.bind("SUPER + LEFT", hl.dsp.focus({ direction = "l" }), { description = "Focus on left window" })
hl.bind("SUPER + RIGHT", hl.dsp.focus({ direction = "r" }), { description = "Focus on right window" })
hl.bind("SUPER + UP", hl.dsp.focus({ direction = "u" }), { description = "Focus on above window" })
hl.bind("SUPER + DOWN", hl.dsp.focus({ direction = "d" }), { description = "Focus on below window" })

hl.bind("SUPER + code:10", hl.dsp.focus({ workspace = "1" }), { description = "Switch to workspace 1" })
hl.bind("SUPER + code:11", hl.dsp.focus({ workspace = "2" }), { description = "Switch to workspace 2" })
hl.bind("SUPER + code:12", hl.dsp.focus({ workspace = "3" }), { description = "Switch to workspace 3" })
hl.bind("SUPER + code:13", hl.dsp.focus({ workspace = "4" }), { description = "Switch to workspace 4" })
hl.bind("SUPER + code:14", hl.dsp.focus({ workspace = "5" }), { description = "Switch to workspace 5" })
hl.bind("SUPER + code:15", hl.dsp.focus({ workspace = "6" }), { description = "Switch to workspace 6" })
hl.bind("SUPER + code:16", hl.dsp.focus({ workspace = "7" }), { description = "Switch to workspace 7" })
hl.bind("SUPER + code:17", hl.dsp.focus({ workspace = "8" }), { description = "Switch to workspace 8" })
hl.bind("SUPER + code:18", hl.dsp.focus({ workspace = "9" }), { description = "Switch to workspace 9" })
hl.bind("SUPER + code:19", hl.dsp.focus({ workspace = "10" }), { description = "Switch to workspace 10" })

hl.bind("SUPER + SHIFT + code:10", hl.dsp.window.move({ workspace = "1" }), { description = "Move window to workspace 1" })
hl.bind("SUPER + SHIFT + code:11", hl.dsp.window.move({ workspace = "2" }), { description = "Move window to workspace 2" })
hl.bind("SUPER + SHIFT + code:12", hl.dsp.window.move({ workspace = "3" }), { description = "Move window to workspace 3" })
hl.bind("SUPER + SHIFT + code:13", hl.dsp.window.move({ workspace = "4" }), { description = "Move window to workspace 4" })
hl.bind("SUPER + SHIFT + code:14", hl.dsp.window.move({ workspace = "5" }), { description = "Move window to workspace 5" })
hl.bind("SUPER + SHIFT + code:15", hl.dsp.window.move({ workspace = "6" }), { description = "Move window to workspace 6" })
hl.bind("SUPER + SHIFT + code:16", hl.dsp.window.move({ workspace = "7" }), { description = "Move window to workspace 7" })
hl.bind("SUPER + SHIFT + code:17", hl.dsp.window.move({ workspace = "8" }), { description = "Move window to workspace 8" })
hl.bind("SUPER + SHIFT + code:18", hl.dsp.window.move({ workspace = "9" }), { description = "Move window to workspace 9" })
hl.bind("SUPER + SHIFT + code:19", hl.dsp.window.move({ workspace = "10" }), { description = "Move window to workspace 10" })

hl.bind("SUPER + SHIFT + ALT + code:10", hl.dsp.window.move({ workspace = "1", follow = false }), { description = "Move window silently to workspace 1" })
hl.bind("SUPER + SHIFT + ALT + code:11", hl.dsp.window.move({ workspace = "2", follow = false }), { description = "Move window silently to workspace 2" })
hl.bind("SUPER + SHIFT + ALT + code:12", hl.dsp.window.move({ workspace = "3", follow = false }), { description = "Move window silently to workspace 3" })
hl.bind("SUPER + SHIFT + ALT + code:13", hl.dsp.window.move({ workspace = "4", follow = false }), { description = "Move window silently to workspace 4" })
hl.bind("SUPER + SHIFT + ALT + code:14", hl.dsp.window.move({ workspace = "5", follow = false }), { description = "Move window silently to workspace 5" })
hl.bind("SUPER + SHIFT + ALT + code:15", hl.dsp.window.move({ workspace = "6", follow = false }), { description = "Move window silently to workspace 6" })
hl.bind("SUPER + SHIFT + ALT + code:16", hl.dsp.window.move({ workspace = "7", follow = false }), { description = "Move window silently to workspace 7" })
hl.bind("SUPER + SHIFT + ALT + code:17", hl.dsp.window.move({ workspace = "8", follow = false }), { description = "Move window silently to workspace 8" })
hl.bind("SUPER + SHIFT + ALT + code:18", hl.dsp.window.move({ workspace = "9", follow = false }), { description = "Move window silently to workspace 9" })
hl.bind("SUPER + SHIFT + ALT + code:19", hl.dsp.window.move({ workspace = "10", follow = false }), { description = "Move window silently to workspace 10" })

hl.bind("SUPER + S", hl.dsp.workspace.toggle_special("scratchpad"), { description = "Toggle scratchpad" })
hl.bind("SUPER + ALT + S", hl.dsp.window.move({ workspace = "special:scratchpad", follow = false }), { description = "Move window to scratchpad" })

hl.bind("SUPER + TAB", hl.dsp.focus({ workspace = "e+1" }), { description = "Next workspace" })
hl.bind("SUPER + SHIFT + TAB", hl.dsp.focus({ workspace = "e-1" }), { description = "Previous workspace" })
hl.bind("SUPER + CTRL + TAB", hl.dsp.focus({ workspace = "previous" }), { description = "Former workspace" })

hl.bind("SUPER + SHIFT + ALT + LEFT", hl.dsp.workspace.move({ monitor = "l" }), { description = "Move workspace to left monitor" })
hl.bind("SUPER + SHIFT + ALT + RIGHT", hl.dsp.workspace.move({ monitor = "r" }), { description = "Move workspace to right monitor" })
hl.bind("SUPER + SHIFT + ALT + UP", hl.dsp.workspace.move({ monitor = "u" }), { description = "Move workspace to up monitor" })
hl.bind("SUPER + SHIFT + ALT + DOWN", hl.dsp.workspace.move({ monitor = "d" }), { description = "Move workspace to down monitor" })

hl.bind("SUPER + SHIFT + LEFT", hl.dsp.window.swap({ direction = "l" }), { description = "Swap window to the left" })
hl.bind("SUPER + SHIFT + RIGHT", hl.dsp.window.swap({ direction = "r" }), { description = "Swap window to the right" })
hl.bind("SUPER + SHIFT + UP", hl.dsp.window.swap({ direction = "u" }), { description = "Swap window up" })
hl.bind("SUPER + SHIFT + DOWN", hl.dsp.window.swap({ direction = "d" }), { description = "Swap window down" })

hl.bind("ALT + TAB", hl.dsp.window.cycle_next(), { description = "Focus on next window" })
hl.bind("ALT + SHIFT + TAB", hl.dsp.window.cycle_next({ next = false }), { description = "Focus on previous window" })
hl.bind("ALT + TAB", hl.dsp.window.bring_to_top(), { description = "Reveal active window on top" })
hl.bind("ALT + SHIFT + TAB", hl.dsp.window.bring_to_top(), { description = "Reveal active window on top" })

hl.bind("CTRL + ALT + TAB", hl.dsp.focus({ monitor = "+1" }), { description = "Focus on next monitor" })
hl.bind("CTRL + ALT + SHIFT + TAB", hl.dsp.focus({ monitor = "-1" }), { description = "Focus on previous monitor" })

hl.bind("SUPER + code:20", hl.dsp.window.resize({ x = -100, y = 0, relative = true }), { description = "Expand window left" })
hl.bind("SUPER + code:21", hl.dsp.window.resize({ x = 100, y = 0, relative = true }), { description = "Shrink window left" })
hl.bind("SUPER + SHIFT + code:20", hl.dsp.window.resize({ x = 0, y = -100, relative = true }), { description = "Shrink window up" })
hl.bind("SUPER + SHIFT + code:21", hl.dsp.window.resize({ x = 0, y = 100, relative = true }), { description = "Expand window down" })

hl.bind("SUPER + ALT + code:20", hl.dsp.window.resize({ x = -25, y = 0, relative = true }), { description = "Expand window left a little" })
hl.bind("SUPER + ALT + code:21", hl.dsp.window.resize({ x = 25, y = 0, relative = true }), { description = "Shrink window left a little" })
hl.bind("SUPER + SHIFT + ALT + code:20", hl.dsp.window.resize({ x = 0, y = -25, relative = true }), { description = "Shrink window up a little" })
hl.bind("SUPER + SHIFT + ALT + code:21", hl.dsp.window.resize({ x = 0, y = 25, relative = true }), { description = "Expand window down a little" })

hl.bind("SUPER + CTRL + code:20", hl.dsp.window.resize({ x = -300, y = 0, relative = true }), { description = "Expand window left a lot" })
hl.bind("SUPER + CTRL + code:21", hl.dsp.window.resize({ x = 300, y = 0, relative = true }), { description = "Shrink window left a lot" })
hl.bind("SUPER + CTRL + SHIFT + code:20", hl.dsp.window.resize({ x = 0, y = -300, relative = true }), { description = "Shrink window up a lot" })
hl.bind("SUPER + CTRL + SHIFT + code:21", hl.dsp.window.resize({ x = 0, y = 300, relative = true }), { description = "Expand window down a lot" })

hl.bind("SUPER + mouse_down", hl.dsp.focus({ workspace = "e+1" }), { description = "Scroll active workspace forward" })
hl.bind("SUPER + mouse_up", hl.dsp.focus({ workspace = "e-1" }), { description = "Scroll active workspace backward" })

hl.bind("SUPER + mouse:272", hl.dsp.window.drag(), { mouse = true, description = "Move window" })
hl.bind("SUPER + mouse:273", hl.dsp.window.resize(), { mouse = true, description = "Resize window" })

hl.bind("SUPER + G", hl.dsp.group.toggle(), { description = "Toggle window grouping" })
hl.bind("SUPER + ALT + G", hl.dsp.window.move({ out_of_group = true }), { description = "Move active window out of group" })

hl.bind("SUPER + ALT + LEFT", hl.dsp.window.move({ into_group = "l" }), { description = "Move window to group on left" })
hl.bind("SUPER + ALT + RIGHT", hl.dsp.window.move({ into_group = "r" }), { description = "Move window to group on right" })
hl.bind("SUPER + ALT + UP", hl.dsp.window.move({ into_group = "u" }), { description = "Move window to group on top" })
hl.bind("SUPER + ALT + DOWN", hl.dsp.window.move({ into_group = "d" }), { description = "Move window to group on bottom" })

hl.bind("SUPER + ALT + TAB", hl.dsp.group.next(), { description = "Next window in group" })
hl.bind("SUPER + ALT + SHIFT + TAB", hl.dsp.group.prev(), { description = "Previous window in group" })

hl.bind("SUPER + CTRL + LEFT", hl.dsp.group.prev(), { description = "Move grouped window focus left" })
hl.bind("SUPER + CTRL + RIGHT", hl.dsp.group.next(), { description = "Move grouped window focus right" })

hl.bind("SUPER + ALT + mouse_down", hl.dsp.group.next(), { description = "Next window in group" })
hl.bind("SUPER + ALT + mouse_up", hl.dsp.group.prev(), { description = "Previous window in group" })

hl.bind("SUPER + ALT + code:10", hl.dsp.group.active({ index = 1 }), { description = "Switch to group window 1" })
hl.bind("SUPER + ALT + code:11", hl.dsp.group.active({ index = 2 }), { description = "Switch to group window 2" })
hl.bind("SUPER + ALT + code:12", hl.dsp.group.active({ index = 3 }), { description = "Switch to group window 3" })
hl.bind("SUPER + ALT + code:13", hl.dsp.group.active({ index = 4 }), { description = "Switch to group window 4" })
hl.bind("SUPER + ALT + code:14", hl.dsp.group.active({ index = 5 }), { description = "Switch to group window 5" })

hl.bind("SUPER + slash", hl.dsp.exec_cmd("tatami scale up"), { description = "Monitor scaling up" })
hl.bind("SUPER + ALT + slash", hl.dsp.exec_cmd("tatami scale down"), { description = "Monitor scaling down" })

-------------------------------------------------------------------------------
-- Utilities (default/hypr/bindings/utilities.lua)
-------------------------------------------------------------------------------

-- Menus
hl.bind("SUPER + SPACE", hl.dsp.exec_cmd("tatami menu"), { description = "Tatami menu" })
hl.bind("SUPER + ALT + SPACE", hl.dsp.exec_cmd("tatami apps"), { description = "Apps menu" })
hl.bind("SUPER + CTRL + E", hl.dsp.exec_cmd("tatami emoji"), { description = "Emojis" })
hl.bind("SUPER + CTRL + C", hl.dsp.exec_cmd("tatami menu capture"), { description = "Capture menu" })
hl.bind("SUPER + CTRL + O", hl.dsp.exec_cmd("tatami menu toggle"), { description = "Toggle menu" })
hl.bind("SUPER + CTRL + H", hl.dsp.exec_cmd("tatami menu hardware"), { description = "Hardware menu" })
hl.bind("SUPER + SHIFT + code:201", hl.dsp.exec_cmd("tatami menu"), { description = "Tatami menu" })
hl.bind("SUPER + ESCAPE", hl.dsp.exec_cmd("tatami menu system"), { description = "System menu" })
hl.bind("XF86PowerOff", hl.dsp.exec_cmd("tatami menu system"), { locked = true, description = "Power menu" })
hl.bind("SUPER + K", hl.dsp.exec_cmd("tatami keybindings"), { description = "Keybindings" })
hl.bind("SUPER + CTRL + SPACE", hl.dsp.exec_cmd("tatami background"), { description = "Background switcher" })
hl.bind("SUPER + CTRL + Q", hl.dsp.exec_cmd("tatami launch gnome-calculator"), { description = "Calculator" })
hl.bind("XF86Calculator", hl.dsp.exec_cmd("tatami launch gnome-calculator"), { description = "Calculator" })

-- Aesthetics
hl.bind("SUPER + SHIFT + SPACE", hl.dsp.exec_cmd("tatami toggle bar"), { description = "Toggle top bar" })
hl.bind("SUPER + BACKSPACE", hl.dsp.exec_cmd("tatami window transparency"), { description = "Toggle window transparency" })
hl.bind("SUPER + SHIFT + BACKSPACE", hl.dsp.exec_cmd("tatami toggle gaps"), { description = "Toggle window gaps" })
hl.bind("SUPER + CTRL + BACKSPACE", hl.dsp.exec_cmd("tatami toggle aspect"), { description = "Toggle single-window square aspect" })

-- Notifications. xkbcommon names the comma keysym "comma"; "COMMA" does not match.
hl.bind("SUPER + comma", hl.dsp.exec_cmd("makoctl dismiss"), { description = "Dismiss last notification" })
hl.bind("SUPER + SHIFT + comma", hl.dsp.exec_cmd("makoctl dismiss --all"), { description = "Dismiss all notifications" })
hl.bind("SUPER + CTRL + comma", hl.dsp.exec_cmd("tatami toggle notifications"), { description = "Toggle silencing notifications" })
hl.bind("SUPER + ALT + comma", hl.dsp.exec_cmd("makoctl invoke"), { description = "Invoke last notification" })
hl.bind("SUPER + SHIFT + ALT + comma", hl.dsp.exec_cmd("makoctl restore"), { description = "Restore last notification" })

-- Toggles
hl.bind("SUPER + CTRL + I", hl.dsp.exec_cmd("tatami toggle idle"), { description = "Toggle locking on idle" })
hl.bind("SUPER + CTRL + N", hl.dsp.exec_cmd("tatami toggle nightlight"), { description = "Toggle nightlight" })
hl.bind("SUPER + CTRL + Delete", hl.dsp.exec_cmd("tatami laptop-display"), { description = "Toggle laptop display" })

-- Captures
hl.bind("PRINT", hl.dsp.exec_cmd("tatami screenshot"), { description = "Screenshot" })
hl.bind("ALT + PRINT", hl.dsp.exec_cmd("tatami screenrecord --menu"), { description = "Screenrecording" })
hl.bind("SUPER + PRINT", hl.dsp.exec_cmd("tatami color-picker"), { description = "Color picker" })
hl.bind("SUPER + CTRL + PRINT", hl.dsp.exec_cmd("tatami capture text"), { description = "Extract text (OCR) from screenshot" })

-- Notifications on demand
hl.bind("SUPER + CTRL + ALT + T", hl.dsp.exec_cmd("tatami notify time"), { description = "Show time" })
hl.bind("SUPER + CTRL + ALT + B", hl.dsp.exec_cmd("tatami notify battery"), { description = "Show battery remaining" })

-- Control panels: Omarchy's bar panels, as terminal applications and menus.
hl.bind("SUPER + CTRL + A", hl.dsp.exec_cmd("tatami audio"), { description = "Audio" })
hl.bind("SUPER + CTRL + B", hl.dsp.exec_cmd("tatami bluetooth"), { description = "Bluetooth" })
hl.bind("SUPER + CTRL + D", hl.dsp.exec_cmd("tatami display"), { description = "Display settings" })
hl.bind("SUPER + CTRL + W", hl.dsp.exec_cmd("tatami network"), { description = "Wi-Fi" })
hl.bind("SUPER + CTRL + P", hl.dsp.exec_cmd("tatami power-profile"), { description = "Power" })
hl.bind("SUPER + CTRL + T", hl.dsp.exec_cmd("tatami tui btop"), { description = "Activity" })

-- Cursor zoom
hl.bind("SUPER + CTRL + Z", hl.dsp.exec_cmd("tatami zoom in"), { description = "Zoom in" })
hl.bind("SUPER + CTRL + ALT + Z", hl.dsp.exec_cmd("tatami zoom reset"), { description = "Reset zoom" })

hl.bind("SUPER + CTRL + L", hl.dsp.exec_cmd("tatami lock"), { description = "Lock system" })
