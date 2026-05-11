#!/bin/bash
# scripts/compute-demo.sh — boot ingot with a Rust compute workload as
# /init, then make a TCP request from the host to validate end-to-end
# compute + networking.
#
# What it does:
#   1. Cross-compiles examples/guest_compute.rs to a static musl binary.
#   2. Builds an initramfs with busybox + the compute binary + a small
#      shell init that brings eth0 up and execs the compute binary.
#   3. Boots ingot in the background with --tap tap0.
#   4. Waits for the guest's TCP listener on 10.0.0.2:9000.
#   5. Sends a few integers, captures the prime-count replies.
#   6. Stops the guest and prints PASS / FAIL.
#
# Prereqs: rustup target installed for x86_64-unknown-linux-musl;
# busybox-static at /tmp/busybox-static; kernel at $KERNEL.

set +e

LOG=/tmp/ingot-compute-demo.log
GUEST_LOG=/tmp/ingot-compute-guest.log
INITRAMFS=/tmp/ingot-compute-initramfs.cpio.gz
COMPUTE_BIN=/tmp/guest-compute
KERNEL=${KERNEL:-/tmp/kbuild/linux-6.6.30/vmlinux}
BUSYBOX=${BUSYBOX:-/tmp/busybox-static}
REPO=$(cd "$(dirname "$0")/.." && pwd)
GUEST_IP=10.0.0.2
HOST_IP=10.0.0.1
PORT=9000

exec > >(tee "$LOG") 2>&1

echo "================================================================"
echo "  ingot compute-demo  $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "================================================================"

# --- preflight + tap0 setup --------------------------------------
[ -f "$KERNEL" ]  || { echo "MISSING: kernel at $KERNEL"; exit 1; }
[ -x "$BUSYBOX" ] || { echo "MISSING: static busybox at $BUSYBOX"; exit 1; }
command -v nc >/dev/null || { echo "MISSING: nc (netcat-openbsd)"; exit 1; }

echo
echo "=== HOST: tap0 setup ==="
if ! ip link show tap0 >/dev/null 2>&1; then
  sudo ip tuntap add tap0 mode tap user "$USER" || exit 1
fi
sudo ip addr replace $HOST_IP/24 dev tap0
sudo ip link set tap0 up
sudo chmod 666 /dev/net/tun
ip -br addr show tap0

# --- build guest binary -----------------------------------------
echo
echo "=== HOST: building guest binary ==="
rustc --edition 2021 -O --target x86_64-unknown-linux-musl \
      "$REPO/examples/guest_compute.rs" -o "$COMPUTE_BIN"
ls -la "$COMPUTE_BIN"

# --- build initramfs --------------------------------------------
echo
echo "=== HOST: building initramfs ==="
ROOT=$(mktemp -d /tmp/ingot-compute-ramfs-XXXX)
mkdir -p "$ROOT"/{bin,dev,proc,sys,tmp}
cp "$BUSYBOX" "$ROOT/bin/busybox"
cp "$COMPUTE_BIN" "$ROOT/compute"
chmod +x "$ROOT/compute"

fakeroot bash -s "$ROOT" "$INITRAMFS" "$GUEST_IP" <<'FAKEROOT_EOF'
ROOT=$1; OUT=$2; IP=$3
mknod $ROOT/dev/console c 5 1
mknod $ROOT/dev/null c 1 3
mknod $ROOT/dev/ttyS0 c 4 64
chmod 600 $ROOT/dev/console
chmod 666 $ROOT/dev/null $ROOT/dev/ttyS0
cat > $ROOT/init <<INIT
#!/bin/busybox sh
/bin/busybox --install -s /bin
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null
ip link set lo up
ip link set eth0 up
ip addr add $IP/24 dev eth0
exec /compute
INIT
chmod +x $ROOT/init
cd $ROOT && find . -print0 | cpio --null -o -H newc 2>/dev/null | gzip > "$OUT"
FAKEROOT_EOF
ls -la "$INITRAMFS"

# --- launch ingot in background ----------------------------------
echo
echo "=== launching guest ==="
cd "$REPO"
cargo build --release --quiet --bin ingot
sg kvm -c "./target/release/ingot run \
  --kernel '$KERNEL' \
  --initramfs '$INITRAMFS' \
  --memory 256 \
  --tap tap0 \
  --cmdline 'console=ttyS0,115200 quiet virtio_mmio.device=4K@0xd0000000:5 reboot=k panic=1' \
  < /dev/null" > "$GUEST_LOG" 2>&1 &
INGOT_PID=$!
echo "ingot pid=$INGOT_PID"

# --- wait for TCP listener --------------------------------------
echo
echo "=== waiting for guest listener on $GUEST_IP:$PORT ==="
ready=0
for i in $(seq 1 30); do
  # Listener readiness probe: warmup request that we throw away. -z would
  # leave a dangling empty connection that the guest counts as a request.
  if echo 100 | nc -w 1 $GUEST_IP $PORT 2>/dev/null | grep -q count=; then
    ready=1; break
  fi
  sleep 0.2
done

if [ $ready -eq 0 ]; then
  echo "guest never came up"
  kill $INGOT_PID 2>/dev/null
  wait $INGOT_PID 2>/dev/null
  tail -20 "$GUEST_LOG"
  exit 1
fi
echo "ready after ${i} * 0.2s"

# --- run compute requests ---------------------------------------
echo
echo "=== compute requests ==="
declare -A reply
for N in 100 1000 100000 1000000; do
  r=$(echo "$N" | nc -w 15 $GUEST_IP $PORT)
  echo "req: $N -> $r"
  reply[$N]="$r"
done

# --- stop guest --------------------------------------------------
echo
kill $INGOT_PID 2>/dev/null
wait $INGOT_PID 2>/dev/null
echo "=== guest output (tail) ==="
tail -15 "$GUEST_LOG"

# --- pass / fail summary ----------------------------------------
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

check "guest warmup banner appeared"   "grep -q 'ingot guest compute demo' '$GUEST_LOG'"
check "guest reports 168 primes <1000" "grep -q 'found 168 primes' '$GUEST_LOG'"
check "guest opened :9000 listener"    "grep -q 'listening on 0.0.0.0:9000' '$GUEST_LOG'"
check "req N=100 -> 25 primes"         "[[ '${reply[100]}' == *'count=25'* ]]"
check "req N=1000 -> 168 primes"       "[[ '${reply[1000]}' == *'count=168'* ]]"
check "req N=100000 -> 9592 primes"    "[[ '${reply[100000]}' == *'count=9592'* ]]"
check "req N=1000000 -> 78498 primes"  "[[ '${reply[1000000]}' == *'count=78498'* ]]"
check "guest processed >=4 requests"   "[ \$(grep -c 'client: req=' '$GUEST_LOG') -ge 4 ]"

echo "================================================================"
echo "Full transcript: $LOG"
echo "Guest log:       $GUEST_LOG"
