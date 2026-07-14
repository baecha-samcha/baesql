#!/usr/bin/env bash
set -euo pipefail

REPO="${BAESQL_GITHUB_REPO:-baecha-samcha/baesql}"
BASE_URL="https://github.com/${REPO}/releases/latest/download"
DEB_NAME="baesql-linux-arm64.deb"
SHA_NAME="${DEB_NAME}.sha256"

arch="$(uname -m)"
case "${arch}" in
  aarch64|arm64)
    ;;
  *)
    echo "error: unsupported architecture '${arch}'. BaeSQL release packages are built for arm64." >&2
    exit 1
    ;;
esac

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "error: required command '$1' not found" >&2
    exit 1
  fi
}

require_cmd curl
require_cmd sha256sum
require_cmd apt
require_cmd findmnt

if [ "$(id -u)" -eq 0 ]; then
  SUDO=""
else
  require_cmd sudo
  SUDO="sudo"
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

cd "${tmp_dir}"
curl -fL "${BASE_URL}/${DEB_NAME}" -o "${DEB_NAME}"
curl -fL "${BASE_URL}/${SHA_NAME}" -o "${SHA_NAME}"
sha256sum -c "${SHA_NAME}"

if findmnt -rn /srv/storage >/dev/null 2>&1; then
  ${SUDO} install -d -m 0755 /srv/storage/baesql
  ${SUDO} install -d -m 0755 /etc/baesql
  if [ ! -e /etc/baesql/config.toml ]; then
    config_tmp="$(mktemp)"
    cat > "${config_tmp}" <<'EOF'
data_dir = "/srv/storage/baesql"
default_database = "main.bae"
EOF
    ${SUDO} install -m 0644 "${config_tmp}" /etc/baesql/config.toml
    rm -f "${config_tmp}"
  else
    echo "info: /etc/baesql/config.toml already exists; leaving it unchanged"
  fi
else
  echo "warning: /srv/storage is not mounted; BaeSQL will use \$HOME/.local/share/baesql by default" >&2
fi

${SUDO} apt install -y "./${DEB_NAME}"
