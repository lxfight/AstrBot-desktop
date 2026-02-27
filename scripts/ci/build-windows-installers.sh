#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root_dir="$(cd "${script_dir}/../.." && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required to build Windows installers." >&2
  exit 1
fi

if ! (
  cd "${root_dir}"
  cargo tauri -V >/dev/null 2>&1
); then
  echo "Tauri CLI is required to build Windows installers (expected: cargo tauri)." >&2
  exit 1
fi

bundles="${ASTRBOT_WINDOWS_BUNDLES:-nsis}"
if [ -z "${bundles}" ]; then
  echo "ASTRBOT_WINDOWS_BUNDLES is empty. Expected a comma-separated bundle list." >&2
  exit 1
fi

max_attempts="${ASTRBOT_WINDOWS_BUILD_MAX_ATTEMPTS:-3}"
retry_sleep_seconds="${ASTRBOT_WINDOWS_BUILD_RETRY_SLEEP_SECONDS:-8}"
max_attempts_upper_bound=6

# Retry only for transient network failures (CDN 5xx, rate limits, timeouts).
transient_retry_pattern='http status: 50[0-9]|http status: 429|spurious network error|network failure|Operation timed out|Connection reset|Temporary failure in name resolution'

case "${max_attempts}" in
  ''|*[!0-9]*|0) max_attempts=3 ;;
esac
if [ "${max_attempts}" -gt "${max_attempts_upper_bound}" ]; then
  max_attempts="${max_attempts_upper_bound}"
fi
case "${retry_sleep_seconds}" in
  ''|*[!0-9]*|0) retry_sleep_seconds=8 ;;
esac

echo "Building Windows installers with bundles: ${bundles} (max_attempts=${max_attempts})"

for attempt in $(seq 1 "${max_attempts}"); do
  build_log="$(mktemp -t tauri-windows-build.XXXXXX.log)"
  (
    cd "${root_dir}"
    if [ -n "${ASTRBOT_TAURI_CONFIG_PATH:-}" ]; then
      echo "Using Tauri config: ${ASTRBOT_TAURI_CONFIG_PATH}"
      cargo tauri build --config "${ASTRBOT_TAURI_CONFIG_PATH}" --bundles "${bundles}"
    else
      echo "No Tauri config override, using default tauri.conf.json"
      cargo tauri build --bundles "${bundles}"
    fi
  ) 2>&1 | tee "${build_log}" && rm -f "${build_log}" && exit 0

  if [ "${attempt}" -ge "${max_attempts}" ]; then
    echo "Windows build failed after ${max_attempts} attempts." >&2
    rm -f "${build_log}" || true
    exit 1
  fi

  if ! grep -Eiq "${transient_retry_pattern}" "${build_log}"; then
    echo "Windows build failed with non-transient error on attempt ${attempt}/${max_attempts}; skip retries." >&2
    rm -f "${build_log}" || true
    exit 1
  fi

  rm -f "${build_log}" || true
  echo "Windows build hit transient failure on attempt ${attempt}/${max_attempts}; retrying in ${retry_sleep_seconds}s..."
  sleep "${retry_sleep_seconds}"
done
