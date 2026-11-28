# Mosh Runtime and Ecosystem Architecture

**Status: architecture proposal for staged implementation.** This document defines the direction for Mosh's general-purpose runtime. It does not claim that the modules described here are already implemented.

## 1. Product direction

Mosh is an interpreted, general-purpose language with a portable runtime and host-platform integrations. The language should let developers express application logic once while using explicit, capability-checked integrations for the environment in which the program runs.

The interpreter remains the execution engine. AI models, databases, browser engines, operating-system services, and hardware drivers are integrations—not systems Mosh should reimplement.

Core principles:

1. Portable language semantics; platform-specific behavior behind host adapters.
2. No simulated success: unavailable capabilities return explicit unsupported/unavailable errors.
3. Least privilege: modules declare capabilities and permissions before performing sensitive work.
4. Bounded resources: streams, buffers, tasks, and handles have documented limits and deterministic cleanup.
5. Stable public APIs: implementation details may vary by platform without changing Mosh source unnecessarily.
6. Incremental delivery: each module is useful, tested, documented, and independently releasable.

## 2. Layered architecture

```text
.mosh source
    |
Lexer / Parser / Diagnostics
    |
Interpreter and Runtime Values
    |
Module Resolver + Standard Library
    |
Capability / Permission / Resource Manager
    |
Host Adapter Interfaces
    |
Android | iOS | Windows | Linux | Server / Embedded
```

The runtime is divided into these logical layers:

- **Language core:** values, scopes, functions, control flow, exceptions, imports, diagnostics.
- **Standard library:** strings, collections, math, paths, files, JSON, time, processes, testing.
- **Module system:** stable module names, exports, versioning, dependency resolution and caching.
- **Runtime services:** task scheduling, cancellation, resource ownership, event delivery and structured errors.
- **Capability manager:** support detection, availability, permission state, permission requests and access checks.
- **Host adapters:** native platform APIs implementing narrow, testable interfaces.
- **Developer tools:** REPL, formatter, test runner, package manager, debugger and documentation tooling.

These are architectural boundaries. They should not all be introduced as one large rewrite.

## 3. Unified module namespaces

The public namespace is organized by domain:

| Namespace | Purpose | Examples of future modules |
|---|---|---|
| `std.*` | Language and general utilities | `std.json`, `std.time`, `std.math`, `std.test` |
| `web.*` | Web clients and servers | `web.http`, `web.websocket`, `web.server` |
| `app.*` | Application lifecycle and UI integration | `app.lifecycle`, `app.navigation`, `app.notifications` |
| `device.*` | Hardware and operating-system device services | `device.camera`, `device.wifi`, `device.bluetooth` |
| `ai.*` | Model and AI service integration | `ai.models`, `ai.embeddings`, `ai.tools` |
| `data.*` | Data storage and processing | `data.sqlite`, `data.csv`, `data.stream` |
| `science.*` | Scientific and engineering computation | `science.stats`, `science.signal`, `science.simulation` |
| `security.*` | Defensive security primitives and analysis | `security.crypto`, `security.audit`, `security.tls` |
| `net.*` | Lower-level networking | `net.tcp`, `net.udp`, `net.dns` |

Names are reserved as design direction, not promises of current availability. A module is considered implemented only when its behavior, errors, tests, documentation, and supported platforms are present.

## 4. Standard module contract

Where appropriate, host-dependent modules should expose a consistent introspection contract:

- `is_supported()`: whether this runtime build has an adapter for the capability.
- `is_available()`: whether the capability can be used now.
- `capabilities()`: structured description of supported operations and limits.
- `permission()`: current permission state, without prompting.
- `request_permission()`: explicit request where the host OS supports it.
- `status()`: current service or resource status.

Not every pure software module needs this contract. A filesystem or camera module may expose domain-specific methods in addition to it.

A permission request must never be hidden inside a read or discovery call. Permission prompts are user-visible side effects and must be explicit.

## 5. Host adapter contract

The interpreter must not call Android, iOS, Windows, or Linux APIs directly. It calls a Rust trait or service interface; a platform host implements that interface.

Adapter responsibilities:

- Report support and runtime availability accurately.
- Validate arguments and enforce capability checks.
- Map native failures to stable Mosh error codes.
- Return owned data rather than leaking native pointers or handles.
- Respect cancellation and lifecycle transitions.
- Bound memory, queue sizes, timeouts, and concurrent operations.
- Close resources when scripts release them or the host lifecycle ends.

Initial stable error vocabulary:

- `UNSUPPORTED`
- `NOT_AVAILABLE`
- `PERMISSION_DENIED`
- `BUSY`
- `TIMEOUT`
- `DISCONNECTED`
- `INVALID_ARGUMENT`
- `OS_ERROR`
- `RESOURCE_CLOSED`
- `LIFECYCLE_INTERRUPTED`

Errors should carry a stable code and useful message, with optional platform diagnostic details. Platform details must not replace the stable code.

## 6. Asynchronous work and resource safety

The current interpreter executes synchronously. Device capture, network servers, long-running AI requests, and event streams need an explicit asynchronous design before they become public Mosh APIs.

The runtime design must provide:

- A task handle with explicit cancellation and completion state.
- Bounded event queues with documented overflow behavior.
- Timeouts on waits and I/O operations.
- Resource handles with explicit close semantics.
- Deterministic cleanup on normal completion, errors, cancellation, permission revocation, and host shutdown.
- No unbounded blocking of the interpreter thread.

A host adapter must copy data needed beyond a native callback's lifetime. For example, a camera adapter must copy a frame into a bounded owned buffer and promptly release the platform image object; it must not retain a camera frame proxy indefinitely.

## 7. Security and trust boundaries

Mosh is not automatically a sandbox. File access, process execution, network access, and native integrations inherit the host process's authority unless a separately designed sandbox is enabled.

Before adding package installation or third-party modules:

- Define package source and integrity policy.
- Keep secrets out of source files and logs.
- Make network and process capabilities explicit.
- Validate native and FFI boundaries.
- Document trust assumptions and unsafe operations.
- Add resource limits and cancellation to externally controlled operations.

The package manager must not silently execute arbitrary install scripts.

## 8. Domain roadmaps

### Foundation and standard library

Complete module resolution and exports, structured errors, JSON, time, robust path/file APIs, process APIs with explicit permissions, testing utilities, and runtime resource ownership.

### Web and backend

Build HTTP client support first, then WebSocket client/server and a small HTTP server. Include TLS verification, timeouts, streaming limits, cancellation, and clear server lifecycle management.

### Apps

Define application lifecycle and UI-host interfaces. Android is the first host integration target because the repository already has Android JNI/Kotlin infrastructure. Keep UI toolkit bindings separate from language semantics.

### IoT and devices

Continue the existing `device.*` capability model. Prioritize camera session bridging, location, connectivity, Bluetooth/BLE, USB, sensors, and audio one adapter at a time. Each module must have a real platform implementation or report unsupported.

### AI

Provide provider-neutral interfaces for model discovery, inference, embeddings, tool invocation, and streaming. Support local and remote inference through adapters. Do not claim an AI model is bundled unless model weights and runtime packaging are actually supplied.

### Science and engineering

Add dependable numerical foundations first: matrix/vector operations, statistics, units, signal processing, plotting data formats, and simulation primitives. Use established native libraries through audited bindings when appropriate rather than reimplementing every numerical algorithm.

### Production ecosystem

Add package management, formatter, test runner, debugger hooks, editor/LSP support, documentation generation, cross-platform release automation, and compatibility/versioning policy.

## 9. Delivery sequence and acceptance gates

1. **Runtime foundation:** module registry contract, structured runtime errors, capability metadata, host adapter documentation, and regression tests.
2. **Standard library:** JSON/time/process/network primitives with security and resource limits.
3. **Web runtime:** HTTP client, then WebSocket and server.
4. **Application runtime:** lifecycle and Android host integration.
5. **Device runtime expansion:** camera bridge and additional device modules.
6. **AI runtime:** model/provider interfaces and streaming.
7. **Science runtime:** numerical and domain libraries.
8. **Ecosystem tooling:** packages, formatter, debugger, LSP, documentation and release process.

A stage is not complete because its API names exist. It is complete only when tests cover success, unsupported platforms, invalid input, permission denial, cancellation, timeout, cleanup, and relevant failure cases. Platform claims must be backed by platform-specific CI; physical-device behavior must be reported separately from CI.

## 10. Current implementation boundary

At the time this architecture was written, Mosh already has an interpreted Rust core, a basic source-module import system, a capability-oriented `device.*` foundation, Linux/Android telemetry readers, Rust embedding, a C ABI, Android JNI/Kotlin integration, and a CameraX host component.

The current device-facing Mosh API exposes battery, memory/RAM, CPU, and mounted-volume telemetry plus capability/permission introspection. CameraX exists as a Kotlin host component but camera frames are not yet exposed to Mosh scripts. Most namespaces in this document are future architecture, not implemented functionality.

The immediate engineering priority is to strengthen the module/runtime contracts without destabilizing the working interpreter, then implement domain modules end-to-end in small verified increments.
