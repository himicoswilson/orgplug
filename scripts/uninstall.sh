#!/usr/bin/env bash
set -euo pipefail

BIN_NAME="orgplug"
INSTALL_DIR="${HOME}/.local/bin"
STATE_DIR="${HOME}/.orgplug"
BIN_PATH="${INSTALL_DIR}/${BIN_NAME}"
PURGE=1

if [ "${1:-}" = "--keep-state" ]; then
  PURGE=0
fi

if [ -f "$BIN_PATH" ]; then
  rm -f "$BIN_PATH"
  echo "Removed ${BIN_PATH}"
else
  echo "Binary not found: ${BIN_PATH}"
fi

if [ "$PURGE" -eq 1 ]; then
  if [ -d "$STATE_DIR" ]; then
    rm -rf "$STATE_DIR"
    echo "Removed ${STATE_DIR}"
  else
    echo "State directory not found: ${STATE_DIR}"
  fi
else
  echo "Kept ${STATE_DIR}"
  echo "Run with --keep-state to keep managed state and config"
fi

echo "Uninstall complete"
