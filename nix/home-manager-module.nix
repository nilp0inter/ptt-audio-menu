{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.ptt-audio-menu;
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
  options.programs.ptt-audio-menu = {
    enable = mkEnableOption "ptt-audio-menu";

    package = mkOption {
      type = types.package;
      default = package;
      defaultText = literalExpression "pkgs.callPackage ./nix/package.nix { }";
      description = "Package providing the ptt-audio-menu executable.";
    };

    configPath = mkOption {
      type = types.nullOr types.path;
      default = null;
      example = literalExpression ''"${config.xdg.configHome}/ptt-audio-menu/config.toml"'';
      description = "Optional TOML configuration path passed with --config.";
    };

    environment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = literalExpression ''{ RUST_LOG = "ptt_audio_menu=debug,info"; }'';
      description = "Extra environment variables for the user service.";
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

    service.enable = mkEnableOption "a user-level systemd service for ptt-audio-menu";
  };

  config = mkIf cfg.enable {
    home.packages = [ cfg.package ];

    systemd.user.services.ptt-audio-menu = mkIf cfg.service.enable {
      Unit = {
        Description = "PTT Audio Menu";
        After = [ "bluetooth.target" ];
      };

      Install.WantedBy = [ "default.target" ];

      Service = {
        ExecStart = lib.escapeShellArgs ([ (getExe cfg.package) ] ++ configArgs ++ cfg.extraArgs);
        Restart = "on-failure";
        RestartSec = "2s";
        Environment =
          lib.mapAttrsToList (name: value: "${name}=${value}") (
            {
              PIPER_ESPEAKNG_DATA_DIRECTORY = "${pkgs.espeak-ng}/share";
              RUST_LOG = cfg.logLevel;
            }
            // cfg.environment
          );
      };
    };
  };
}
