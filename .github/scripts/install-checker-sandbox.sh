#!/usr/bin/env bash
# Ubuntu 22.04's bubblewrap predates the --size option used to bound the writable tmpfs.
set -euo pipefail
version=0.12.0
digest=9760d007363e3abba7c747489910f9f82d9fca53ba3bd3282e396fa3c97a3314
scratch="$(mktemp -d)"
trap 'rm -rf -- "${scratch}"' EXIT
archive="bubblewrap-${version}.tar.xz"
sudo apt-get update -qq
sudo apt-get install --no-install-recommends -y libcap-dev meson ninja-build
curl --fail --silent --show-error --location --proto '=https' --proto-redir '=https' \
  "https://github.com/containers/bubblewrap/releases/download/v${version}/${archive}" \
  --output "${scratch}/${archive}"
(
  cd "${scratch}"
  printf '%s  %s\n' "${digest}" "${archive}" | sha256sum --check --strict
  tar -xf "${archive}"
  meson setup build "bubblewrap-${version}" -Dtests=false -Dman=disabled -Dselinux=disabled
  meson compile -C build
  sudo install -m 0755 build/bwrap /usr/bin/bwrap
)
/usr/bin/bwrap --version
geometry="$(/usr/bin/bwrap --unshare-all --new-session --die-with-parent --clearenv --cap-drop ALL \
  --ro-bind / / --proc /proc --dev /dev --size 67108864 --tmpfs /tmp -- \
  /usr/bin/prlimit --cpu=8 --as=1600000000 --nproc=64 --fsize=33554432 --nofile=128 --core=0 -- \
  /usr/bin/stat -f --format='%S %b' /tmp)"
read -r block_size blocks <<< "${geometry}"
((block_size * blocks == 67108864))
