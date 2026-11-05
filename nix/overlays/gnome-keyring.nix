# The Secret Service records each D-Bus client from an idle that its message
# filter queues, and nothing orders that idle before the client's first call.
# A call that comes first aborts the daemon (an OpenSession with no reply, or
# a property read with no value and no error), and the login keyring is
# locked from then on. GNOME Keyring merge request !112, for issues #190,
# #195 and #206: drop this once a release includes it.
final: prev: {
  gnome-keyring = prev.gnome-keyring.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../patches/gnome-keyring-client-record.patch ];
  });
}
