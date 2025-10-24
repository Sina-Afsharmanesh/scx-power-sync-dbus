# Maintainer: Sina Afsharmanesh
pkgname=scx-power-sync-dbus
pkgver=0.1.0
pkgrel=2
pkgdesc="Event-driven SCX scheduler binder for power-profiles-daemon (Rust, zbus)"
arch=('x86_64')
#url=""
license=('MIT')
depends=('power-profiles-daemon' 'scxctl')
makedepends=('rustup' 'cargo' 'clang' 'lld' 'pkgconf' 'git')
source=("${pkgname}-${pkgver}.tar.gz")
sha256sums=('SKIP')

_stable_toolchain="stable"

prepare() {
  cd "${srcdir}/${pkgname}-${pkgver}"

  rustup toolchain install "${_stable_toolchain}" --profile minimal --component rust-src
  rustup run "${_stable_toolchain}" rustc -V
  rustup run "${_stable_toolchain}" cargo -V
}

build() {
  cd "${srcdir}/${pkgname}-${pkgver}"

  RUSTUP_TOOLCHAIN="${_stable_toolchain}" \
  cargo build --release --frozen
}

package() {
  cd "${srcdir}/${pkgname}-${pkgver}"

  install -Dm755 "target/release/${pkgname}" "${pkgdir}/usr/bin/${pkgname}"

  install -Dm644 "contrib/${pkgname}.service" \
    "${pkgdir}/usr/lib/systemd/user/${pkgname}.service"

  install -Dm644 LICENSE "${pkgdir}/usr/share/licenses/${pkgname}/LICENSE"
}
