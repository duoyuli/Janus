#!/usr/bin/env bash
# Prints the extra arguments `swift build --build-tests` and `swift test` need on this machine.
#
# With only the Command Line Tools installed (no Xcode), Swift Testing's macro plugin is not on
# the compiler's default plugin path and has to be named. `just test-mac` and CI's macOS job both
# use this, so the maintainer's own runner builds the tests the way a terminal on that machine does.
set -euo pipefail
clt=/Library/Developer/CommandLineTools
if [ "$(xcode-select -p)" = "$clt" ] && [ -d "$clt/usr/lib/swift/host/plugins/testing" ]; then
  echo "-Xswiftc -plugin-path -Xswiftc $clt/usr/lib/swift/host/plugins/testing"
fi
