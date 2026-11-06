# Responsiveness probes

How `apps/tasuke/DESIGN.md`'s "Fast without taking over the machine"
figures were measured, on runnymede (Ryzen 7 9800X3D, RTX 5090 driving the
displays), 2026-11-05/06. They compare conditions on one machine at one
time; other work on the machine shows up in every condition.

## The processor

A llama-server for Gemma 4 26B-A4B in the 8 GB configuration (`-ngl 99
-ncmoe 24 -b 2048 -ub 2048 -t 6 -c 32768 -np 1 -fa on -ctk q8_0 -ctv q8_0`)
runs as a transient user service, once with Tasuke's unit settings and once
without, while `server_load.py PORT` keeps it decoding:

    systemd-run --user --unit=resp-llama -p Nice=10 -p CPUWeight=20 -p IOWeight=20 llama-server …
    systemd-run --user --unit=resp-llama llama-server …

Each probe runs in its own scope at the default weight, like an app:

    systemd-run --user --scope -p CPUWeight=100 python3 -I timer_probe.py LABEL
    systemd-run --user --scope -p CPUWeight=100 python3 -I busy_probe.py LABEL

`timer_probe.py` wakes every 2 ms for 20 s and reports lateness, and times
a fixed hashing task; `busy_probe.py` hashes on all 16 hardware threads for
20 s and reports the blocks done. Run each with no model before and after.

## The display GPU

`gpu_frame_probe.c` renders a 2560×1440 frame of four blended full-screen
layers every 4.17 ms (240 Hz) on one GPU, chosen by its render node through
EGL's device platform, waits for each, and reports frame times. Build it
against libglvnd and run it on NVIDIA's EGL:

    cc -O2 -I$LIBGLVND_DEV/include -L$LIBGLVND/lib -o probe gpu_frame_probe.c -lEGL -lGLESv2
    __EGL_VENDOR_LIBRARY_FILENAMES=/run/opengl-driver/share/glvnd/egl_vendor.d/10_nvidia.json \
      LD_LIBRARY_PATH=/run/opengl-driver/lib ./probe /dev/dri/renderD129 LABEL 20

`router_load.py MODEL decode|prefill KEYFILE` keeps Tasuke's own router
(127.0.0.1:8737) decoding 256-token replies or reading ~10K-token prompts;
the probe runs after 25 s, once the model has loaded and settled.
