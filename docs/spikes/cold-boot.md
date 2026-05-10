# Spike: cold boot

Tests architecture.md §12.1: the Phase 0 layout supports sub-50ms wall-clock from `Vm::run()` to first guest userspace instruction with a minimal kernel + init. Stretch goal: under 30ms.

## Method

Paper analysis. Decompose the cold-boot path into stages, attribute a wall-clock budget to each, identify the dominant cost. Compare against the refutation conditions in §12.1. Where assumptions are tight, surface them as concrete commitments the implementation must keep. Empirical validation is a Phase 9 deliverable; the goal here is feasibility, not measurement.

## Stage decomposition

The path from `Vm::run()` returning to the first userspace instruction in the guest. Numbers below are order-of-magnitude estimates from public KVM/Linux microvm work (Firecracker's published cold-boot breakdowns, kernel boot-time analyses), normalized to a modern x86_64 host with KVM.

| Stage | What happens | Estimated cost |
|---|---|---|
| **A. VMM setup** | mmap guest memory, `KVM_CREATE_VM`, `KVM_CREATE_VCPU`, `KVM_SET_USER_MEMORY_REGION`, set CPUID/MSRs, construct devices and MMIO bus | **3–5 ms** |
| **B. Boot prep** | ELF parse + segment copy of uncompressed `vmlinux` (≤10 MB), initramfs copy (≤5 MB), build `boot_params`, build identity-mapped page tables, set initial vCPU registers via `KVM_SET_REGS`/`KVM_SET_SREGS` | **3–5 ms** |
| **C. Kernel boot** | First guest instruction through to `exec /init`. Setup phase, memory init, minimal driver init (virtio-mmio only), VFS init, initramfs unpack, init exec | **15–30 ms** |
| **D. Init → workload** | Static init runs, optionally configures vsock, execs the workload | **1–3 ms** |
| **Total** |  | **22–43 ms** |

Median around 35 ms. Worst case at the upper end of each stage: 43 ms. Comfortably under the 50 ms target on the median; the 30 ms stretch goal is reachable only with the optimizations below.

## Where the budget lives

Stage **C** dominates. Inside it, the largest variable contributions are:

1. Memory init (`free_all_bootmem`-style work): ~5–10 ms for 128 MB. Roughly linear in guest memory.
2. Driver init: skippable cost in principle (no PCI, no ACPI), but each kernel module that initializes adds milliseconds. Hard cap requires kernel-config discipline.
3. Initramfs unpack: ~2–5 ms for ≤5 MB busybox image. Linear in image size.
4. Decompression: zero in this architecture — the spec commits to uncompressed `vmlinux`. This is where the Firecracker-style "skip bzImage decompression" win already lives.

Stages **A** and **B** combined are 6–10 ms total — small enough that aggressive optimization there is low ROI compared to kernel-config tightening.

## Checks against the refutation conditions

### 1. Boot-protocol setup > 5 ms even on a minimized path?

Stage B is estimated at 3–5 ms. The dominant cost is segment copying, bounded by memory bandwidth (~5 GB/s typical → 1–2 ms for 10 MB). Refutation condition is borderline — passes on the median, fails on a kernel image substantially larger than 10 MB, or on a host with significantly lower memory bandwidth.

**Concrete commitments to keep this passing:**

- C1. **Kernel image size budget:** uncompressed `vmlinux` ≤ 10 MB. Enforced by a CI check on the reference kernel build.
- C2. **Loader copies via `mmap` of the image file**, not `read()` into a heap buffer that is then parsed and copied again. Single bounce only, host-page-cache-friendly.
- C3. **Page tables are statically positioned** (architecture.md §5: PML4 at 0x9000–0xC000) and built by writing four pages of constants — never iterated dynamically.

### 2. MMIO base or initramfs placement forces additional kernel work?

The chosen MMIO base of `0xD000_0000` sits below 4 GiB, which keeps the kernel's early MMIO mapping in a 32-bit address space and avoids any PAE-related path. The initramfs is placed page-aligned immediately after the kernel image — the standard layout that the boot protocol expects. No additional kernel work is forced by either choice.

**Pass.** No new commitments needed.

### 3. ELF parsing of uncompressed `vmlinux` slower than expected → reconsider direct-load format?

Estimated 1–2 ms for a 10 MB image. Refutation requires either a much larger kernel (already covered by C1) or surprising overhead in the ELF parser itself. The latter is implementation-controllable — a streaming-copy parser that reads program headers and copies segments without intermediate allocation hits memory bandwidth, not CPU.

**Pass.** Implementation commitment:

- C4. **ELF loader is single-pass and allocation-free in the hot path.** Read program headers, then for each `PT_LOAD` segment, copy directly from the source slice (or `mmap`'d image) into `GuestMem`.

## Reaching the 30 ms stretch

Median 35 ms is over the stretch budget. To land sub-30 ms consistently, additional commitments are needed — these are stretch-goal commitments, not v1 commitments:

- S1. **Kernel build with aggressive minimization:** drop all non-virtio drivers, no ACPI, no PCI, `CONFIG_PRINTK=n`, panic=1, `quiet` cmdline.
- S2. **Static init** that immediately execs the workload — no busybox shell startup.
- S3. **Smaller guest memory** (e.g. 64 MB instead of 128) reduces stage C's memory-init linearly.
- S4. **PVH boot protocol** (replaces the realmode-trampoline path with a 64-bit entry point). This is a Phase 9 optimization; SPEC.md does not require it for v1.

Any of S1–S3 alone is unlikely to close the 5 ms gap; all four together plausibly bring the median to ~22–25 ms.

## Verdict

**Confirmed for the 50 ms target.** The architecture's choices — uncompressed `vmlinux` direct-load, MMIO base at `0xD000_0000`, page tables and `boot_params` at fixed low addresses, single-mmap guest memory — do not block sub-50 ms boot. The dominant cost is kernel-side and is controlled by build-time configuration of the reference kernel, not by VMM architecture.

**Not yet confirmed for the 30 ms stretch.** Reaching it requires the additional stretch commitments S1–S4, which are explicitly out of scope for v1.

### Concrete commitments adopted from this spike

1. **C1**: kernel image ≤ 10 MB uncompressed (CI-enforced).
2. **C2**: loader uses `mmap` of the kernel image file, not double-buffered read.
3. **C3**: page tables statically positioned, built from constants.
4. **C4**: ELF loader is single-pass and allocation-free in the hot path.

These are added to the boot subsystem's design constraints; architecture.md §5 already pins the addresses, so no edit is needed there. The four commitments above will be re-stated in `docs/boot.md` when that subsystem doc is written (Phase 8).
