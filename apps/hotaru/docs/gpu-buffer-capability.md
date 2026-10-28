# Offscreen GPU buffer and Nix loader probe

The workstation can render with its NVIDIA driver under the same Nix libc
loader used by the current Hotaru package. **DMA-BUF export does not work under
the current SSH user's device access.** This is a partial capability result,
not a GPU renderer, Wayland presentation result or performance comparison.

The Linux-only diagnostic [`gpu-buffer-probe.c`](../tools/gpu-buffer-probe.c)
creates two unshared surfaceless EGL/GLES contexts through the device platform.
It draws four opaque quadrants into a 32×32 RGBA texture and verifies every
producer pixel. It then attempts to export a single-plane DMA-BUF, wait for a
producer fence, import into the other context and verify that texture's pixels.
The planned consumer path closes the exported descriptor after successful
import and explicitly destroys images, textures and contexts. Failed attempts
exit the diagnostic process, whose resources are reclaimed by the OS.

## Observed result

Both the host and explicitly selected Nix loader runs in
[`check-5/result.json`](../upstream/gpu-buffer/check-5/result.json) report:

- NVIDIA RTX PRO 6000 Blackwell Max-Q, OpenGL ES 3.2, driver 580.159.04.
- All 1,024 producer pixels match: 4,096 RGBA bytes, no GL error.
- Export metadata is available: one plane, fourcc `875708993`, modifier
  `0x0300000000e08012`.
- The descriptor export fails with `EGL_BAD_MATCH` (`0x3009`). Both syscall
  traces show repeated `EACCES` opening `/dev/dri/renderD128` read-write.
- The full round-trip report remains **failed**, and the runner exits nonzero.
  Fence waiting, import, consumer comparison and successful-path cleanup are
  consequently not runtime-qualified by these attempts.

The failure and denied device opens are consistent with the earlier permission
inventory. No device permission, group membership, kernel module, driver,
desktop session or power setting was changed. A device-platform context alone
does not establish export/import or normal presentation support.

## Loader isolation and identities

The runner uses host GCC with `-Wall -Wextra -Werror`; this is a small loader
diagnostic rather than a Nix-built application. Its second executable selects
the exact package interpreter:

`/nix/store/pqyiidz5mi73wv5f91d48s9plcqyj5bz-glibc-2.44-57/lib/ld-linux-x86-64.so.2`

The second run resolves ordinary file-backed mappings only from that libc,
explicit Nix GLVND/libdrm directories, the probe executable, and four exact
host NVIDIA driver files. It does not add a host system library directory to
the library path. Driver names, loaded paths and SHA-256 values are recorded;
device and deleted driver memfd mappings are recorded separately from library
files. This qualifies the observed file-backed loader scope, not arbitrary
future drivers or application dependencies.

| Artifact | SHA-256 |
| --- | --- |
| C source | `9ae5753f7a6d316607009c127e0e991f6c0919772328e912c523c2c2abcf4102` |
| Runner | `282542cd5a8f01ba4f424a5055712c4540b619bae847f8c893517120d439d19b` |
| Host executable | `378bcc0f57e499eb4b4eb8a39cc341fc2a80d3eb6369eb686c01fedc662e0799` |
| Nix-loader executable | `809faee46126895ce65edc0fe4e00af3bd1b6d19d6373911189ce49c9194846b` |

The runner compiles and hashes the archived source copy in each result
directory. Independent review corrected that provenance boundary and required
`EGL_IMAGE_PRESERVED_KHR=TRUE` at both image creations so a future successful
pixel comparison has a preservation guarantee. The relevant ownership and
preservation rules are the Khronos
[image base](https://registry.khronos.org/EGL/extensions/KHR/EGL_KHR_image_base.txt),
[DMA-BUF export](https://registry.khronos.org/EGL/extensions/MESA/EGL_MESA_image_dma_buf_export.txt),
and [DMA-BUF import](https://registry.khronos.org/EGL/extensions/EXT/EGL_EXT_image_dma_buf_import.txt)
specifications. Review found no concrete descriptor/bounds/context-lifetime
defect in the intended success path, which still requires actual execution.

The producer readback completes its prior draw before the later fence. Even a
future passing round trip would therefore establish a CPU-waited transfer,
not independent validation of outstanding GPU work or asynchronous cross-process
synchronization. No timing is reported by this probe.

## Reproduction and next work

Full original attempts and archived sources remain on wx-workstation in
`/home/callen/work/hotaru-gpu-buffer-20261028/`. The `nix-libc`, `nix-glvnd` and
`nix-drm` symlinks there are registered GC roots. Earlier attempts remain raw
diagnostics: they exposed a missing Nix libdrm, an incorrect external-platform
environment-variable name and overly broad classification of non-file mappings.
The current report supersedes those for the narrow loader claim.

Run `gpu-buffer-check.py --help` for explicit loader, GLVND, libdrm and driver
file arguments. Every attempt requires a new output directory and takes the
shared workstation lock. `--trace-calls` records only its own subprocesses;
timeouts terminate that process group. The exact compile/run commands and
environment allowlist are saved in the report.

Offscreen GPU component work can continue with the demonstrated context path.
Hardware Wayland qualification still needs a working import/export path under
the intended runtime account, then compositor/client synchronization and
presentation checks. No native toolkit GPU path is added by this diagnostic.
