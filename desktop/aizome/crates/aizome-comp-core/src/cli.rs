//! Command-line options.

use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// DRM/KMS on a TTY, through libseat.
    Udev,
    /// Nested in another compositor's window, for development.
    Winit,
    /// Offscreen outputs only: tests, CI and measurements.
    Headless,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RendererKind {
    Gles,
    Pixman,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeadlessOutput {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub backend: BackendKind,
    /// `None` picks GLES and falls back to pixman.
    pub renderer: Option<RendererKind>,
    pub headless_outputs: Vec<HeadlessOutput>,
    /// The Wayland socket name; picked automatically when absent.
    pub socket: Option<String>,
    /// Start no shell (aizome-shell).
    pub no_shell: bool,
    /// Start no X11 bridge (xwayland-satellite).
    pub no_xwayland: bool,
    /// Don't talk to systemd or D-Bus (tests, nested sessions).
    pub no_session: bool,
    /// Settings file to read instead of ~/.config/aizome/settings.toml.
    pub config: Option<PathBuf>,
    /// Program to run once the compositor is up (with its environment).
    pub command: Vec<String>,
    /// The logic library to load (`--logic`, else `$AIZOME_LOGIC`, else
    /// the one installed beside the binary).
    pub logic: Option<PathBuf>,
}

pub const USAGE: &str = "\
usage: aizome [options] [-- command args...]

  --backend udev|winit|headless   where to show (default: udev on a TTY,
                                  winit inside another Wayland or X session)
  --renderer gles|pixman          how to draw (default: gles, falling back to
                                  pixman); also AIZOME_RENDERER
  --headless-output WxH[@S]       a headless output (repeatable; default
                                  2880x1800@2)
  --socket NAME                   Wayland socket name (default: wayland-N)
  --config PATH                   settings file (default
                                  ~/.config/aizome/settings.toml)
  --no-shell                      don't start aizome-shell
  --no-xwayland                   don't start xwayland-satellite
  --no-session                    don't import the environment into systemd
                                  and D-Bus or notify readiness
  --logic PATH                    the compositor logic library to load
                                  (default: $AIZOME_LOGIC, else the one
                                  installed beside the binary)
  -- command...                   run a program once the compositor is ready
";

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut o = Options {
        backend: default_backend(),
        renderer: None,
        headless_outputs: Vec::new(),
        socket: None,
        no_shell: false,
        no_xwayland: false,
        no_session: false,
        config: None,
        command: Vec::new(),
        logic: None,
    };
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match a.as_str() {
            "--backend" => {
                o.backend = match value("--backend")?.as_str() {
                    "udev" | "tty" | "drm" => BackendKind::Udev,
                    "winit" | "nested" => BackendKind::Winit,
                    "headless" => BackendKind::Headless,
                    b => return Err(format!("unknown backend {b}")),
                }
            }
            "--renderer" => o.renderer = Some(parse_renderer(&value("--renderer")?)?),
            "--headless-output" => o.headless_outputs.push(parse_output(&value("--headless-output")?)?),
            "--socket" => o.socket = Some(value("--socket")?),
            "--config" => o.config = Some(PathBuf::from(value("--config")?)),
            "--logic" => o.logic = Some(PathBuf::from(value("--logic")?)),
            "--no-shell" => o.no_shell = true,
            "--no-xwayland" => o.no_xwayland = true,
            "--no-session" => o.no_session = true,
            "-h" | "--help" => return Err(String::new()),
            "--" => {
                o.command = args.by_ref().collect();
                break;
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    if o.renderer.is_none()
        && let Ok(r) = std::env::var("AIZOME_RENDERER")
        && !r.is_empty()
    {
        o.renderer = Some(parse_renderer(&r)?);
    }
    if o.headless_outputs.is_empty() {
        o.headless_outputs.push(HeadlessOutput { width: 2880, height: 1800, scale: 2.0 });
    }
    Ok(o)
}

fn parse_renderer(s: &str) -> Result<RendererKind, String> {
    match s {
        "gles" | "gl" => Ok(RendererKind::Gles),
        "pixman" | "software" => Ok(RendererKind::Pixman),
        r => Err(format!("unknown renderer {r}")),
    }
}

/// `WxH` or `WxH@S`.
pub fn parse_output(s: &str) -> Result<HeadlessOutput, String> {
    let (size, scale) = match s.split_once('@') {
        Some((a, b)) => (a, b.parse::<f64>().map_err(|_| format!("bad scale in {s}"))?),
        None => (s, 1.0),
    };
    let (w, h) = size.split_once('x').ok_or_else(|| format!("bad size {s}"))?;
    let width = w.parse().map_err(|_| format!("bad width in {s}"))?;
    let height = h.parse().map_err(|_| format!("bad height in {s}"))?;
    if width < 64 || height < 64 || !(0.5..=4.0).contains(&scale) {
        return Err(format!("{s} is out of range"));
    }
    Ok(HeadlessOutput { width, height, scale })
}

fn default_backend() -> BackendKind {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some() {
        BackendKind::Winit
    } else {
        BackendKind::Udev
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_options() {
        let o = parse(
            [
                "--backend",
                "headless",
                "--headless-output",
                "1280x800@1",
                "--headless-output",
                "2880x1800@2",
                "--renderer",
                "pixman",
                "--no-shell",
                "--",
                "foot",
            ]
            .iter()
            .map(|s| s.to_string()),
        )
        .unwrap();
        assert_eq!(o.backend, BackendKind::Headless);
        assert_eq!(o.renderer, Some(RendererKind::Pixman));
        assert_eq!(o.headless_outputs.len(), 2);
        assert_eq!(o.headless_outputs[1], HeadlessOutput { width: 2880, height: 1800, scale: 2.0 });
        assert!(o.no_shell);
        assert_eq!(o.command, ["foot"]);
        assert!(parse_output("10x10").is_err());
    }
}
