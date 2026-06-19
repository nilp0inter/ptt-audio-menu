{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.ptt-audio-menu;
  inherit (lib)
    getExe
    literalExpression
    mkEnableOption
    mkIf
    mkOption
    optional
    types
    ;

  package = pkgs.callPackage ./package.nix { };
  configArgs = optional (cfg.configPath != null) "--config" ++ optional (cfg.configPath != null) cfg.configPath;
in
{
  options.services.ptt-audio-menu = {
    enable = mkEnableOption "the ptt-audio-menu Bluetooth audio menu service";

    package = mkOption {
      type = types.package;
      default = package;
      defaultText = literalExpression "pkgs.callPackage ./nix/package.nix { }";
      description = "Package providing the ptt-audio-menu executable.";
    };

    configPath = mkOption {
      type = types.nullOr types.path;
      default = null;
      example = "/etc/ptt-audio-menu/config.toml";
      description = "Optional TOML configuration path passed with --config.";
    };

    user = mkOption {
      type = types.str;
      default = "root";
      description = "User that runs the system service.";
    };

    group = mkOption {
      type = types.str;
      default = "root";
      description = "Group that runs the system service.";
    };

    supplementaryGroups = mkOption {
      type = types.listOf types.str;
      default = [
        "audio"
        "bluetooth"
      ];
      description = "Supplementary groups for Bluetooth and audio device access.";
    };

    environment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = literalExpression ''{ RUST_LOG = "ptt_audio_menu=debug,info"; }'';
      description = "Extra environment variables for the service.";
    };

    logLevel = mkOption {
      type = types.str;
      default = "info";
      description = "Default RUST_LOG value when environment.RUST_LOG is not set.";
    };

    extraArgs = mkOption {
      type = types.listOf types.str;
      default = [ ];
      example = [ "--help" ];
      description = "Additional command-line arguments appended after the config path.";
    };
  };

  config = mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];

    systemd.services.ptt-audio-menu = {
      description = "PTT Audio Menu";
      wantedBy = [ "multi-user.target" ];
      after = [
        "bluetooth.service"
        "sound.target"
      ];
      wants = [ "bluetooth.service" ];

      environment =
        {
          PIPER_ESPEAKNG_DATA_DIRECTORY = "${pkgs.espeak-ng}/share/espeak-ng-data";
          RUST_LOG = cfg.logLevel;
        }
        // cfg.environment;

      serviceConfig = {
        ExecStart = lib.escapeShellArgs ([ (getExe cfg.package) ] ++ configArgs ++ cfg.extraArgs);
        Restart = "on-failure";
        RestartSec = "2s";
        User = cfg.user;
        Group = cfg.group;
        SupplementaryGroups = cfg.supplementaryGroups;
      };
    };
  };
}
