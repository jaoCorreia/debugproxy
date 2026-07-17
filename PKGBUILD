# Maintainer: Joao Correia <joao@example.com>
pkgname=debugproxy
pkgver=1.0.0
pkgrel=1
pkgdesc="HTTP proxy with ratatui TUI for debugging mobile and web applications"
arch=('x86_64')
url="https://github.com/anomalyco/debugproxy"
license=('MIT')
depends=('gcc-libs')
makedepends=('cargo')
source=("$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

build() {
  cd "$srcdir/$pkgname-$pkgver"
  cargo build --release --frozen
}

package() {
  cd "$srcdir/$pkgname-$pkgver"
  install -Dm755 target/release/debugproxy "$pkgdir/usr/bin/debugproxy"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
