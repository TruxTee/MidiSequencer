#!/usr/bin/env bash
# Cross-builds the engine on this Windows workstation and deploys it to the Pi.
#   scripts/deploy-pi.sh             build, copy, keep the previous binary, restart
#   scripts/deploy-pi.sh --rollback  put the previous binary back and restart
# Also installs the config files and systemd units from pi/ and profiles/.
# Needs: rustup target arm-unknown-linux-gnueabihf, cargo-zigbuild, pip ziglang + pkgconf,
# and engine/.sysroot-armhf (the Pi's ALSA headers and libasound; see docs/SEQUENCER.md).
set -euo pipefail
PI="${PI:-flo@192.168.0.147}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SSH=(ssh -o BatchMode=yes "$PI")

if [[ "${1:-}" == "--rollback" ]]; then
  "${SSH[@]}" 'cd ~/seq/engine && test -f seq-engine.prev && mv seq-engine seq-engine.bad && mv seq-engine.prev seq-engine && mv seq-engine.bad seq-engine.prev && sudo systemctl restart seq-engine && systemctl is-active seq-engine'
  exit
fi

export PATH="$HOME/.cargo/bin:$LOCALAPPDATA/Microsoft/WinGet/Packages/BrechtSanders.WinLibs.POSIX.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe/mingw64/bin:$PATH"
SR="$(cygpath -m "$ROOT/engine/.sysroot-armhf")"
export CARGO_ZIGBUILD_ZIG_PATH="$(python -c 'import ziglang,os;print(os.path.join(os.path.dirname(ziglang.__file__),"zig.exe"))')"
export PKG_CONFIG="$(python -c 'import pkgconf;print(pkgconf.get_executable())')"
export PKG_CONFIG_ALLOW_CROSS=1 PKG_CONFIG_LIBDIR="$SR/pc"
# Debian trixie armhf builds libasound with a 64-bit time_t; alsa-sys must see the same.
export CFLAGS_arm_unknown_linux_gnueabihf="-D_TIME_BITS=64 -D_FILE_OFFSET_BITS=64"

cd "$ROOT/engine"
cargo zigbuild -q --release --target arm-unknown-linux-gnueabihf.2.41
BIN=target/arm-unknown-linux-gnueabihf/release/seq-engine

"${SSH[@]}" 'mkdir -p ~/seq/engine ~/seq/profiles'
scp -q "$BIN" "$PI:seq/engine/seq-engine.new"
scp -q routing.json "$PI:seq/engine/"
scp -q "$ROOT/profiles/"*.json "$PI:seq/profiles/"
scp -q "$ROOT/pi/rtpmidid.ini" "$ROOT/pi/rtpmidid.service" "$ROOT/pi/seq-engine.service" "$PI:/tmp/"
"${SSH[@]}" 'set -e
  cd ~/seq/engine && chmod +x seq-engine.new
  if [ -f seq-engine ]; then mv seq-engine seq-engine.prev; fi
  mv seq-engine.new seq-engine
  mv /tmp/rtpmidid.ini ~/seq/rtpmidid.ini
  for u in rtpmidid seq-engine; do
    sudo install -m644 /tmp/$u.service /etc/systemd/system/$u.service
  done
  sudo systemctl daemon-reload
  sudo systemctl enable -q rtpmidid seq-engine
  sudo systemctl restart rtpmidid seq-engine
  sleep 2
  systemctl is-active rtpmidid seq-engine'
