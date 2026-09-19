#!/usr/bin/env sh
set -eu
cd "$(dirname "$0")/.."
rm -rf .pure
mkdir .pure
# Uses an installed TypeScript compiler; no invented Vue/Pinia declaration stubs.
tsc src/contracts.ts src/time.ts src/listenerScope.ts --strict --target ES2022 --module ES2022 --moduleResolution bundler --noUncheckedIndexedAccess --exactOptionalPropertyTypes --skipLibCheck --outDir .pure
node tests/pure.mjs
