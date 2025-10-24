# Maintainer: Sina Afsharmanesh
_pkgname=scx-power-sync-dbus
pkgname=${_pkgname}-git
pkgver=0.1.0.1.g3db286d
pkgrel=1
pkgdesc="Event-driven SCX scheduler binder for power-profiles-daemon (Rust, zbus)"
arch=('x86_64')
url="https://github.com/Sina-Afsharmanesh/${_pkgname}"
license=('MIT')
depends=('power-profiles-daemon')
optdepends=('scxctl: sched_ext control CLI required at runtime')
makedepends=('rustup' 'clang' 'lld' 'pkgconf' 'git')
provides=("${_pkgname}")
conflicts=("${_pkgname}" "${_pkgname}-toolchain-nightly-git")
source=("${_pkgname}::git+https://github.com/Sina-Afsharmanesh/${_pkgname}.git#branch=master")
sha256sums=('SKIP')

_toolchain="stable"

pkgver() {
  cd "${srcdir}/${_pkgname}"
  if ver="$(git describe --tags --long 2>/dev/null)"; then
    printf '%s\n' "${ver#v}" | sed 's/-/./g'
  else
    printf 'r%s.%s\n' "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
  fi
}

prepare() {
  cd "${srcdir}/${_pkgname}"

  rustup toolchain install "${_toolchain}" --profile minimal --component rust-src

  sed -i 's|^ExecStart=.*|ExecStart=/usr/bin/scx-power-sync-dbus|' \
    "contrib/${_pkgname}.service"

  rustup run "${_toolchain}" rustc -V
  rustup run "${_toolchain}" cargo -V
}

build() {
  cd "${srcdir}/${_pkgname}"

  local cargo_flags=()
  [[ -f Cargo.lock ]] && cargo_flags+=(--locked)

  RUSTUP_TOOLCHAIN="${_toolchain}" \
  cargo build --release "${cargo_flags[@]}"
}

package() {
  cd "${srcdir}/${_pkgname}"

  install -Dm755 "target/release/${_pkgname}" "${pkgdir}/usr/bin/${_pkgname}"

  install -Dm644 "contrib/${_pkgname}.service" \
    "${pkgdir}/usr/lib/systemd/user/${_pkgname}.service"

  install -Dm644 LICENSE "${pkgdir}/usr/share/licenses/${pkgname}/LICENSE"
}
