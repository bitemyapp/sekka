# Sekka 雪花 brand

Sekka 雪花 is a NixOS desktop you install and look after through a graphical interface: Genkan installs it, Tatami is the desktop, Yukimi manages software and updates. Its identity has two sources. The name is an Edo-period word for snow-crystal patterns, made popular by Doi Toshitsura's 1832 microscope study *Sekka Zusetsu*. The colours and shapes are Tatami's Tokyo Night desktop, where people spend their time. **Night** is the primary theme. **Day** exists for documentation and light-mode pages; Tatami itself is dark only. **High contrast** is an added dark theme for people who need more: every text colour reaches 7:1 (WCAG AAA) on every surface and edges reach 3:1, while Tatami's own Night values stay exactly as they are.

## Voice

- Write plainly and say what does not work yet. The project README opens with "Why you shouldn't use Sekka (yet)"; keep that candour everywhere.
- Write "Sekka" with a capital in prose. Lowercase "sekka" appears only inside the wordmark, which ships as outlines.
- Keep component names romanized in running text: Genkan, Tatami, Yukimi. Add kanji only beside a heading or a name's first appearance, set in `kanji-label`: Sekka 雪花, Genkan 玄関, Tatami 畳, Yukimi 雪見.
- Say what each name means when it helps, in one clause: Genkan is the entrance, a tatami room is where you live, a yukimi window lets you watch the snow from somewhere warm.
- Name menu paths as the desktop shows them, joined with ›: **Style › Background settings**.
- Use sentence case for labels and headings. No emoji, no exclamation marks.
- Real copy to match: "Welcome to Sekka", "Choose your disk, account, desktop, applications, and location.", "Snow-viewing for NixOS."

## Logo

The mark is a *yukiwa* 雪輪, the snow ring of Japanese family crests: six circles of radius 76 with their centres 36 from the middle, unioned into a softly scalloped ring, on a 256 grid. A six-armed crystal is cut through it: arms from radius 19 to 84, one pair of branches at 60 on each arm, and a hollow hexagon at the centre. Arms point straight up.

- On Night, use `sekka-crest-blue.svg` (ink `crest`). On Day, use `sekka-crest-day.svg` (the Day value of `crest`).
- On photographs and other grounds, use the single-ink crest that reaches 3:1 there: `sekka-crest-night.svg` (ink `surface` at night) on light grounds, `sekka-crest-snow.svg` (ink `text-strong` at night) on dark ones.
- `sekka-crest-line.svg` is the crest in line on Night, for large decorative use only; it carries `crest`, `text-strong` and `surface`.
- Use the horizontal lockup where there is room: `sekka-lockup-night.svg` (crest, wordmark in `text-strong`, 雪花 in `text`) or `sekka-lockup-day.svg`. The vertical 雪花 sits beside the wordmark like a seal; never move it to a second line.
- Use the stacked lockup (`sekka-stacked-*`) for square spaces such as a splash screen or a social card.
- Use the wordmark alone (`sekka-wordmark-*`) only where the crest already appears nearby.
- Keep clear space of a quarter of the crest's width on every side.
- Never draw the full crest below 24px. From 16 to 23px use `sekka-symbolic.svg`, which drops the branches and thickens the cuts so they survive the pixel grid; it is single ink (#2e3436, the GTK symbolic convention) and may be recoloured to any text token.
- The application icon is `sekka-app-icon.svg`: the blue crest on a square `surface` tile with no rounding (`radius-none`), like every Tatami surface.
- Don't rotate the crest, add outlines, shadows or gradients to it, or recolour it outside `crest`, `text-strong` and `surface`. Don't set the wordmark in another face or in capitals. Don't lock the crest up with the NixOS snowflake as if they were one mark: Sekka runs on NixOS, but the marks stay separate.

## Colour

- Ground everything on `surface`. Raise cards and popovers to `surface-raised`; mark selected text and rows with `selection`.
- `accent` is the only accent. Use it for the focused window's 2px border, notification and launcher borders, today in the calendar, links, focus rings and accent fills. Text on an accent fill is `on-accent`, never white at night.
- Running text is `text`; headings and the wordmark are `text-strong`; secondary copy is `text-secondary`. All three hold 4.5:1 or more on `surface`, `surface-raised` and `selection` in both themes.
- `text-faint` stays exactly as Tatami has it, but it is under 4.5:1. Use it only for decorative, repeated labels like the calendar's week numbers.
- Show status with `danger`, `success` and `warning`, always with a word or an icon as well. Recording and a Keep Awake request that is not actually holding are `danger`.
- `purple` and `teal` are for terminal output and charts, not interface chrome.
- `indigo` is cloth: large fields such as the noren in the Genkan icon. Only marks and text 24px or larger go on it.
- Draw the crest in `crest`, an alias of `accent`: one brand blue per theme, at 4.5:1 or more on every surface and on white.
- Draw focus as a solid 2px `accent` ring outside the control, matching Tatami's window borders. It reaches at least 4.5:1 on every surface in Night and Day, and 7.4:1 in High contrast.
- In High contrast, set raised surfaces and controls apart with a `border-width` edge in `border-inactive` (3.6:1 or more), never by shade alone: `surface-raised` is only slightly lighter than `surface` there.
- Offer High contrast wherever people choose a theme, and follow the system's contrast preference where one exists.

## Type

- JetBrains Mono (`mono`) is Tatami's face. Bar text is `bar`, 12px in the 26px bar; bar glyphs come from JetBrainsMono Nerd Font at `bar-glyph`; the terminal is `terminal`.
- Use `display` (mono, 800) for one headline per screen, such as Genkan's welcome title.
- Noto Serif JP (`serif-jp`) sets kanji only: the 雪花 `seal` and `kanji-label`. Never set Latin text in it.
- Documentation and web pages set running text in Atkinson Hyperlegible Next (`sans`) at `body`, with `h1`, `h2` and `caption`: the Braille Institute drew it for low-vision readers, and it is packaged in Nixpkgs, so pages serve it themselves instead of loading it from a font service. These prose styles are additions for the web; inside the desktop everything is mono except notifications, which Mako sets in Liberation Sans (`notification`).

## Shape and space

- Corners are square everywhere (`radius-none`). The crest is the one round form.
- Borders are `border-width` (2px): `accent` for the focused window, notifications and the launcher; `text` for the launcher's dialogs; `border-inactive` for unfocused windows.
- Tiled windows sit `gap-in` apart and `gap-out` from the screen edge. The bar is `bar-height` tall with `bar-edge` margins, `workspace-slot`, `indicator-slot` and `icon-slot` widths. Notifications are `card-width` wide with `card-pad-y` and `card-pad-x` padding and `notify-gap` between them; launcher menus use `dialog-pad`.
- Fills are flat. Tatami draws no drop shadows.

## Iconography

- Bar and menu glyphs come from JetBrainsMono Nerd Font (Material Design Icons, such as U+F0343 for Logout) at `bar-glyph`.
- Component icons sit on a square `surface` tile in flat Tokyo Night fills. `genkan.svg` is a noren, an entrance curtain, in `indigo`, with the crest in `text-strong` dyed across it and split by the slits as on real cloth. `tatami.svg` is a four-and-a-half-mat room: four mats around a half mat, which also reads as tiled windows with the focused one in `accent`.
- Yukimi's shipping icon is a shōji frame round a snowy night. It still uses NixOS blues and a cream frame and does not follow this system yet.
- The bar's menu button shows the NixOS logo today, and Genkan's welcome page shows the `nix-snowflake` icon. Adopting this system replaces both with the crest (`sekka-symbolic.svg` in the bar).

## Motion

- Tatami has no decorative motion. Yukimi's falling snow is the one ambient animation, and it pauses when the window is in the background. Keep it that way, and honour reduced-motion settings.

## Files

- `logos/`: the crests, the symbolic glyph, the application icon, and Night and Day versions of the horizontal and stacked lockups and the wordmark. Every file names its ink; none uses `currentColor`.
- `family/`: proposed icons for Genkan (`genkan.svg`) and Tatami (`tatami.svg`). Yukimi's icon stays at `apps/yukimi/yukimi/data/io.github.bitemyapp.Yukimi.svg`.
- `tokens.json`: every colour with its Night and Day value and where it is used, and the type, spacing, radius, size and border tokens. Night values are Tatami's own, from `desktop/tatami/config`.
- `generate.py`: draws every SVG above. Change the script and regenerate rather than editing an SVG by hand; its header has the command, which takes the fonts from the root flake's Nixpkgs.
