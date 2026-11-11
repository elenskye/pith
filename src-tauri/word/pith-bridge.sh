#!/bin/sh
# Pith Word bridge helper. Installed by Pith; do not edit (it is overwritten on update).
# Usage: Pith-bridge.sh <request file>. Prints the reply (first line OK or ERR).
# Reads the port and token of the running Pith from service.json; starts Pith in the
# background when it is not running.
DATA_DIR=@DATA_DIR@
LAUNCH_KIND=@LAUNCH_KIND@
LAUNCH=@LAUNCH@
DATA_ENV=@DATA_ENV@
REQ="$1"
INFO="$DATA_DIR/service.json"

call() {
  [ -f "$INFO" ] || return 7
  port=$(/usr/bin/plutil -extract port raw -o - "$INFO" 2>/dev/null) || return 7
  token=$(/usr/bin/plutil -extract token raw -o - "$INFO" 2>/dev/null) || return 7
  /usr/bin/curl -sS --fail --max-time 900 -H "Authorization: Bearer $token" \
    -H "Content-Type: application/json" -H "Expect:" \
    --data-binary "@$REQ" "http://127.0.0.1:$port/v1/word"
}

out=$(call 2>/dev/null)
code=$?
if [ $code -eq 0 ]; then printf '%s' "$out"; exit 0; fi
if [ $code -ne 7 ]; then
  printf 'ERR\tPith did not answer (curl error %s). Try again, or restart Pith.' "$code"
  exit 0
fi

# Pith is not running: start it in the background and wait for its service.
case "$LAUNCH_KIND" in
  app)
    if [ -n "$DATA_ENV" ]; then /usr/bin/open -g --env "PITH_DATA_DIR=$DATA_ENV" "$LAUNCH" >/dev/null 2>&1
    else /usr/bin/open -g "$LAUNCH" >/dev/null 2>&1; fi ;;
  bin)
    if [ -n "$DATA_ENV" ]; then (PITH_DATA_DIR="$DATA_ENV" nohup "$LAUNCH" >/dev/null 2>&1 &)
    else (nohup "$LAUNCH" >/dev/null 2>&1 &); fi ;;
  *)
    printf 'ERR\tPith is not running. Open Pith, then try again.'
    exit 0 ;;
esac
i=0
while [ $i -lt 120 ]; do
  sleep 0.5
  out=$(call 2>/dev/null) && { printf '%s' "$out"; exit 0; }
  i=$((i + 1))
done
printf 'ERR\tPith did not start in time. Open Pith, then try again.'
