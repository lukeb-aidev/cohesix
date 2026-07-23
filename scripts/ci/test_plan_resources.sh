#!/usr/bin/env bash
# Author: Lukas Bower
# Purpose: Keep test-plan build and test concurrency responsive on shared developer hosts.
# Copyright 2026 Lukas Bower

set -euo pipefail

tp_detect_logical_cpus() {
  local detected=""
  if command -v sysctl >/dev/null 2>&1; then
    detected="$(sysctl -n hw.logicalcpu 2>/dev/null || true)"
  fi
  if [[ ! "${detected}" =~ ^[1-9][0-9]*$ ]] &&
    command -v getconf >/dev/null 2>&1; then
    detected="$(getconf _NPROCESSORS_ONLN 2>/dev/null || true)"
  fi
  if [[ ! "${detected}" =~ ^[1-9][0-9]*$ ]] &&
    command -v nproc >/dev/null 2>&1; then
    detected="$(nproc 2>/dev/null || true)"
  fi
  if [[ ! "${detected}" =~ ^[1-9][0-9]*$ ]]; then
    detected=2
  fi
  printf "%s\n" "${detected}"
}

tp_default_parallelism() {
  local logical_cpus="$1"
  local jobs
  if ((logical_cpus <= 2)); then
    jobs=1
  else
    jobs=$(((logical_cpus + 1) / 2))
  fi
  ((jobs > 6)) && jobs=6
  printf "%s\n" "${jobs}"
}

tp_configure_resource_limits() {
  local logical_cpus
  logical_cpus="$(tp_detect_logical_cpus)"
  TP_HOST_JOBS="${TP_HOST_JOBS:-$(tp_default_parallelism "${logical_cpus}")}"
  if [[ ! "${TP_HOST_JOBS}" =~ ^[1-9][0-9]*$ ]] ||
    ((TP_HOST_JOBS > 64)); then
    printf "TP_HOST_JOBS must be an integer from 1 through 64, got: %s\n" \
      "${TP_HOST_JOBS}" >&2
    return 2
  fi

  if [[ "${TP_ALLOW_OVERSUBSCRIBE:-0}" != "1" ]] &&
    ((TP_HOST_JOBS > logical_cpus)); then
    printf "TP_HOST_JOBS=%s exceeds detected CPUs=%s; set TP_ALLOW_OVERSUBSCRIBE=1 for an intentional override\n" \
      "${TP_HOST_JOBS}" \
      "${logical_cpus}" >&2
    return 2
  fi

  if [[ "${TP_PRESERVE_PARALLEL_ENV:-0}" == "1" ]]; then
    CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-${TP_HOST_JOBS}}"
    CMAKE_BUILD_PARALLEL_LEVEL="${CMAKE_BUILD_PARALLEL_LEVEL:-${TP_HOST_JOBS}}"
    RAYON_NUM_THREADS="${RAYON_NUM_THREADS:-${TP_HOST_JOBS}}"
    RUST_TEST_THREADS="${RUST_TEST_THREADS:-${TP_HOST_JOBS}}"
  else
    CARGO_BUILD_JOBS="${TP_HOST_JOBS}"
    CMAKE_BUILD_PARALLEL_LEVEL="${TP_HOST_JOBS}"
    RAYON_NUM_THREADS="${TP_HOST_JOBS}"
    RUST_TEST_THREADS="${TP_HOST_JOBS}"
  fi
  local default_ui_workers=2
  ((TP_HOST_JOBS < 2)) && default_ui_workers=1
  TP_UI_WORKERS="${TP_UI_WORKERS:-${default_ui_workers}}"

  if [[ ! "${TP_UI_WORKERS}" =~ ^[1-9][0-9]*$ ]] ||
    ((TP_UI_WORKERS > TP_HOST_JOBS)); then
    printf "TP_UI_WORKERS must be between 1 and TP_HOST_JOBS=%s, got: %s\n" \
      "${TP_HOST_JOBS}" \
      "${TP_UI_WORKERS}" >&2
    return 2
  fi

  if [[ "${TP_PRESERVE_PARALLEL_ENV:-0}" != "1" ]]; then
    MAKEFLAGS="-j${TP_HOST_JOBS}"
  else
    MAKEFLAGS="${MAKEFLAGS:-}"
  fi

  export \
    CARGO_BUILD_JOBS \
    CMAKE_BUILD_PARALLEL_LEVEL \
    MAKEFLAGS \
    RAYON_NUM_THREADS \
    RUST_TEST_THREADS \
    TP_HOST_JOBS \
    TP_UI_WORKERS
}
