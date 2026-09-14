# Maintainer: hollykbuck

pkgname=clashlime
pkgver=1.2.0
pkgrel=1
pkgdesc='Terminal dashboard for Mihomo on Omarchy'
arch=('x86_64')
url='https://github.com/hollykbuck/clashlime'
license=('GPL-3.0-only')
depends=()
optdepends=('mihomo: system core (or provide $CLASHLIME_MIHOMO / auto-download)'
            'clash-geoip: system GeoIP database (or auto-download)')
makedepends=('cargo')
options=('!lto')
source=("$pkgname-$pkgver.tar.gz::$url/archive/refs/tags/v$pkgver.tar.gz")
b2sums=('SKIP')

prepare() {
  cd "$pkgname-$pkgver"
  cargo fetch --locked --target "$CARCH-unknown-linux-gnu"
}

build() {
  cd "$pkgname-$pkgver"
  cargo build --frozen --release
}

check() {
  cd "$pkgname-$pkgver"
  cargo test --frozen
}

package() {
  cd "$pkgname-$pkgver"

  install -Dm755 target/release/clashlime "$pkgdir/usr/bin/clashlime"
  install -Dm644 systemd/clashlime-supervisor.service \
    "$pkgdir/usr/lib/systemd/user/clashlime-supervisor.service"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 themes/default.toml \
    "$pkgdir/usr/share/$pkgname/themes/default.toml"
  install -Dm644 integrations/omarchy/hollykbuck.clashlime/manifest.json \
    "$pkgdir/usr/share/$pkgname/omarchy/hollykbuck.clashlime/manifest.json"
  install -Dm644 integrations/omarchy/hollykbuck.clashlime/Panel.qml \
    "$pkgdir/usr/share/$pkgname/omarchy/hollykbuck.clashlime/Panel.qml"
  install -Dm644 integrations/omarchy/hollykbuck.clashlime/README.md \
    "$pkgdir/usr/share/$pkgname/omarchy/hollykbuck.clashlime/README.md"
}
