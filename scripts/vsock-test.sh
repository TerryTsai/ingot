#!/bin/bash
# scripts/vsock-test.sh — exercise virtio-vsock end to end.
#
# What it does:
#   1. Cross-compiles examples/guest_vsock_echo.rs (musl) and a host
#      AF_VSOCK client (host gnu).
#   2. Builds a busybox initramfs whose /init brings up basic mounts
#      and execs the vsock echo server as PID 1.
#   3. Boots ingot with --vsock-cid 42.
#   4. From the host, connects via AF_VSOCK to (CID=42, port 9000),
#      sends a payload, expects "ECHO: <payload>" back.
#   5. Prints PASS / FAIL summary.

set +e

LOG=/tmp/ingot-vsock-test.log
GUEST_LOG=/tmp/ingot-vsock-guest.log
INITRAMFS=/tmp/ingot-vsock-initramfs.cpio.gz
GUEST_BIN=/tmp/guest-vsock-echo
HOST_BIN=/tmp/host-vsock-client
KERNEL=${KERNEL:-/tmp/kbuild/linux-6.6.30/vmlinux}
BUSYBOX=${BUSYBOX:-/tmp/busybox-static}
REPO=$(cd "$(dirname "$0")/.." && pwd)
CID=42
PORT=9000

exec > >(tee "$LOG") 2>&1

echo "================================================================"
echo "  ingot vsock-test  $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "================================================================"

[ -f "$KERNEL" ]  || { echo "MISSING: kernel at $KERNEL"; exit 1; }
[ -x "$BUSYBOX" ] || { echo "MISSING: static busybox at $BUSYBOX"; exit 1; }

echo
echo "=== HOST: cross-compile guest binary (musl) + host client (python) ==="
rustc --edition 2021 -O --target x86_64-unknown-linux-musl \
      "$REPO/examples/guest_vsock_echo.rs" -o "$GUEST_BIN"
cat > "$HOST_BIN" <<'PY'
#!/usr/bin/env python3
# host AF_VSOCK client: ./host-vsock-client <cid> <port> <msg>
import socket, sys
cid, port, msg = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
s = socket.socket(socket.AF_VSOCK, socket.SOCK_STREAM)
s.settimeout(5)
s.connect((cid, port))
s.sendall((msg + "\n").encode())
data = s.recv(4096)
sys.stdout.write(data.decode(errors='replace'))
PY
chmod +x "$HOST_BIN"
ls -la "$GUEST_BIN" "$HOST_BIN"

echo
echo "=== HOST: build initramfs ==="
ROOT=$(mktemp -d /tmp/ingot-vsock-ramfs-XXXX)
mkdir -p "$ROOT"/{bin,dev,proc,sys}
cp "$BUSYBOX" "$ROOT/bin/busybox"
cp "$GUEST_BIN" "$ROOT/echo"
chmod +x "$ROOT/echo"
fakeroot bash -s "$ROOT" "$INITRAMFS" <<'FAKEROOT_EOF'
ROOT=$1; OUT=$2
mknod $ROOT/dev/console c 5 1
mknod $ROOT/dev/null c 1 3
mknod $ROOT/dev/ttyS0 c 4 64
mknod $ROOT/dev/vsock c 10 127
chmod 600 $ROOT/dev/console
chmod 666 $ROOT/dev/null $ROOT/dev/ttyS0 $ROOT/dev/vsock
cat > $ROOT/init <<'INIT'
#!/bin/busybox sh
/bin/busybox --install -s /bin
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null
exec /echo
INIT
chmod +x $ROOT/init
cd $ROOT && find . -print0 | cpio --null -o -H newc 2>/dev/null | gzip > "$OUT"
FAKEROOT_EOF
ls -la "$INITRAMFS"

echo
echo "=== launching guest ==="
cd "$REPO"
cargo build --release --quiet --bin ingot
sg kvm -c "./target/release/ingot run \
  --kernel '$KERNEL' \
  --initramfs '$INITRAMFS' \
  --memory 256 \
  --vsock-cid $CID \
  --cmdline 'console=ttyS0,115200 virtio_mmio.device=4K@0xd0000000:5 virtio_mmio.device=4K@0xd0002000:7 reboot=k panic=1' \
  < /dev/null" > "$GUEST_LOG" 2>&1 &
INGOT_PID=$!
echo "ingot pid=$INGOT_PID"

echo
echo "=== waiting for AF_VSOCK listener on (CID=$CID, port=$PORT) ==="
ready=0
for i in $(seq 1 40); do
  if "$HOST_BIN" $CID $PORT "probe" 2>/dev/null | grep -q "^ECHO"; then
    ready=1; break
  fi
  sleep 0.2
done
[ $ready -eq 1 ] && echo "ready after ${i} * 0.2s" || echo "TIMEOUT waiting for listener"

echo
echo "=== vsock requests ==="
declare -A reply
for MSG in "hello-1" "two-words" "the-third-message"; do
  r=$("$HOST_BIN" $CID $PORT "$MSG" 2>&1)
  echo "send: $MSG  ->  $r"
  reply[$MSG]="$r"
done

kill $INGOT_PID 2>/dev/null
wait $INGOT_PID 2>/dev/null
echo
echo "=== guest output (tail) ==="
tail -15 "$GUEST_LOG"

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

check "virtio-mmio.1 registered (vsock)"   "grep -q 'virtio-mmio.1 at 0xd0002000-0xd0002fff, IRQ 7' '$GUEST_LOG'"
check "PF_VSOCK protocol registered"        "grep -q 'PF_VSOCK protocol family' '$GUEST_LOG'"
check "guest echo banner printed"           "grep -q 'ingot guest vsock echo server' '$GUEST_LOG'"
check "host sees probe reply"               "[ $ready -eq 1 ]"
check "echo hello-1"                        "[[ '${reply[hello-1]}' == *'ECHO: hello-1'* ]]"
check "echo two-words"                      "[[ '${reply[two-words]}' == *'ECHO: two-words'* ]]"
check "echo the-third-message"              "[[ '${reply[the-third-message]}' == *'ECHO: the-third-message'* ]]"
check "guest serviced >=3 requests"         "[ \$(grep -c 'client said' '$GUEST_LOG') -ge 3 ]"

echo "================================================================"
echo "Full transcript: $LOG"
echo "Guest log:       $GUEST_LOG"
