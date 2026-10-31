# The real original and patched daemons, synchronized at PID publication.
# Private namespaces keep the host's services, network and /run untouched.
{ pkgs, patched }:
pkgs.runCommand "sekka-avahi-startup"
  {
    nativeBuildInputs = [ pkgs.python3 ];
  }
  ''
    python3 ${../tests/check_avahi_startup.py} \
      --bwrap ${pkgs.bubblewrap}/bin/bwrap \
      --gdb ${pkgs.gdb}/bin/gdb \
      --original ${pkgs.avahi}/sbin/avahi-daemon \
      --patched ${patched}/sbin/avahi-daemon \
      --output "$out"
  ''
