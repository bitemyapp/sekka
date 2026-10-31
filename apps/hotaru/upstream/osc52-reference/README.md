# Unchanged Ghostty OSC 52 reference

`receipt.json` identifies 201 selected GTK observations across nine runs and
111 direct exported-parser cases. Raw attempts are never rewritten; the
receipt's `selected` and `scope` fields identify harness failures and
superseded runs. See [the explanation](../../docs/osc52-reference.md).

The reproducible working directory on `wx-workstation` is
`/home/callen/work/hotaru-osc52-reference-20261028`. These exact tools remain
there along with all logs, screenshots and the generated shim. The committed
archive excludes compiled binaries and generated large clipboard corpora.
Copy `tools/` into that directory to reproduce with its pinned dependencies.
The GTK tool and parser runner acquire the shared workstation lock themselves.

```sh
cd /home/callen/work/hotaru-osc52-reference-20261028
python3 tools/gtk_probe.py --suite allow --output artifacts/new-allow
# Other suites: deny, ask, large, lifecycle, order, raw, stream-parser.
flock -x /home/callen/.cache/hotaru-workstation.lock \
  cc -std=c11 -Wall -Wextra -Werror -O2 -shared -fPIC tools/parser_oracle.c \
  -I/home/callen/work/ghostty-reference-20261028/include \
  -L/home/callen/work/ghostty-reference-20261028/zig-out/lib \
  -Wl,-rpath,/home/callen/work/ghostty-reference-20261028/zig-out/lib \
  -lghostty-vt -o artifacts/libparser-oracle.so
python3 tools/parser_oracle.py
```

The GUI `bin/ghostty` recorded by the harness is a Nix wrapper, not its ELF.
The receipt identifies the actual `.ghostty-wrapped` ELF separately. Its
software rendering environment is isolated; no host clipboard, settings or
compositor are used. The direct parser shim links the unchanged reference
library. No diagnostic implementation replaces the parser or GTK frontend.
