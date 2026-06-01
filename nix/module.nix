{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.cyrene;
in {
  options.services.cyrene = {
    enable = mkEnableOption "Cyrene autonomous AI agent";

    package = mkOption {
      type = types.package;
      default = pkgs.cyrene;
      description = "The Cyrene package to use.";
    };

    dataDir = mkOption {
      type = types.path;
      default = "/var/lib/cyrene";
      description = "Directory for Cyrene runtime data (SQLite DB, state, ledger).";
    };

    user = mkOption {
      type = types.str;
      default = "cyrene";
      description = "User account under which Cyrene runs.";
    };

    group = mkOption {
      type = types.str;
      default = "cyrene";
      description = "Group under which Cyrene runs.";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = "Whether to open the dashboard port (8080) in the firewall.";
    };
  };

  config = mkIf cfg.enable {
    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
      home = cfg.dataDir;
      createHome = true;
    };
    users.groups.${cfg.group} = {};

    systemd.services.cyrene = {
      description = "Cyrene autonomous AI agent";
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        User = cfg.user;
        Group = cfg.group;
        ExecStart = "${cfg.package}/bin/cyrene gateway";
        Restart = "on-failure";
        RestartSec = 5;
        StateDirectory = "cyrene";
        WorkingDirectory = cfg.dataDir;
        EnvironmentFile = mkIf (cfg.dataDir != null) "${cfg.dataDir}/.env";
      };
    };

    networking.firewall.allowedTCPPorts = mkIf cfg.openFirewall [ 8080 ];
  };
}
