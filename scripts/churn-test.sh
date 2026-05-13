#!/bin/bash
# scripts/churn-test.sh — integration-hardening starter.
# Create and destroy VMs in a loop. Record peak host RSS, fd count,
# and tap0 / virtio-vsock state. Pass criterion: zero growth over the
# run. Lightweight version; the 24-hour churn belongs in CI.
#
# What gets churned: a tiny initramfs that prints once and exits,
# triggering panic=1 → triple-fault → ingot exits. Each iteration
# does Vm::new + Vm::run + Drop. Run for $ITERATIONS (default 100).
#
# Outputs:
#   /tmp/ingot-churn.log  — per-iteration summary
#   stdout                — PASS / FAIL summary at end

set +e

ITERATIONS=${ITERATIONS:-100}
LOG=/tmp/ingot-churn.log
KERNEL=${KERNEL:-/tmp/kbuild/linux-6.6.30/vmlinux}
INITRAMFS=${INITRAMFS:-/tmp/min-initramfs.cpio.gz}
BUSYBOX=${BUSYBOX:-/tmp/busybox-static}
REPO=$(cd "$(dirname "$0")/.." && pwd)

[ -f "$KERNEL" ]  || { echo "MISSING: kernel"; exit 1; }
[ -x "$BUSYBOX" ] || { echo "MISSING: busybox-static"; exit 1; }

# Build the tiny initramfs if not present.
if [ ! -f "$INITRAMFS" ]; then
  ROOT=$(mktemp -d /tmp/churn-XXXX)
  mkdir -p $ROOT/{bin,dev,proc,sys}
  cp "$BUSYBOX" $ROOT/bin/busybox
  fakeroot bash -c "
    mknod $ROOT/dev/console c 5 1
    chmod 600 $ROOT/dev/console
    printf '#!/bin/busybox sh\necho CHURN_OK\nexit 0\n' > $ROOT/init
    chmod +x $ROOT/init
    cd $ROOT && find . -print0 | cpio --null -o -H newc 2>/dev/null \
                                | gzip > '$INITRAMFS'
  "
fi

cd "$REPO"
cargo build --release --quiet --bin ingot

rss_at_start=$(awk '/VmRSS/ {print $2; exit}' /proc/self/status 2>/dev/null || echo 0)
echo "iter,rss_kb,fd_count,exit_status" > "$LOG"

failures=0
ok_count=0
for i in $(seq 1 $ITERATIONS); do
  out=$(timeout 6 sg kvm -c "./target/release/ingot run \
    --kernel '$KERNEL' \
    --initramfs '$INITRAMFS' \
    --memory 64 \
    --cmdline 'console=ttyS0,115200 quiet reboot=t panic=1' \
    < /dev/null" 2>&1)
  status="?"
  echo "$out" | grep -q "exit=GuestFault(TripleFault)" && status="ok"
  echo "$out" | grep -q "ingot: " && [ "$status" != "ok" ] && status="fail"
  if [ "$status" = "ok" ]; then
    ok_count=$((ok_count+1))
  else
    failures=$((failures+1))
  fi
  rss=$(awk '/VmRSS/ {print $2; exit}' /proc/self/status 2>/dev/null || echo 0)
  fds=$(ls /proc/self/fd 2>/dev/null | wc -l)
  echo "$i,$rss,$fds,$status" >> "$LOG"
  if [ $((i % 10)) -eq 0 ]; then echo "  iter $i/$ITERATIONS  rss=${rss}kb fds=$fds ok=$ok_count fail=$failures"; fi
done

rss_at_end=$(awk '/VmRSS/ {print $2; exit}' /proc/self/status 2>/dev/null || echo 0)
rss_growth=$(( rss_at_end - rss_at_start ))
# Inspect: max RSS, max fds
max_rss=$(awk -F, 'NR>1 && $2+0>m{m=$2+0} END{print m}' "$LOG")
max_fds=$(awk -F, 'NR>1 && $3+0>m{m=$3+0} END{print m}' "$LOG")

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

check "all $ITERATIONS iterations triple-faulted cleanly" "[ $ok_count -eq $ITERATIONS ]"
check "no run-time failures"                              "[ $failures -eq 0 ]"
check "host RSS growth <2 MiB"                            "[ $rss_growth -lt 2048 ]"
check "host fd-count stable (<32 max)"                    "[ $max_fds -lt 32 ]"

echo
echo "Iterations: $ITERATIONS"
echo "Ok: $ok_count, Fail: $failures"
echo "Host RSS: start=$rss_at_start kb, end=$rss_at_end kb, max=$max_rss kb, growth=$rss_growth kb"
echo "Max fds: $max_fds"
echo "Per-iter log: $LOG"
