# Mosh Device Runtime

## Status

This document describes the platform-neutral runtime foundation currently implemented in `compiler/src/device.rs`. It is not a claim that camera, Wi-Fi, Bluetooth, GPS, battery, or other native hardware access is already available from Mosh programs.

## Design

Mosh remains an interpreted language. Device access is provided by a host-installed native adapter behind a platform-neutral Rust contract; it is not implemented as direct operating-system calls inside the interpreter.

The runtime contract includes:

- `Platform`: Android, iOS, Windows, Linux, or unknown.
- `Capability`: separate `supported` and `available` states, permission state, supported feature names, and limitations.
- `PermissionState`: not required, not requested, granted, denied, restricted, or unknown.
- `DeviceError`: stable error codes, module and operation context, a message, and retryability.
- `DeviceBackend`: native-host interface for capability discovery and permission operations.
- `DeviceRegistry`: checks support and availability before requesting permission.
- `DeviceHandle`: explicit, idempotent close with best-effort Drop cleanup.
- `CancellationToken` and `Lifecycle`: shared cancellation and validated resource lifecycle transitions.

## Important behavior

The CLI and REPL install a Linux/Android procfs/sysfs adapter when running on those operating systems. It discovers batteries through `/sys/class/power_supply`, reads memory information from `/proc/meminfo`, and reads CPU model/core/frequency information from `/proc/cpuinfo`. Other device modules remain unsupported. On iOS and Windows, the default remains `UnsupportedBackend` until native adapters are added. If sysfs battery discovery fails, battery support is reported as unavailable rather than returning fabricated data.

A native adapter must query the actual OS and device. It must distinguish hardware absence from permission denial and temporary unavailability. Permission prompts must be initiated only by an explicit program/user action and must respect app lifecycle interruption and revocation.

## Platform adapter plan

- Android: Kotlin/JVM adapter via JNI or a small host bridge; use Android platform APIs and runtime permissions.
- iOS: Swift adapter via FFI/host bridge; use the relevant Apple frameworks and their protected-resource authorization flow.
- Windows: WinRT/Win32 adapter with explicit capability and permission reporting.
- Linux: service/kernel interfaces such as NetworkManager, BlueZ, V4L2, PipeWire, sysfs, and udev where available.

Each module is independently capability-gated. Support for one module or platform never implies support for another.

## Implementation sequence

1. Runtime contracts and lifecycle primitives (this change).
2. Interpreter-visible `device` namespace and safe introspection APIs.
3. Real host adapters, beginning with read-only battery and system capability reporting.
4. Camera and sensor sessions with bounded frame/event delivery and cancellation.
5. Connectivity modules, including secure peer authentication and explicit user approval.
6. Windows, iOS, Android, and Linux integration tests on actual supported hosts.

No hardware operation should be represented by a stub that returns success or sample data. Unsupported modules must remain explicitly unsupported until their native implementation is complete and tested.
