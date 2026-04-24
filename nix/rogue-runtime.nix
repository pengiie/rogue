{ lib, 
  stdenv, 
  rustc, 
  cargo, 
  makeWrapper, 
  wayland,
  clang,
  xorg,
  libxkbcommon,
  vulkan-loader,
  vulkan-validation-layers,
  rustPlatform,
  shaderc,
  spirv-tools,
  alsa-lib,
  udev,
  mold,
  pango,
  atkmm,
  gdk-pixbuf,
  rubyPackages,
  gtk3,
  glib,
  openssl,
  libclang,
  shader-slang,
  pkg-config
}:

stdenv.mkDerivation rec {
  pname = "rogue-runtime";
  version = "0.1.0";

  src = ./..; 

  cargoDeps = rustPlatform.importCargoLock {
    lockFile = ../Cargo.lock;
    # If you have git dependencies, specify them:
    outputHashes = {
      "erased-serde-0.4.9" = "sha256-6w8yNE4T9kJ0GApmibQEb6qy1nlWY+4oZmgxDKXtau0=";
"shader-slang-0.1.0" = "sha256-EAb4X78+d3OEc80mm0uxp5E6Uy6d+DTL4zvds8rl6sk=";
      # Add other git deps here
    };
  };

  nativeBuildInputs = [
    rustc
    mold
    clang
    cargo
    makeWrapper
    rustPlatform.cargoSetupHook
    pkg-config
  ];

  inputsFrom = [
    wayland

    # X11 libraries
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    libxkbcommon

    # Vulkan libraries
    shaderc
    spirv-tools
    vulkan-loader
    vulkan-validation-layers
  ];
  buildInputs = [
    # Audio
    alsa-lib
    udev

    # File chooser
    pango
    atkmm
    gdk-pixbuf
    rubyPackages.gdk3
    gtk3
    glib

    wayland.dev
    openssl
    openssl.dev

    libclang
    shader-slang
  ];

  #CARGO_HOME = ".cargo";

  buildPhase = ''
    runHook preBuild
    
    export LD_LIBRARY_PATH=${lib.makeLibraryPath (inputsFrom ++ buildInputs)};
    export SHADERC_LIB_DIR=${lib.makeLibraryPath [ shaderc ]};
    export SLANG_DIR=${shader-slang.dev};
    export PKG_CONFIG_PATH=${lib.makeLibraryPath [ wayland.dev ]}/pkgconfig:$PKG_CONFIG_PATH;

    # Build the specific workspace member binary
    cargo build --release --bin rogue_runtime
    
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    
    # Copy binary and binary assets
    mkdir -p $out/bin
    cp target/release/rogue_runtime $out/bin
    cp -r assets $out/bin
    cp -r project_data $out/bin

    # Wrap the binary to set asset path if needed
    wrapProgram $out/bin/rogue-runtime \
      --set WAYLAND_DISPLAY= RUST_LOG=info "$out/share/rogue_runtime"
    
    runHook postInstall
  '';
}
