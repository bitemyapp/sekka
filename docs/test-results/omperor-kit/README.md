# Omperor on Aizome's kit: screenshots

Captured on 2026-11-05 for [PR #133](https://github.com/bitemyapp/sekka/pull/133),
after its rebase onto `main`. All five PNGs are original 1440 × 900 `grim`
captures of the whole output at scale 1, copied byte for byte. No crops,
scaling, compositing or generated artwork were applied.

Omperor is an Aizome app for coding agents kept outside Sekka
([bitemyapp/omperor](https://github.com/bitemyapp/omperor), branch `aizome`).
These pictures show it using what this PR adds to `aizome-kit`. The agent's
turns come from Omperor's built-in mock backend: no account, provider, OMP
helper or inference was involved, and the model name `deterministic` is
the mock's. The project is the README demo from Omperor's
`tests/readme-demo`, committed by a made-up `Demo` author, with its work
patch applied.

| Capture | What it shows |
| --- | --- |
| [Agents, Night](agents-night.png) | A turn under way (`/mock scenario=tour`). The conversation's tool calls are folded to a line each, the `task` call opened with a click. The Agents panel, the side pane of `split(..).second_size(..)`, lists the two scouts while they run. The composer is in its Steer mode with Stop. This is the fourth of twelve frames taken 0.5 s apart. |
| [Change, Night](change-night.png) | The finished turn: the transcript kept to its end as the answer arrived (`scroll(..).follow_end()`), with highlighted code, a diff and a callout. The Changes panel (❖1) shows the project's uncommitted files and `chat.rs`'s diff. A three-line draft, its lines broken with Shift+Return (Return goes to the program, through `.on_key`, to send), makes the composer as tall as its text (`text_editor(..).fit_lines(..)`). |
| [Change, Day](change-day.png) | The same, in Day. |
| [Math, Night](math-night.png) | A second session (❖N) with the mock's showcase answer, scrolled back to its Math and Diagrams. Formulas sit in the sentence as inline boxes (`Span::inline`), and a Mermaid flowchart keeps its proportions (`canvas(..).aspect(..)`). The sentence was selected by dragging, copied with ❖C and pasted with ❖V into the composer, which shows it back as LaTeX (`$e^{i\pi} + 1 = 0$`), then selected again. |
| [Math, Day](math-day.png) | The same, in Day. Selected text keeps its colour over the selection fill, as the kit's text fields and editor already draw it, so in Day it reads with less contrast than in Night. |

## How they were made

On wx-workstation, a private Aizome compositor ran with its headless
backend and the pixman renderer
(`aizome --backend headless --renderer pixman --headless-output 1440x900@1 --no-session --no-shell --no-xwayland`),
with Aizome's shell started beside it. Both are release builds of this
branch at `6a81f60ce66965a7a78ade629bdc39001a534495` (rustc 1.99.0, from
`nix develop .#aizome`). Omperor's `aizome` branch at `e8e62b9` was built in
release with its own toolchain (rustc 1.99.0-nightly 2026-08-07), with
`vendor/sekka` replaced by this same tree and `vendor/oh-my-pi` at its
submodule commit `12d59c09`. Omperor's branch predates `main`'s
`Length::FillPortion(NonZeroU16)` (#275), so two call sites were adapted for
the build ([capture/omperor-kit-adaptation.patch](capture/omperor-kit-adaptation.patch));
Omperor was otherwise unchanged.

Each theme ran in a fresh session with its own home, configuration, data,
state, cache and runtime directories under `/tmp`, and D-Bus addresses that
lead nowhere, so the shell shows no network, Bluetooth or media status.
`settings.toml` chose the theme. Omperor's state database was seeded with
Developer mode and the mock backend, as Settings ▸ Developer sets them.
The window was driven only through the compositor's own input path
(`aizomectl input type`, `key`, `chord`, `motion`, `click`, `button`,
`scroll`), with positions read from the kit's `AIZOME_KIT_INSPECT` files.
The pointer was moved over the Deskbar before each capture; `grim` doesn't
draw it. The scripts, as run, are in [capture/](capture/).

```sh
# In each tree: Sekka at the commit above, Omperor at e8e62b9 with vendor/sekka
# pointing at it and the patch applied.
nix develop .#aizome -c cargo build --locked --release \
  --manifest-path desktop/aizome/Cargo.toml -p aizome-comp -p aizome-shell -p aizome-ipc
nix develop -c cargo build --release --bin omperor   # in Omperor
capture/omp-drive.sh night
capture/omp-drive.sh day
```

The scripts expect the builds and Omperor's tree where they were on
wx-workstation (`~/sekka-work/omperor-rebase`), and `grim`, `sqlite3`,
`python3` and `git` on `PATH`. The stand-in sessions are component evidence
of the kit's features in a real compositor and shell. They are not an Aizome
session from the ISO, not GPU rendering, and not a test of a real agent.

## Hashes (SHA-256)

| File | SHA-256 |
| --- | --- |
| `agents-night.png` | `54f735f58e43cf6375f5fd17e41b7ff343fabe3dcd962b9d590732080df45d57` |
| `change-night.png` | `af174940c96d2ed1dba5ebd1d841331de0829320b71dd8dffa2fb621048da260` |
| `change-day.png` | `8a74e1aa37abadd06097b1e2945f3978655990a827bed83014858d4291072f8c` |
| `math-night.png` | `90f849c577422eca30b486ecea024bd849887ce0e26f7aed661e1a2b464ebc05` |
| `math-day.png` | `fcf74e45025a4e2901658f1a405711d98a4e7a78b04725a565ce676f1169b14a` |
| `aizome` | `3f2deedeaa9aee2b715914b8ea505745e48c1320ba6507a4ca7d166e0d36f8b5` |
| `libaizome_comp_core.so` | `deea024eed7617c953315f2761ac670233a59eb94a7145e91ba9e0c87891ffcd` |
| `libaizome_comp_logic.so` | `78f4a3dd85eeb61f502a74fe9a998b3b82f696ce110f7fc895b8bd59fb484122` |
| `aizome-shell` | `28484b1717498da8c281f14b9d353711bb98204a8c0177a64fa5e8e28d993d74` |
| `aizomectl` | `99d92945c0a0df55bd9e9daccc321a5ab6818ce7218e701d720c881ba3bb941e` |
| `omperor` | `2ef9dcb85f0fb89701576c58ac6c739beccc071131925d2c4cddf1221b908fa2` |
