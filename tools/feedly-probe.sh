#!/usr/bin/env bash
# M0: Feedly-Endpoint-Probe (Spezifikation §12.2/§12.4).
# Liest den persönlichen Entwicklertoken aus ~/.config/lesefluss/feedly-token.
# Gibt NUR HTTP-Status und anonyme Strukturinfos aus; niemals Token oder
# Artikelinhalte ins Log schreiben. Ergebnis -> docs/feedly-api-vertrag.md
set -u
TOKEN_FILE="${FEEDLY_TOKEN_FILE:-$HOME/.config/lesefluss/feedly-token}"
BASE="${FEEDLY_BASE:-https://feedly.com}"

if [[ ! -r "$TOKEN_FILE" ]]; then
  echo "Token-Datei nicht lesbar: $TOKEN_FILE" >&2
  exit 1
fi
TOKEN=$(<"$TOKEN_FILE")
AUTH="Authorization: Bearer $TOKEN"

probe() { # methode pfad [label]
  local m="$1" p="$2" label="${3:-$p}"
  local out code
  out=$(curl -sS --max-time 20 -o /tmp/feedly-probe-body -w "%{http_code}" \
        -X "$m" -H "$AUTH" -H "Content-Type: application/json" \
        ${4:+-d "$4"} "$BASE$p")
  code="$out"
  local size
  size=$(wc -c < /tmp/feedly-probe-body)
  printf "%-6s %-40s -> %s (%s Bytes)\n" "$m" "$label" "$code" "$size"
  if [[ "$code" == 200 ]]; then
    python3 - "$label" <<'EOF'
import json,sys
label=sys.argv[1]
try:
    d=json.load(open('/tmp/feedly-probe-body'))
except Exception as e:
    print(f"  {label}: kein JSON ({e})"); sys.exit()
def keys(o):
    if isinstance(o,dict): return sorted(o.keys())
    if isinstance(o,list) and o: return [f"[{len(o)}x]", *keys(o[0])]
    if isinstance(o,list): return ["[]"]
    return [type(o).__name__]
print(f"  Struktur: {keys(d)[:20]}")
EOF
  fi
}

echo "Basis: $BASE"
echo "== Identität"
probe GET /v3/profile "GET /v3/profile"

echo "== Abonnements/Kategorien"
probe GET /v3/subscriptions "GET /v3/subscriptions"
probe GET /v3/categories "GET /v3/categories"
probe GET /v3/opml "GET /v3/opml"

echo "== Streams (max. 3 Artikel, anonymisierte Struktur)"
probe GET "/v3/streams/contents?streamId=user%2F-\\/category\\/global.all&count=3" "GET /v3/streams/contents (global.all)"
probe GET "/v3/streams/contents?streamId=user%2F-\\/tag\\/global.saved&count=3" "GET /v3/streams/contents (global.saved)"
probe GET "/v3/streams/ids?streamId=user%2F-\\/category\\/global.all&count=5" "GET /v3/streams/ids"
probe GET "/v3/markers/counts" "GET /v3/markers/counts"
probe GET "/v3/markers/reads?count=5" "GET /v3/markers/reads"
probe GET "/v3/markers/unreads?count=5" "GET /v3/markers/unreads"

echo "== Schreiboperationen (nur mit Testkonto!)"
if [[ "${FEEDLY_PROBE_WRITE:-0}" == "1" ]]; then
  ENTRY_ID="${FEEDLY_TEST_ENTRY_ID:-}"
  if [[ -n "$ENTRY_ID" ]]; then
    ENC=$(python3 -c "import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1],safe=''))" "$ENTRY_ID")
    probe POST /v3/markers "" "{\"action\":\"markAsRead\",\"type\":\"entries\",\"entryIds\":[\"$ENTRY_ID\"]}"
    probe POST /v3/markers "" "{\"action\":\"keepUnread\",\"type\":\"entries\",\"entryIds\":[\"$ENTRY_ID\"]}"
    probe POST "/v3/tags/user%2F-\\/tag\\/global.saved/$ENC" "saved: taggen"
    probe DELETE "/v3/tags/user%2F-\\/tag\\/global.saved/$ENC" "saved: entfernen"
  else
    echo "FEEDLY_TEST_ENTRY_ID nicht gesetzt; Schreibtests übersprungen."
  fi
else
  echo "Übersprungen (FEEDLY_PROBE_WRITE=1 zum Aktivieren)."
fi
rm -f /tmp/feedly-probe-body
