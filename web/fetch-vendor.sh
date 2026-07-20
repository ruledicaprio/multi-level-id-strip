#!/usr/bin/env bash
# Fetch + SHA-256-verify the browser demo's OCR runtime into web/vendor/ —
# the same pin-and-verify pattern the native pipeline uses for its .rten
# models. Run by the Pages deploy workflow (and locally for dev); the files
# are deliberately NOT committed to git. After this, the deployed site makes
# zero CDN requests: tesseract.js, its worker, both LSTM cores, and the eng
# traineddata are all served same-origin.
set -euo pipefail
cd "$(dirname "$0")"

TESS_VERSION=5.1.1
CORE_VERSION=5.1.1
ENG_DATA_VERSION=1.0.0

mkdir -p vendor

fetch() { # fetch <url> <dest> <sha256>
  local url=$1 dest=$2 sum=$3
  if [ -f "$dest" ] && echo "$sum  $dest" | sha256sum -c - >/dev/null 2>&1; then
    echo "cached  $dest"
    return
  fi
  curl -fsSL --retry 3 -o "$dest" "$url"
  echo "$sum  $dest" | sha256sum -c -
}

fetch "https://cdn.jsdelivr.net/npm/tesseract.js@$TESS_VERSION/dist/tesseract.min.js" \
  vendor/tesseract.min.js \
  a8e29918d098b2b06e1012bdaeffb4aec0445c5d5654709023e0bd1f442a80e8
fetch "https://cdn.jsdelivr.net/npm/tesseract.js@$TESS_VERSION/dist/worker.min.js" \
  vendor/worker.min.js \
  aca1229639fc9907d86f96e825955a2b7c5716d17f3bc3acd71f9c7ab66181fc
fetch "https://cdn.jsdelivr.net/npm/tesseract.js-core@$CORE_VERSION/tesseract-core-simd-lstm.wasm.js" \
  vendor/tesseract-core-simd-lstm.wasm.js \
  ce20eda9533cbed1e6c2b4276fbae1e0adc61b6754b5513084be601787b457cf
fetch "https://cdn.jsdelivr.net/npm/tesseract.js-core@$CORE_VERSION/tesseract-core-lstm.wasm.js" \
  vendor/tesseract-core-lstm.wasm.js \
  8f04aa0cc81e7bde33f80e92fa01a7a665f0b4884d098acf5de9c7104a11dfaa
# eng traineddata joins the vendored mrz.traineddata in tessdata/ (the
# 4.0.0_best_int build tesseract.js v5's default langPath points at).
fetch "https://cdn.jsdelivr.net/npm/@tesseract.js-data/eng@$ENG_DATA_VERSION/4.0.0_best_int/eng.traineddata.gz" \
  tessdata/eng.traineddata.gz \
  45b4cb346724ac1774f1c36f42f182b887bcdb28ebe63e6fff90ac41f3fcff91

echo "vendor assets ready"
