set positional-arguments
set shell := ["bash", "scripts/just-shell", "-cu"]

# List the checkout's development commands.
default:
    @just --list

# Build an app from this checkout and replace it in the running Aizome session.
replace app:
    @aizome-dev reload "$1" --checkout "$PWD"
