with import <nixpkgs> {};

stdenv.mkDerivation {
  name = "Pijul";
  buildInputs = with pkgs; [
    zstd
    libsodium
    openssl
    pkg-config
    libiconv
    xxHash
  ] ++ lib.optionals stdenv.isDarwin
    (with darwin.apple_sdk.frameworks; [
      CoreServices
      Security
      SystemConfiguration
    ]);
}
