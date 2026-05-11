#!/bin/bash
# scripts/net-test.sh — exercise ingot's virtio-net end to end.
#
# What it does:
#   1. Ensures tap0 exists, has 10.0.0.1/24, and is up (sudo, idempotent).
#   2. Builds a busybox initramfs whose /init brings eth0 up at
#      10.0.0.2/24 and runs ping + arp + ifconfig.
#   3. Boots the guest with --tap tap0 and captures the serial output.
#   4. Prints a PASS / FAIL summary you can eyeball.
#
# Outputs:
#   /tmp/ingot-net-test.log   — full transcript (host + guest)
#   /tmp/ingot-net-guest.log  — guest serial only
#
# Usage:  ./scripts/net-test.sh
# Prereqs: cargo built ingot, busybox-static at /tmp/busybox-static,
#          vmlinux at /tmp/kbuild/linux-6.6.30/vmlinux (override via $KERNEL).

set +e

LOG=/tmp/ingot-net-test.log
GUEST_LOG=/tmp/ingot-net-guest.log
INITRAMFS=/tmp/ingot-net-initramfs.cpio.gz
KERNEL=${KERNEL:-/tmp/kbuild/linux-6.6.30/vmlinux}
BUSYBOX=${BUSYBOX:-/tmp/busybox-static}
REPO=$(cd "$(dirname "$0")/.." && pwd)

exec > >(tee "$LOG") 2>&1

echo "================================================================"
echo "  ingot net-test  $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "  repo:    $REPO"
echo "  kernel:  $KERNEL"
echo "  busybox: $BUSYBOX"
echo "================================================================"

# --- preflight --------------------------------------------------------------
[ -f "$KERNEL" ]  || { echo "MISSING: kernel at $KERNEL"; exit 1; }
[ -x "$BUSYBOX" ] || { echo "MISSING: static busybox at $BUSYBOX"; exit 1; }
command -v fakeroot >/dev/null || { echo "MISSING: fakeroot"; exit 1; }

# --- host tap setup (sudo, idempotent) --------------------------------------
echo
echo "=== HOST: tap0 setup ==="
if ! ip link show tap0 >/dev/null 2>&1; then
  sudo ip tuntap add tap0 mode tap user "$USER" || exit 1
fi
sudo ip addr replace 10.0.0.1/24 dev tap0
sudo ip link set tap0 up
sudo chmod 666 /dev/net/tun
ip -br addr show tap0
ls -l /dev/net/tun

# --- build initramfs --------------------------------------------------------
echo
echo "=== HOST: building initramfs ==="
ROOT=$(mktemp -d /tmp/ingot-ramfs-XXXX)
mkdir -p "$ROOT"/{bin,dev,proc,sys,tmp}
cp "$BUSYBOX" "$ROOT/bin/busybox"

fakeroot bash -s "$ROOT" "$INITRAMFS" <<'FAKEROOT_EOF'
ROOT=$1; OUT=$2
mknod $ROOT/dev/console c 5 1
mknod $ROOT/dev/null c 1 3
mknod $ROOT/dev/tty c 5 0
mknod $ROOT/dev/ttyS0 c 4 64
chmod 600 $ROOT/dev/console
chmod 666 $ROOT/dev/null $ROOT/dev/tty $ROOT/dev/ttyS0
cat > $ROOT/init <<'INIT'
#!/bin/busybox sh
/bin/busybox --install -s /bin
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null

echo
echo "=== guest: interfaces before bringup ==="
ip -br link

echo
echo "=== guest: bringing eth0 up ==="
ip link set lo up
ip link set eth0 up
ip addr add 10.0.0.2/24 dev eth0
ip route add default via 10.0.0.1

echo
echo "=== guest: interfaces after bringup ==="
ip -br addr

echo
echo "=== guest: ping 10.0.0.1 (3 packets, 1s timeout each) ==="
ping -c 3 -W 1 10.0.0.1

echo
echo "=== guest: arp / neighbor table ==="
ip neigh show

echo
echo "=== guest: counters ==="
ifconfig eth0 | grep -E "RX packets|TX packets|RX bytes|TX bytes"

echo
echo "GUEST_DONE_MARKER"
sleep 1
INIT
chmod +x $ROOT/init
cd $ROOT && find . -print0 | cpio --null -o -H newc 2>/dev/null | gzip > "$OUT"
FAKEROOT_EOF
ls -la "$INITRAMFS"

# --- boot guest -------------------------------------------------------------
echo
echo "=== BOOTING GUEST ==="
cd "$REPO"
timeout 20 sg kvm -c "cargo run --quiet --bin ingot -- run \
  --kernel '$KERNEL' \
  --initramfs '$INITRAMFS' \
  --memory 256 \
  --tap tap0 \
  --cmdline 'console=ttyS0,115200 virtio_mmio.device=4K@0xd0000000:5 reboot=k panic=1' \
  < /dev/null" 2>&1 | tee "$GUEST_LOG"

# --- pass / fail summary ----------------------------------------------------
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

check "eth0 visible in /sys/class/net"   "grep -qw eth0 '$GUEST_LOG'"
check "MAC 52:54:00:c0:ff:ee negotiated" "grep -qiE '52:54:00:c0:ff:ee' '$GUEST_LOG'"
check "eth0 has 10.0.0.2 assigned"       "grep -q '10.0.0.2' '$GUEST_LOG'"
check "neighbor table mentions 10.0.0.1" "grep -q '10.0.0.1' '$GUEST_LOG'"
check "ping: 1 or more replies"          "grep -qE '[1-9][0-9]* (packets )?received' '$GUEST_LOG'"
check "eth0 TX packets > 0"              "grep -qE 'TX packets[: ]+[1-9]' '$GUEST_LOG'"
check "eth0 RX packets > 0"              "grep -qE 'RX packets[: ]+[1-9]' '$GUEST_LOG'"
check "guest /init reached end"          "grep -q GUEST_DONE_MARKER '$GUEST_LOG'"

echo "================================================================"
echo
echo "Full transcript: $LOG"
echo "Guest log:       $GUEST_LOG"
