{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustup
  ];

  shellHook = ''
    if [ ! -d "$HOME/.rustup" ]; then
      echo "Initializing rustup..."
      rustup default stable
      rustup component add rust-analyzer
    fi
  '';
}
