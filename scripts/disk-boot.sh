#!/bin/bash
# scripts/disk-boot.sh — strict Phase 3 done-criterion: boot from a
# virtio-block rootfs without an initramfs. Builds a self-contained
# ext4 image (just /init + /dev/console + the bare mount points),
# boots ingot with `root=/dev/vda init=/init` and no --initramfs,
# connects to the guest's TCP server via the same tap0 path the
# compute demo uses.
#
# Done-criterion verified by the host completing an end-to-end TCP
# round-trip with the guest while ingot has no initramfs argument.

set +e

LOG=/tmp/ingot-disk-boot.log
GUEST_LOG=/tmp/ingot-disk-boot-guest.log
IMG=/tmp/ingot-disk-boot.img
COMPUTE_BIN=/tmp/guest-compute
KERNEL=${KERNEL:-/tmp/kbuild/linux-6.6.30/vmlinux}
REPO=$(cd "$(dirname "$0")/.." && pwd)
GUEST_IP=10.0.0.2
HOST_IP=10.0.0.1
PORT=9000

exec > >(tee "$LOG") 2>&1

echo "================================================================"
echo "  ingot disk-boot  $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "================================================================"

[ -f "$KERNEL" ] || { echo "MISSING: kernel"; exit 1; }
command -v nc >/dev/null || { echo "MISSING: nc"; exit 1; }

echo
echo "=== HOST: cross-compile compute binary ==="
rustc --edition 2021 -O --target x86_64-unknown-linux-musl \
      "$REPO/examples/guest_compute.rs" -o "$COMPUTE_BIN"
ls -la "$COMPUTE_BIN"

echo
echo "=== HOST: tap0 + rootfs setup ==="
if ! ip link show tap0 >/dev/null 2>&1; then
  sudo ip tuntap add tap0 mode tap user "$USER" || exit 1
fi
sudo ip addr replace $HOST_IP/24 dev tap0
sudo ip link set tap0 up
sudo chmod 666 /dev/net/tun

dd if=/dev/zero of="$IMG" bs=1M count=8 status=none
/sbin/mkfs.ext4 -F -L INGOTROOT -q "$IMG" 2>/dev/null
MNT=$(mktemp -d)
sudo mount -o loop "$IMG" "$MNT"
sudo mkdir -p $MNT/{dev,proc,sys,tmp}
sudo mknod $MNT/dev/console c 5 1
sudo mknod $MNT/dev/null c 1 3
sudo mknod $MNT/dev/vda b 254 0
sudo cp "$COMPUTE_BIN" $MNT/init
sudo chmod +x $MNT/init
sudo umount "$MNT"
rmdir "$MNT"
ls -la "$IMG"

echo
echo "=== launching guest (no --initramfs) ==="
cd "$REPO"
cargo build --release --quiet --bin ingot
sg kvm -c "./target/release/ingot run \
  --kernel '$KERNEL' \
  --rootfs '$IMG' \
  --tap tap0 \
  --memory 256 \
  --cmdline 'console=ttyS0,115200 root=/dev/vda rw init=/init ip=10.0.0.2::10.0.0.1:255.255.255.0::eth0:off virtio_mmio.device=4K@0xd0000000:5 virtio_mmio.device=4K@0xd0001000:6 reboot=k panic=1' \
  < /dev/null" > "$GUEST_LOG" 2>&1 &
INGOT_PID=$!
echo "ingot pid=$INGOT_PID"

echo
echo "=== wait for guest TCP listener ==="
ready=0
for i in $(seq 1 40); do
  if echo 100 | nc -w 1 $GUEST_IP $PORT 2>/dev/null | grep -q count=; then ready=1; break; fi
  sleep 0.2
done
[ $ready -eq 1 ] && echo "ready after ${i}*0.2s" || echo "TIMEOUT"

echo
echo "=== compute round-trip ==="
REPLY=$(echo 10000 | nc -w 5 $GUEST_IP $PORT)
echo "host: $REPLY"

kill $INGOT_PID 2>/dev/null
wait $INGOT_PID 2>/dev/null
echo
echo "=== guest log (tail) ==="
tail -10 "$GUEST_LOG"

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

check "virtio_blk recognized /dev/vda"      "grep -q 'virtio_blk virtio1: \[vda\]' '$GUEST_LOG'"
check "ext4 root mounted from disk"          "grep -q 'EXT4-fs (vda): mounted filesystem' '$GUEST_LOG'"
check "kernel mounted root on /dev/vda"      "grep -q 'Mounted root (ext4 filesystem) on device 254:0' '$GUEST_LOG'"
check "kernel exec'd /init from disk"        "grep -q 'Run /init as init process' '$GUEST_LOG'"
check "compute demo banner from disk-/init"  "grep -q 'ingot guest compute demo' '$GUEST_LOG'"
check "TCP listener came up"                  "[ $ready -eq 1 ]"
check "host got correct prime count (10000)" "[[ '$REPLY' == *'count=1229'* ]]"
check "no initramfs path passed to ingot"    "! grep -q 'Unpacking initramfs' '$GUEST_LOG'"

echo "================================================================"
echo "Full transcript: $LOG"
echo "Guest log:       $GUEST_LOG"
echo "Rootfs image:    $IMG"
