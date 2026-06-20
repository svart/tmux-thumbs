#!/usr/bin/env bash
set -Eeu -o pipefail

# Setup env variables to be compatible with compiled and bundled installations
CURRENT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
RELEASE_DIR="${CURRENT_DIR}/target/release"

THUMBS_BINARY="${RELEASE_DIR}/thumbs"
TMUX_THUMBS_BINARY="${RELEASE_DIR}/tmux-thumbs"
VERSION=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "${CURRENT_DIR}/Cargo.toml")

function install-binaries() {
  tmux split-window "cd ${CURRENT_DIR} && bash ./tmux-thumbs-install.sh"
  exit
}

function update-binaries() {
  tmux split-window "cd ${CURRENT_DIR} && bash ./tmux-thumbs-install.sh update"
  exit
}

function validate-binary() {
  local binary expected actual
  binary="${1}"; expected="${2}"

  if [ ! -x "${binary}" ]; then
    install-binaries
  fi

  actual="$(${binary} --version 2> /dev/null || true)"
  if [[ "${actual}" != "${expected}" ]]; then
    update-binaries
  fi
}

validate-binary "${THUMBS_BINARY}" "thumbs ${VERSION}"
validate-binary "${TMUX_THUMBS_BINARY}" "tmux-thumbs ${VERSION}"

function get-opt-value() {
  tmux show -vg "@thumbs-${1}" 2> /dev/null
}

function get-opt-arg() {
  local opt type value
  opt="${1}"; type="${2}"
  value="$(get-opt-value "${opt}")" || true

  if [ "${type}" = string ]; then
    [ -n "${value}" ] && echo "--${opt}=${value}"
  elif [ "${type}" = boolean ]; then
    [ "${value}" = 1 ] && echo "--${opt}"
  else
    return 1
  fi
}

PARAMS=(--dir "${CURRENT_DIR}")

function add-param() {
  local type opt arg
  opt="${1}"; type="${2}"
  if arg="$(get-opt-arg "${opt}" "${type}")"; then
    PARAMS+=("${arg}")
  fi
}

add-param command        string
add-param upcase-command string
add-param multi-command  string
add-param osc52          boolean

"${TMUX_THUMBS_BINARY}" "${PARAMS[@]}"
