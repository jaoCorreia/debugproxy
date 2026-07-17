#!/bin/sh
# The release binary isn't notarized, so macOS Gatekeeper quarantines it on
# download and refuses to run it. This lifts that flag. Bundled next to the
# binary in each macOS release archive — run it once after extracting.
set -e
DIR="$(cd "$(dirname "$0")" && pwd)"
xattr -cr "$DIR/debugproxy"
echo "Quarantine flag removed. You can now run ./debugproxy"
