#!/usr/bin/env python3
"""Read-only Linux graphics inventory; optional tiny device-platform EGL probe.

No window, compositor, permission, power, clock or desktop configuration changes.
The EGL probe creates a 16x16 offscreen pbuffer per enumerated device and reads
one pixel. It establishes rendering capability, not Wayland presentation or speed.
Run with the host's Python/library loader; record Nix-loader compatibility separately.
"""
import argparse
import ctypes as c
import datetime
import glob
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess


def command(argv):
    try:
        run = subprocess.run(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                             text=True, timeout=10, check=False)
        return {"argv": argv, "returncode": run.returncode,
                "stdout": run.stdout, "stderr": run.stderr}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"argv": argv, "error": str(error)}


def device(path):
    result = {"path": path}
    try:
        stat = os.stat(path)
        result.update(mode=oct(stat.st_mode & 0o777), uid=stat.st_uid, gid=stat.st_gid)
        fd = os.open(path, os.O_RDWR | os.O_CLOEXEC)
        os.close(fd)
        result["open_read_write"] = True
    except OSError as error:
        result.update(open_read_write=False, error=str(error), errno=error.errno)
    return result


def egl_probe():
    # Signatures/constants from the host's Khronos EGL headers. No native display
    # or DRM master fd is passed: EGL_EXT_platform_device is offscreen only.
    egl = c.CDLL("libEGL.so.1")
    ptr, integer, boolean = c.c_void_p, c.c_int32, c.c_uint32

    def bind(name, result, *args):
        fn = getattr(egl, name)
        fn.restype, fn.argtypes = result, args
        return fn

    get_proc = bind("eglGetProcAddress", ptr, c.c_char_p)
    error = bind("eglGetError", integer)
    query = bind("eglQueryString", c.c_char_p, ptr, integer)
    initialize = bind("eglInitialize", boolean, ptr, c.POINTER(integer), c.POINTER(integer))
    terminate = bind("eglTerminate", boolean, ptr)
    choose = bind("eglChooseConfig", boolean, ptr, c.POINTER(integer), c.POINTER(ptr), integer,
                  c.POINTER(integer))
    bind_api = bind("eglBindAPI", boolean, integer)
    pbuffer = bind("eglCreatePbufferSurface", ptr, ptr, ptr, c.POINTER(integer))
    create = bind("eglCreateContext", ptr, ptr, ptr, ptr, c.POINTER(integer))
    current = bind("eglMakeCurrent", boolean, ptr, ptr, ptr, ptr)
    destroy_context = bind("eglDestroyContext", boolean, ptr, ptr)
    destroy_surface = bind("eglDestroySurface", boolean, ptr, ptr)

    def extension(name, result, *args):
        address = get_proc(name.encode())
        if not address:
            raise RuntimeError(f"missing {name}")
        return c.CFUNCTYPE(result, *args)(address)

    devices = extension("eglQueryDevicesEXT", boolean, integer, c.POINTER(ptr), c.POINTER(integer))
    device_string = extension("eglQueryDeviceStringEXT", c.c_char_p, ptr, integer)
    display = extension("eglGetPlatformDisplayEXT", ptr, integer, ptr, c.POINTER(integer))
    decode = lambda value: value.decode("utf-8", "replace") if value else None
    result = {"client_extensions": decode(query(None, 0x3055)), "devices": []}
    found, count = (ptr * 16)(), integer()
    if not devices(16, found, c.byref(count)):
        raise RuntimeError(f"eglQueryDevicesEXT failed: {error():#x}")
    for handle in found[:count.value]:
        row = {"device_extensions": decode(device_string(handle, 0x3055))}
        result["devices"].append(row)
        for key, value in (("drm_device", 0x3233), ("drm_render_node", 0x3377), ("renderer", 0x335F)):
            row[key] = decode(device_string(handle, value))
            error()  # Optional query tokens need not be supported by every device.
        dpy, surface, context = display(0x313F, handle, None), None, None
        major, minor = integer(), integer()
        if not dpy or not initialize(dpy, c.byref(major), c.byref(minor)):
            row["initialize_error"] = hex(error())
            continue
        try:
            row.update(egl_version=f"{major.value}.{minor.value}",
                       egl_vendor=decode(query(dpy, 0x3053)),
                       egl_extensions=decode(query(dpy, 0x3055)))
            attributes = (integer * 11)(0x3033, 1, 0x3040, 8, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3038)
            configs, size = (ptr * 1)(), integer()
            if not bind_api(0x30A2) or not choose(dpy, attributes, configs, 1, c.byref(size)) or not size.value:
                raise RuntimeError(f"OpenGL pbuffer config unavailable: {error():#x}")
            surface = pbuffer(dpy, configs[0], (integer * 5)(0x3057, 16, 0x3056, 16, 0x3038))
            context = create(dpy, configs[0], None, (integer * 1)(0x3038))
            if not surface or not context or not current(dpy, surface, surface, context):
                raise RuntimeError(f"OpenGL pbuffer context failed: {error():#x}")
            gl_string = extension("glGetString", c.c_char_p, c.c_uint32)
            clear_color = extension("glClearColor", None, c.c_float, c.c_float, c.c_float, c.c_float)
            clear = extension("glClear", None, c.c_uint32)
            read = extension("glReadPixels", None, integer, integer, integer, integer,
                             c.c_uint32, c.c_uint32, ptr)
            gl_error = extension("glGetError", c.c_uint32)
            row.update(gl_vendor=decode(gl_string(0x1F00)), gl_renderer=decode(gl_string(0x1F01)),
                       gl_version=decode(gl_string(0x1F02)))
            clear_color(0.25, 0.5, 0.75, 1.0)
            clear(0x4000)
            pixel = (c.c_ubyte * 4)()
            read(0, 0, 1, 1, 0x1908, 0x1401, pixel)
            row.update(readback_rgba=list(pixel), gl_error=hex(gl_error()))
        except RuntimeError as exception:
            row["context_error"] = str(exception)
        finally:
            current(dpy, None, None, None)
            if context:
                destroy_context(dpy, context)
            if surface:
                destroy_surface(dpy, surface)
            terminate(dpy)
    result["loaded_graphics_mappings"] = sorted({line.split()[-1] for line in Path("/proc/self/maps").read_text().splitlines()
                                              if any(word in line for word in ("libEGL", "libGL", "nvidia", "dri.so"))
                                              and "/" in line.split()[-1]})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--egl", action="store_true", help="also create tiny offscreen device contexts")
    args = parser.parse_args()
    result = {"schema": 1, "recorded_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "tool_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              "platform": platform.platform(), "hostname": platform.node(),
              "uid": os.getuid(), "groups": os.getgroups(),
              "scope": "read-only inventory and optional offscreen capability; not a benchmark",
              "devices": [device(path) for path in sorted(glob.glob("/dev/dri/card[0-9]*") + glob.glob("/dev/dri/renderD*")
                                                          + ["/dev/nvidia0", "/dev/nvidiactl"])],
              "commands": [command(argv) for argv in [
                  ["nvidia-smi", "--query-gpu=name,driver_version,uuid,pci.bus_id,memory.total,memory.used,power.draw,utilization.gpu", "--format=csv,noheader"],
                  ["lspci", "-nnk"], ["getfacl", "/dev/dri/renderD128", "/dev/dri/card1"],
                  ["sway", "--version"], ["ldconfig", "-p"]]]}
    if args.egl:
        try:
            result["egl"] = egl_probe()
        except (OSError, RuntimeError) as error:
            result["egl_error"] = str(error)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
