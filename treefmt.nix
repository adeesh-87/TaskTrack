# treefmt.nix
{ pkgs, ... }:
{
  # Used to find the project root
  projectRootFile = "flake.nix";
  programs.rustfmt.enable = true;
  programs.nixfmt.enable = true;
  programs.taplo.enable = true;
}
