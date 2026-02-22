#!/usr/bin/env bash

set -euo pipefail

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI is required to clean release assets, but was not found in PATH." >&2
  exit 1
fi

if [ -z "${RELEASE_TAG:-}" ]; then
  echo "RELEASE_TAG is required." >&2
  exit 1
fi

if [ -z "${GITHUB_REPOSITORY:-}" ]; then
  echo "GITHUB_REPOSITORY is required." >&2
  exit 1
fi

release_lookup_err="$(mktemp)"
release_id=""
if release_id="$(
  gh api "repos/${GITHUB_REPOSITORY}/releases/tags/${RELEASE_TAG}" \
    --jq '.id' 2>"${release_lookup_err}"
)"; then
  :
else
  if grep -q "HTTP 404" "${release_lookup_err}"; then
    release_id=""
  else
    echo "Failed to resolve release ${RELEASE_TAG} from ${GITHUB_REPOSITORY}:" >&2
    cat "${release_lookup_err}" >&2
    rm -f "${release_lookup_err}"
    exit 1
  fi
fi
rm -f "${release_lookup_err}"

if [ -z "${release_id}" ]; then
  echo "Release ${RELEASE_TAG} does not exist yet. No assets to clean."
  exit 0
fi

deleted_count=0
while IFS=$'\t' read -r asset_id asset_name; do
  [ -n "${asset_id}" ] || continue
  gh api -X DELETE "repos/${GITHUB_REPOSITORY}/releases/assets/${asset_id}" >/dev/null
  echo "Deleted existing release asset: id=${asset_id}, name=${asset_name}"
  deleted_count=$((deleted_count + 1))
done < <(
  gh api --paginate "repos/${GITHUB_REPOSITORY}/releases/${release_id}/assets?per_page=100" \
    --jq 'if type == "array" then .[] else empty end | [.id, .name] | @tsv'
)

if [ "${deleted_count}" -eq 0 ]; then
  echo "Release ${RELEASE_TAG} has no existing assets."
fi
