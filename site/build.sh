#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
DIST="${SCRIPT_DIR}/_dist"
ASSETS="${DIST}/assets"

rm -rf "${DIST}"
mkdir -p "${ASSETS}"

cp "${SCRIPT_DIR}/index.html" "${SCRIPT_DIR}/style.css" "${SCRIPT_DIR}/script.js" "${DIST}/"
touch "${DIST}/.nojekyll"

copy_asset() {
  local src="$1"
  local dest_name="$2"
  local full_src="${REPO_ROOT}/${src}"
  if [[ ! -f "${full_src}" ]]; then
    echo "build: missing source ${src}" >&2
    exit 1
  fi
  cp "${full_src}" "${ASSETS}/${dest_name}"
}

# Steam screenshots → flat asset names
for name in alpine-peloton-1080 desert-volcano-1080 workout-1080 progress-1080 title-1080; do
  copy_asset "docs/img/shots/steam/${name}.png" "${name}.png"
done

# Atmosphere strip
for name in alpine-dawn alpine-sunset alpine-night alpine-rain alpine-snow; do
  copy_asset "docs/img/world/atmosphere/${name}.png" "${name}.png"
done

# Brand marks
copy_asset "docs/img/banner.svg" "banner.svg"
copy_asset "docs/img/logo.svg" "logo.svg"
copy_asset "packaging/steam/assets/shortcut-icon-256x256.png" "shortcut-icon-256x256.png"
copy_asset "packaging/steam/assets/header-capsule-920x430.png" "header-capsule-920x430.png"

echo "build: verifying asset references in index.html"
missing=0
while IFS= read -r ref; do
  [[ -z "${ref}" ]] && continue
  path="${DIST}/${ref}"
  if [[ ! -e "${path}" ]]; then
    echo "build: missing referenced file ${ref}" >&2
    missing=$((missing + 1))
  fi
done < <(grep -oE 'assets/[a-zA-Z0-9_.-]+' "${DIST}/index.html" | sort -u)

if [[ "${missing}" -gt 0 ]]; then
  echo "build: ${missing} asset(s) missing" >&2
  exit 1
fi

echo "build: OK — output at ${DIST}"
ls -la "${ASSETS}"