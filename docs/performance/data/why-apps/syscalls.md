Syscalls from launch to the first frame (median over runs, under strace):

| | kit | gtk4 | adw | qtwidgets | qtquick | sumi | gnome-text-editor | kwrite |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| syscalls | 2732 | 28894 | 30704 | 7555 | 15180 | 2678 | 35838 | 20531 |
| openat ok | 304 | 2351 | 2463 | 503 | 886 | 305 | 2632 | 649 |
| openat ENOENT | 415 | 1739 | 1846 | 1167 | 1412 | 421 | 2286 | 2373 |
| mmap of a file | 93 | 705 | 801 | 436 | 716 | 92 | 884 | 828 |
| mmap | 120 | 856 | 964 | 546 | 888 | 126 | 1058 | 1004 |
| mprotect | 26 | 329 | 352 | 241 | 2652 | 30 | 398 | 342 |
| munmap | 28 | 80 | 82 | 62 | 100 | 30 | 88 | 72 |
| read | 287 | 5390 | 5867 | 837 | 1456 | 278 | 6488 | 1010 |
| newfstatat/statx | 462 | 2952 | 3109 | 814 | 1373 | 416 | 5027 | 6620 |
| connect | 3 | 5 | 5 | 6 | 6 | 3 | 12 | 6 |
| futex | 26 | 970 | 1150 | 840 | 1078 | 13 | 1725 | 1412 |
| poll/ppoll/epoll_wait | 11 | 242 | 296 | 160 | 168 | 8 | 408 | 174 |
| sendmsg/recvmsg | 9 | 199 | 220 | 115 | 123 | 8 | 269 | 129 |
| clone/clone3 | 5 | 7 | 8 | 7 | 27 | 7 | 14 | 14 |
| opened: shared objects | 13 | 774 | 880 | 117 | 186 | 13 | 977 | 219 |
| opened: other files | 92 | 124 | 131 | 37 | 72 | 92 | 166 | 64 |
| opened: /proc, /sys, /dev | 1 | 838 | 838 | 4 | 132 | 1 | 840 | 7 |
| opened: font files | 29 | 2 | 4 | 4 | 4 | 29 | 8 | 6 |
| opened: fontconfig caches | 94 | 61 | 63 | 18 | 16 | 92 | 58 | 20 |
| opened: configuration (conf, xml, ini, json, css, qml) | 75 | 381 | 380 | 19 | 148 | 75 | 378 | 20 |
| opened: icons and images | 0 | 2 | 2 | 4 | 2 | 0 | 20 | 11 |

kit: most frequent: openat 719, close 306, read 287, newfstatat 235, fstatfs 182, access 158, mmap 120, statx 118, readlink 110, fstat 109, munmap 29, write 28
    sockets connected: ?
gtk4: most frequent: read 5390, readlink 4466, openat 4090, lseek 4003, close 2428, fstat 1985, futex 970, newfstatat 967, mmap 856, mremap 759, getpid 356, access 341
    sockets connected: $XDG_RUNTIME_DIR/at-spi/bus, $XDG_RUNTIME_DIR/bus, $XDG_RUNTIME_DIR/wayland-1, ?
adw: most frequent: read 5867, readlink 4466, lseek 4319, openat 4309, close 2544, fstat 2093, futex 1154, newfstatat 1016, mmap 965, mremap 812, write 374, getpid 370
    sockets connected: $XDG_RUNTIME_DIR/at-spi/bus, $XDG_RUNTIME_DIR/bus, $XDG_RUNTIME_DIR/wayland-1, ?
qtwidgets: most frequent: openat 1670, futex 860, read 836, mmap 546, close 510, newfstatat 434, access 330, fstat 317, lseek 307, readlink 268, mprotect 241, write 217
    sockets connected: $XDG_RUNTIME_DIR/bus, $XDG_RUNTIME_DIR/wayland-1, ?
qtquick: most frequent: mprotect 2656, openat 2298, read 1455, futex 1064, getpid 978, readlink 914, close 902, mmap 887, newfstatat 596, fstat 560, lseek 510, access 343
    sockets connected: $XDG_RUNTIME_DIR/bus, $XDG_RUNTIME_DIR/wayland-1, ?
sumi: most frequent: openat 726, close 307, read 279, newfstatat 237, fstatfs 182, access 158, mmap 126, fstat 110, readlink 96, statx 69, mprotect 30, munmap 28
    sockets connected: ?
gnome-text-editor: most frequent: read 6489, openat 4918, lseek 4674, readlink 4466, newfstatat 2759, close 2724, fstat 2263, futex 1744, mmap 1060, mremap 922, write 491, access 429
    sockets connected: $XDG_RUNTIME_DIR/at-spi/bus, $XDG_RUNTIME_DIR/bus, $XDG_RUNTIME_DIR/wayland-1, /var/run/nscd/socket
kwrite: most frequent: statx 5373, access 3640, openat 3023, futex 1421, read 1011, mmap 1005, newfstatat 816, write 682, close 654, readlink 643, fstat 431, mprotect 343
    sockets connected: $XDG_RUNTIME_DIR/bus, $XDG_RUNTIME_DIR/wayland-1, ?
