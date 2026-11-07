# udev IMPORT{program} for 61-sekka-identity.rules: given a partition's GPT
# UUID ($1, udev's ID_PART_ENTRY_UUID), print SEKKA_PARTITION=root or esp when
# the kernel command line names it (sekka.root=PARTUUID=…, sekka.esp=…). The
# rule then links it as /dev/disk/by-sekka/root or /dev/disk/by-sekka/esp.
# Shell builtins only: udev runs it with a minimal PATH.
uuid=$1
case $uuid in
*[!0-9a-f-]* | "") exit 0 ;;
esac
read -r -a words <"${SEKKA_CMDLINE:-/proc/cmdline}" || true
for word in "${words[@]}"; do
	case $word in
	"sekka.root=PARTUUID=$uuid") echo "SEKKA_PARTITION=root" ;;
	"sekka.esp=PARTUUID=$uuid") echo "SEKKA_PARTITION=esp" ;;
	esac
done
exit 0
