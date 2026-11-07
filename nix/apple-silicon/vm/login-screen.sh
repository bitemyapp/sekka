# Run as root inside a Mac image's test VM (boot-image-in-vm.sh): start the
# login screen and check it stays up. Prints `login-screen: …` lines and
# exits non-zero, with greetd's journal and what tty1 shows, if it doesn't.
#
# Both images start Aizome's compositor on the Mac's GPU, by its render
# node's name (AIZOME_DRM_DEVICE in greetd's command, module.nix), which a
# VM hasn't, so the compositor stops at once and greetd gives up. The one
# VM-only stand-in: that name for the VM's own render node (virtio-gpu).
# What it can't show is the T6030 display and GPU handoff itself.
set -u
cfg=$(systemctl show -p ExecStart --value greetd.service | grep -o '/nix/store/[^ ;]*\.toml' | head -n1)
dev=$(sed -n 's/.*AIZOME_DRM_DEVICE=\([^ ]*\).*/\1/p' "$cfg" | head -n1 | tr -dc 'A-Za-z0-9/._-')
if [ -n "$dev" ] && [ ! -e "$dev" ] && [ -e /dev/dri/renderD128 ]; then
	mkdir -p "$(dirname "$dev")" && ln -s /dev/dri/renderD128 "$dev"
	echo "login-screen: $dev stands for /dev/dri/renderD128"
fi
# Started afresh, so systemd counts its restarts from here.
systemctl reset-failed greetd.service 2>/dev/null
systemctl restart greetd.service
sleep 15
state=$(systemctl is-active greetd.service)
restarts=$(systemctl show -p NRestarts --value greetd.service)
greeter=none
pgrep -u greeter -x aizome-greeter >/dev/null && greeter=running
echo "login-screen: greetd $state, $restarts restarts in 15 s, greeter $greeter"
if [ "$state" != active ] || [ "$restarts" != 0 ] || [ "$greeter" != running ]; then
	journalctl -b -o cat -u greetd.service | tail -n 10 | sed 's/^/greetd: /'
	fold -w 200 /dev/vcs1 | grep -v '^ *$' | tail -n 5 | sed 's/^/tty1: /'
	exit 1
fi
