#!/usr/bin/env bash
set -euo pipefail

OWNER="himicoswilson"
REPO="orgplug"
BIN_NAME="orgplug"
INSTALL_DIR="${HOME}/.local/bin"
STATE_DIR="${HOME}/.orgplug"
WORKDIR="${STATE_DIR}/workdir/orgplug"
CONFIG_FILE="${STATE_DIR}/config.yaml"
DEFAULT_CONFIG_URL="https://raw.githubusercontent.com/himicoswilson/orgplug/main/config/config.yaml"
REPO_URL_DEFAULT="https://github.com/himicoswilson/orgplug.git"
REPO_URL="${ORG_PLUG_REPO_URL:-$REPO_URL_DEFAULT}"
VERSION="${ORG_PLUG_VERSION:-latest}"

need_cmd() { command -v "$1" >/dev/null 2>&1 || { echo "Missing required command: $1" >&2; exit 1; }; }
need_cmd curl; need_cmd tar; need_cmd uname; need_cmd git

os="$(uname -s)"; arch="$(uname -m)"
case "$os" in Darwin) os_slug="darwin";; Linux) os_slug="linux";; *) echo "Unsupported OS: $os" >&2; exit 1;; esac
case "$arch" in arm64|aarch64) arch_slug="arm64";; x86_64|amd64) arch_slug="amd64";; *) echo "Unsupported architecture: $arch" >&2; exit 1;; esac

if [ "$VERSION" = "latest" ]; then
  release_base="https://github.com/${OWNER}/${REPO}/releases/latest/download"
else
  release_base="https://github.com/${OWNER}/${REPO}/releases/download/${VERSION}"
fi

asset="${BIN_NAME}-${os_slug}-${arch_slug}.tar.gz"
tmp_dir="$(mktemp -d)"; trap 'rm -rf "$tmp_dir"' EXIT

is_tty=0
if [ -t 1 ]; then
  is_tty=1
fi

run_step() {
  local label="$1"
  shift
  local log_file="$tmp_dir/step.log"

  if [ "$is_tty" -eq 1 ]; then
    "$@" >"$log_file" 2>&1 &
    local pid=$!
    local spin='|/-\\'
    local i=0
    while kill -0 "$pid" 2>/dev/null; do
      printf '\r[%c] %s' "${spin:i++%${#spin}:1}" "$label"
      sleep 0.1
    done
    wait "$pid"
    local status=$?
    if [ "$status" -eq 0 ]; then
      printf '\r[ok] %s\n' "$label"
      return 0
    fi
    printf '\r[fail] %s\n' "$label" >&2
    cat "$log_file" >&2
    exit "$status"
  fi

  echo "- $label"
  if "$@" >"$log_file" 2>&1; then
    return 0
  fi
  echo "[fail] $label" >&2
  cat "$log_file" >&2
  exit 1
}

run_step "Downloading release binary" curl -fsSL "${release_base}/${asset}" -o "${tmp_dir}/${asset}"
run_step "Preparing install directories" mkdir -p "$INSTALL_DIR" "$STATE_DIR/workdir"
run_step "Extracting release archive" tar -xzf "${tmp_dir}/${asset}" -C "$tmp_dir"

[ -f "${tmp_dir}/${BIN_NAME}" ] || { echo "Binary ${BIN_NAME} not found in archive" >&2; exit 1; }
run_step "Installing orgplug binary" install -m 0755 "${tmp_dir}/${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"

if [ -d "$WORKDIR/.git" ]; then
  run_step "Updating managed workdir" git -C "$WORKDIR" fetch --all --prune
  run_step "Fast-forwarding managed workdir" sh -c "git -C '$WORKDIR' pull --ff-only || true"
else
  run_step "Cloning managed workdir" sh -c "rm -rf '$WORKDIR' && git clone '$REPO_URL' '$WORKDIR'"
fi

run_step "Syncing submodules" git -C "$WORKDIR" submodule sync --recursive
run_step "Updating submodules" git -C "$WORKDIR" submodule update --init --recursive

if [ ! -f "$CONFIG_FILE" ]; then
  if ! curl -fsSL "$DEFAULT_CONFIG_URL" -o "$CONFIG_FILE"; then
    cat > "$CONFIG_FILE" <<'YAML'
version: 1

rules:
  repos:
    plugins/anthropics-skills:
      skills:
        deny: []

    plugins/knowledge-work-plugins:
      plugins:
        deny: []
YAML
  fi
fi

case ":${PATH}:" in *":${INSTALL_DIR}:"*) ;; *) echo "Add ${INSTALL_DIR} to PATH:"; echo '  export PATH="$HOME/.local/bin:$PATH"';; esac

echo "Installed ${BIN_NAME} to ${INSTALL_DIR}/${BIN_NAME}"
echo "Workdir: ${WORKDIR}"
echo "Config: ${CONFIG_FILE}"
echo "Run: ${BIN_NAME} doctor"
