{
  description = "Rogue flake";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
  };

  outputs = { self, nixpkgs }: let 
    pkgs = nixpkgs.legacyPackages.x86_64-linux;
  in {
    packages.x86_64-linux.rogue-runtime = pkgs.callPackage ./nix/rogue-runtime.nix {};
    packages.x86_64-linux.default = self.packages.x86_64-linux.rogue-runtime;
  };
}
