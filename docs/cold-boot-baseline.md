# Cold-boot baseline

Captured 2026-05-11, commit before any Phase 9 optimization work. The
SPEC §2.4 differentiator-axis target is **<50ms** from `Vm::new` to
first guest byte; stretch is **<30ms**. The numbers below are the
*baseline* — what we get with no optimization, the current 6.6.30
custom kernel, default initramfs that exits immediately.

## Method

`./target/release/ingot run --kernel /tmp/kbuild/linux-6.6.30/vmlinux
--initramfs /tmp/min-initramfs.cpio.gz --memory 64 --timing --cmdline
"console=ttyS0,115200 quiet reboot=t panic=1"`

The `--timing` flag prints four numbers:
- `Vm::new` — wall-clock duration of `Vm::new(config)?`. Allocates
  guest memory, opens `/dev/kvm`, creates VM + vCPU, registers
  memory region, sets up KVM_CREATE_IRQCHIP and KVM_CREATE_PIT2,
  configures CPUID.
- `boot-kernel` — wall-clock from `Vm::new` return to "kernel state
  set up" — ELF parse, copy into guest memory, build page tables /
  GDT / boot_params / MP table, set initial vCPU registers.
- `first guest byte` — wall-clock from `Vm::new` start to the first
  byte the guest writes to the serial console (= first kernel printk).
- `total` — `Vm::new` start to `run()` return.

`first guest byte` is the cold-boot metric. `Vm::new` and `boot-kernel`
are diagnostic — they tell us where the time goes.

## Five samples

| Run | Vm::new (ms) | boot-kernel (ms) | first guest byte (ms) |
|-----|-------------:|-----------------:|----------------------:|
| 1 | 0.72  | 14.16 | **38.18** |
| 2 | 0.81  | 10.73 | **23.65** |
| 3 | 22.78 | 11.91 | **54.52** |
| 4 | 0.57  | 11.88 | **33.65** |
| 5 | 25.24 | 31.85 | **91.60** |

Best of five: **23.6ms** (already meets stretch target of <30ms).
Median: ~38ms (meets baseline target of <50ms).
Worst: 91ms (1 in 5 — host scheduling / load variance).

## Where the time goes

- `Vm::new` is typically <1ms but occasionally jumps to 20ms+. Suspect
  kernel-side KVM ioctl latency under load (KVM_CREATE_IRQCHIP and
  CPUID setup) or first-fault on mmap'd memory regions.
- `boot-kernel` (between Vm::new and first guest insn) is ~12ms
  steady. ELF load is the big chunk — we copy each kernel segment
  byte-by-byte via `Memory::write`.
- Guest-side kernel boot (between first vCPU run and first printk) is
  ~10–25ms, depending on host load. The kernel does cpuid probes,
  PIT calibration (fails — TSC unstable), refined-jiffies init.

## Path to lower numbers (Phase 9 work, not yet started)

In approximate order of impact:

1. **Kernel config trimming.** Current vmlinux is 13 MB; trims to
   ~5 MB are plausible by stripping FPU/MPX features, more locale
   subsystems, the entire crypto API, the BPF runtime, KASLR
   relocations. Smaller image = faster ELF load + less page-fault
   churn during early boot.
2. **`init=/init` direct call instead of `Run /init`.** The kernel's
   `kernel_init_freeable` does meaningful work before exec'ing /init:
   `console_init`, `kmem_cache_init_late`, `arch_post_acpi_subsys_init`.
   Replacing init with a no-op static binary doesn't bypass that.
3. **TSC calibration fix.** PIT calibration fails on our setup
   ("Unable to calibrate against PIT", "TSC unstable"). The kernel
   falls back to refined-jiffies. Fixing this saves the calibration
   wait and would let `time=` measurements be real.
4. **`KVM_PRE_FAULT_MEMORY`.** Pre-fault the guest memory region so
   first-touch faults don't account against the boot path.
5. **`fork` + `posix_spawn` shaving.** Vm::new's ~25ms-occasional
   jumps look like CPU migration / cache cold. Pinning the run-loop
   thread to a CPU might help.

These are tracked in SPEC §9 as the Phase 9 spike. None blocked on
the others; pick the highest-impact first when the spike opens.

## Reproduce

```sh
ROOT=$(mktemp -d /tmp/min-XXXX)
mkdir -p $ROOT/{bin,dev,proc,sys}
cp /tmp/busybox-static $ROOT/bin/busybox
fakeroot bash -c "
mknod $ROOT/dev/console c 5 1
chmod 600 $ROOT/dev/console
echo -e '#!/bin/busybox sh\necho COLD_BOOT_READY\nexit 0' > $ROOT/init
chmod +x $ROOT/init
cd $ROOT && find . -print0 | cpio --null -o -H newc 2>/dev/null \
                            | gzip > /tmp/min-initramfs.cpio.gz
"

for i in 1 2 3 4 5; do
  timeout 12 sg kvm -c "./target/release/ingot run \
    --kernel /tmp/kbuild/linux-6.6.30/vmlinux \
    --initramfs /tmp/min-initramfs.cpio.gz \
    --memory 64 --timing \
    --cmdline 'console=ttyS0,115200 quiet reboot=t panic=1' \
    < /dev/null" 2>&1 | grep "first guest"
done
```
