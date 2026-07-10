#!/bin/sh
set -eu

secret_file=/app/config/secret-key

if [ -z "${SECRET_KEY:-}" ]; then
  if [ -s "$secret_file" ]; then
    SECRET_KEY=$(cat "$secret_file")
  else
    umask 077
    SECRET_KEY=$(od -An -N32 -tx1 /dev/urandom | tr -d ' \n')
    printf '%s\n' "$SECRET_KEY" > "$secret_file"
  fi
  export SECRET_KEY
fi

case "$0" in
  */lab-safety-system)
    set -- /usr/local/bin/lab-safety-system.bin "$@"
    ;;
esac

if [ "${1:-}" = "lab-safety-system" ]; then
  shift
  set -- /usr/local/bin/lab-safety-system.bin "$@"
fi

exec "$@"
