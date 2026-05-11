# 0005: `MmioDevice::write` takes `&mut Memory`

Date: 2026-05-11
Status: accepted
Phase: 4

## Decision

`MmioDevice::write` and `MmioBus::write` carry `mem: &mut Memory`. The
read path does not. A new `poll_io(&mut self, mem: &mut Memory)` method
is added with a default no-op implementation for devices that don't
need async drainage.

## Why

A virtio device's notify path needs to walk descriptor chains in guest
memory to discover what the driver queued, gather the buffers, then
write a used-ring entry back. All three steps touch `Memory`. The
notify is initiated by a `MmioDevice::write` to the QueueNotify
register, so the trait method has to be able to reach memory.

Before this change, `MmioDevice::write(offset, data)` had no memory
reference. Options considered:

- **Thread memory through**: what we picked. Bus already finds the
  device on each call; passing one more `&mut Memory` is cheap and
  keeps the dispatch flat.
- **Return a `DeviceEvent` enum from `write`** that the lifecycle
  layer interprets and then calls back into the device with memory:
  two-step dance that just relocates the problem.
- **Internal mutability on a shared `Memory` (`Arc<Mutex<Memory>>`)**:
  contention on every read/write in the run loop. Memory access is
  hot; locking it is wrong.

For symmetry, `read` could also take `&mut Memory`, but no current
device needs it — register reads return state, not memory contents.
Keeping `read` narrower documents what does and doesn't reach across
into guest memory.

## `poll_io` rationale

`poll_io` handles the *device-initiated* direction: TAP fd has bytes,
we need to push them into the guest's RX queue and assert the IRQ.
That happens outside any guest-initiated write, so we need a hook
called from the run loop after each exit.

Signature mirrors `write`: same `&mut Memory` it would need to scatter
RX bytes across descriptors. Default no-op so non-async devices
(today, none — but `Uart` is PIO inline and `Block`/`Vsock` are
placeholders) don't need to implement it.

## Cost paid

- One extra `&mut Memory` parameter on every device write call. The
  borrow is free; `&mut` arguments are pointers.
- `Block` and `Vsock` placeholder implementations needed updating to
  match the new signature. Both are `unimplemented!()` placeholders.

## When to revisit

- If a future device needs guest memory access on the *read* path, add
  the parameter there too. Document why if it ever happens.
