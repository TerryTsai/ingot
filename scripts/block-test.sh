#!/bin/bash
# scripts/block-test.sh — exercise virtio-block end to end.
#
# What it does:
#   1. Builds a 4 MiB ext4 image and seeds it with a marker file.
#   2. Builds a busybox initramfs whose /init mounts /dev/vda, reads
#      the marker, appends a new line, syncs, and unmounts.
#   3. Boots ingot with --rootfs IMG.
#   4. Mounts the image back on the host to confirm the write persisted.
#   5. Prints PASS / FAIL summary.
#
# Outputs:
#   /tmp/ingot-block-test.log   — full transcript
#   /tmp/ingot-block-guest.log  — guest serial only
#   /tmp/ingot-block-test.img   — disk image used (kept for inspection)
#
# Prereqs: cargo built ingot release, busybox-static at /tmp/busybox-static,
# vmlinux with VIRTIO_BLK + EXT4 at $KERNEL.

set +e

LOG=/tmp/ingot-block-test.log
GUEST_LOG=/tmp/ingot-block-guest.log
IMG=/tmp/ingot-block-test.img
INITRAMFS=/tmp/ingot-block-initramfs.cpio.gz
KERNEL=${KERNEL:-/tmp/kbuild/linux-6.6.30/vmlinux}
BUSYBOX=${BUSYBOX:-/tmp/busybox-static}
REPO=$(cd "$(dirname "$0")/.." && pwd)

exec > >(tee "$LOG") 2>&1

echo "================================================================"
echo "  ingot block-test  $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "================================================================"

[ -f "$KERNEL" ]  || { echo "MISSING: kernel at $KERNEL"; exit 1; }
[ -x "$BUSYBOX" ] || { echo "MISSING: static busybox at $BUSYBOX"; exit 1; }

# --- build disk image (sudo for loop-mount the seed step) ---------
echo
echo "=== HOST: building disk image ==="
MARKER="BLOCK_TEST_$(date +%s)"
dd if=/dev/zero of="$IMG" bs=1M count=4 status=none
/sbin/mkfs.ext4 -F -L INGOT -q "$IMG" 2>/dev/null
MNT=$(mktemp -d)
sudo mount -o loop "$IMG" "$MNT"
echo "$MARKER" | sudo tee "$MNT/marker.txt" >/dev/null
sudo umount "$MNT"
rmdir "$MNT"
echo "  $IMG ($(stat -c%s $IMG) bytes), marker=$MARKER"

# --- build initramfs ---------------------------------------------
echo
echo "=== HOST: building initramfs ==="
ROOT=$(mktemp -d /tmp/ingot-block-ramfs-XXXX)
mkdir -p "$ROOT"/{bin,dev,proc,sys,mnt}
cp "$BUSYBOX" "$ROOT/bin/busybox"
fakeroot bash -s "$ROOT" "$INITRAMFS" "$MARKER" <<'FAKEROOT_EOF'
ROOT=$1; OUT=$2; SEED=$3
mknod $ROOT/dev/console c 5 1
mknod $ROOT/dev/null c 1 3
mknod $ROOT/dev/ttyS0 c 4 64
mknod $ROOT/dev/vda b 254 0
chmod 600 $ROOT/dev/console
chmod 666 $ROOT/dev/null $ROOT/dev/ttyS0
chmod 660 $ROOT/dev/vda
cat > $ROOT/init <<INIT
#!/bin/busybox sh
/bin/busybox --install -s /bin
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null
echo
echo "=== /dev/vda visible ==="
ls -la /dev/vda
echo
echo "=== mount ext4 ==="
mkdir -p /mnt/disk
mount -t ext4 /dev/vda /mnt/disk
echo
echo "=== marker file ==="
cat /mnt/disk/marker.txt
echo
echo "=== writing back ==="
echo "GUEST_WROTE_BACK_$SEED" >> /mnt/disk/marker.txt
sync
umount /mnt/disk
echo
echo "GUEST_BLOCK_DONE"
sleep 1
INIT
chmod +x $ROOT/init
cd $ROOT && find . -print0 | cpio --null -o -H newc 2>/dev/null | gzip > "$OUT"
FAKEROOT_EOF
ls -la "$INITRAMFS"

# --- boot guest --------------------------------------------------
echo
echo "=== BOOTING GUEST ==="
cd "$REPO"
cargo build --release --quiet --bin ingot
timeout 20 sg kvm -c "./target/release/ingot run \
  --kernel '$KERNEL' \
  --initramfs '$INITRAMFS' \
  --rootfs '$IMG' \
  --memory 256 \
  --cmdline 'console=ttyS0,115200 virtio_mmio.device=4K@0xd0000000:5 virtio_mmio.device=4K@0xd0001000:6 reboot=k panic=1' \
  < /dev/null" 2>&1 | tee "$GUEST_LOG" | sed -n '/=== \/dev\/vda visible ===/,/GUEST_BLOCK_DONE/p'

# --- verify writeback --------------------------------------------
echo
echo "=== verifying write persisted ==="
MNT=$(mktemp -d)
sudo mount -o loop,ro "$IMG" "$MNT"
MARKER_CONTENTS=$(sudo cat "$MNT/marker.txt")
HOST_SEES_MARKER=0
echo "$MARKER_CONTENTS" | grep -q "GUEST_WROTE_BACK_$MARKER" && HOST_SEES_MARKER=1
echo "$MARKER_CONTENTS"
sudo umount "$MNT"
rmdir "$MNT"

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

check "virtio-mmio.1 registered (IRQ 6)" "grep -q 'virtio-mmio.1 at 0xd0001000-0xd0001fff, IRQ 6' '$GUEST_LOG'"
check "virtio_blk driver bound to vda"    "grep -q 'virtio_blk virtio1' '$GUEST_LOG'"
check "capacity advertised (4 MiB)"       "grep -q '8192 512-byte' '$GUEST_LOG'"
check "ext4 mount succeeded"              "grep -q 'mounted filesystem' '$GUEST_LOG'"
check "guest read marker.txt"             "grep -q '$MARKER' '$GUEST_LOG'"
check "host sees guest writeback"         "[ '$HOST_SEES_MARKER' = '1' ]"
check "ext4 unmount succeeded"            "grep -q 'unmounting filesystem' '$GUEST_LOG'"
check "guest /init reached end"           "grep -q GUEST_BLOCK_DONE '$GUEST_LOG'"

echo "================================================================"
echo "Full transcript: $LOG"
echo "Guest log:       $GUEST_LOG"
echo "Disk image:      $IMG"
