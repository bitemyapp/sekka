#!/bin/sh
set -eu
cd /home/callen/work/hotaru-sprites-20261028/oracle
exec flock -x /home/callen/.cache/hotaru-workstation.lock /home/callen/.cache/hotaru-reference/zig-x86_64-linux-0.17.0/zig build-lib -dynamic -lc -O ReleaseSafe --dep z2d -Mroot=oracle.zig --dep z2d_options -Mz2d=/home/callen/work/ghostty-reference-20261028/zig-pkg/z2d-0.13.0-j5P_HgsRFABzZDH54gI35P0OSrg7HFyJwS2bXX5uBdU4/src/z2d.zig -Mz2d_options=z2d-options.zig -femit-bin=libsprites.so
