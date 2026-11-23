# The TV account can have a short PIN as its password. Its nologin shell
# alone still allows SSH forwarding. Deny its group while the account
# exists, including after the desktop or Chanoma has been turned off.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  group = "aizome-tv-kiosk";
  ssh = config.services.openssh.enable;
  chanoma = config.programs.aizome.enable && config.programs.aizome.chanoma.enable;
in
{
  users.groups.${group} = lib.mkIf (chanoma || ssh) { };
  services.openssh.settings.DenyGroups = lib.mkIf ssh [ group ];

  # Settings adds newly created accounts to the group. Older installations
  # have the root-owned TV record but no membership yet. Restore it before
  # sshd starts with the new configuration.
  system.activationScripts.aizomeTvSsh = lib.mkIf ssh {
    deps = [ "users" ];
    text = ''
      record=/var/lib/aizome/tv-account.toml
      if [ -f "$record" ]; then
        tvuser=$(${pkgs.gnused}/bin/sed -n 's/^user = "\([a-z_][a-z0-9_-]*\)"$/\1/p' "$record")
        if [ -n "$tvuser" ] && tvgroups=$(${pkgs.coreutils}/bin/id -nG "$tvuser" 2>/dev/null); then
          case " $tvgroups " in
            *" ${group} "*) ;;
            *) ${pkgs.shadow}/bin/usermod --append --groups ${group} "$tvuser" ;;
          esac
        fi
      fi
    '';
  };
}
