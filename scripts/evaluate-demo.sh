#!/bin/bash
# scripts/evaluate-demo.sh — worked example of the embedded-API shape:
# caller's library function takes a request, returns a response, with
# AF_VSOCK as the data channel between host and guest.
#
# Builds:
#   - examples/evaluate.rs       library example, host side
#   - examples/guest_evaluator.rs guest side, packed as /init
# Boots a sequence of (input, output) pairs and prints PASS/FAIL.

set +e
LOG=/tmp/ingot-evaluate-demo.log
GUEST_BIN=/tmp/guest-evaluator
INITRAMFS=/tmp/eval-initramfs.cpio.gz
KERNEL=${KERNEL:-/tmp/kbuild/linux-6.6.30/vmlinux}
BUSYBOX=${BUSYBOX:-/tmp/busybox-static}
REPO=$(cd "$(dirname "$0")/.." && pwd)

exec > >(tee "$LOG") 2>&1

echo "================================================================"
echo "  ingot evaluate-demo  $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "================================================================"

[ -f "$KERNEL" ]  || { echo "MISSING: kernel"; exit 1; }
[ -x "$BUSYBOX" ] || { echo "MISSING: busybox-static"; exit 1; }

echo
echo "=== HOST: build guest evaluator (musl) + evaluate example ==="
rustc --edition 2021 -O --target x86_64-unknown-linux-musl \
      "$REPO/examples/guest_evaluator.rs" -o "$GUEST_BIN"
cd "$REPO" && cargo build --release --quiet --example evaluate

echo
echo "=== HOST: build initramfs ==="
ROOT=$(mktemp -d /tmp/ingot-eval-ramfs-XXXX)
mkdir -p "$ROOT"/{bin,dev,proc,sys}
fakeroot bash -s "$ROOT" "$INITRAMFS" "$GUEST_BIN" <<'FAKEROOT_EOF'
ROOT=$1; OUT=$2; BIN=$3
mknod $ROOT/dev/console c 5 1
mknod $ROOT/dev/null c 1 3
chmod 600 $ROOT/dev/console
chmod 666 $ROOT/dev/null
cp "$BIN" $ROOT/init
chmod +x $ROOT/init
cd $ROOT && find . -print0 | cpio --null -o -H newc 2>/dev/null | gzip > "$OUT"
FAKEROOT_EOF
ls -la "$INITRAMFS"

declare -A expected
expected[1]=0
expected[100]=25
expected[10000]=1229
expected[1000000]=78498

declare -A got
CID=100
echo
echo "=== running evaluate() with four inputs ==="
for N in 1 100 10000 1000000; do
  CID=$((CID+1))
  r=$(timeout 25 sg kvm -c "$REPO/target/release/examples/evaluate \
    '$KERNEL' '$INITRAMFS' $CID $N" 2>/dev/null)
  echo "  evaluate($N) -> $r  (expected ${expected[$N]})"
  got[$N]="$r"
  sleep 2
done

echo
echo "================================================================"
echo "                     PASS / FAIL SUMMARY                         "
echo "================================================================"
check() {
  local name=$1 cmd=$2
  if eval "$cmd" >/dev/null 2>&1; then
    echo "  [PASS]  $name"
  else
    echo "  [FAIL]  $name"
  fi
}
for N in 1 100 10000 1000000; do
  check "evaluate($N) = ${expected[$N]}" "[ '${got[$N]}' = '${expected[$N]}' ]"
done

echo "================================================================"
echo "Full transcript: $LOG"
