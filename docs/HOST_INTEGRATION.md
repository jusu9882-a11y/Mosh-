# Embedding Mosh in native applications

## Supported integration surfaces

Mosh currently provides two host integration surfaces:

1. A Rust library (moshc) that exposes Interpreter and DeviceBackend.
2. A C ABI shared library (libmoshc.so on Linux/Android, with platform-specific library naming on other targets) with a public header at compiler/include/mosh.h.

The C ABI provides interpreter creation, source evaluation, version retrieval, and owned-string cleanup. It does not yet include JNI/Kotlin, Swift, or Windows-specific binding layers.

## Build

From the repository:

~~~sh
cd compiler
cargo build --release --lib
~~~

For Android, build for the intended Android NDK Rust target and package the resulting shared object in the matching ABI directory of the Android application. The NDK toolchain and target configuration must be installed by the host build environment; a desktop Linux build is not an Android binary.

## C API

Include compiler/include/mosh.h and link the Mosh shared library.

~~~c
#include "mosh.h"
#include <stdio.h>

int main(void) {
    MoshRuntime *runtime = mosh_runtime_new();
    if (runtime == NULL) return 1;

    char *result = mosh_runtime_eval(runtime, "let answer = 40 + 2\nanswer\n");
    if (result != NULL) {
        puts(result); /* JSON result */
        mosh_string_free(result);
    }

    mosh_runtime_free(runtime);
    return 0;
}
~~~

Every string returned by mosh_runtime_eval or mosh_runtime_version is owned by the Mosh library and must be released exactly once with mosh_string_free. Do not call free() from the host allocator.

Evaluation returns a JSON object with either ok: true and a string-valued value, or ok: false and an error message. The value is the final expression result. Mosh print() writes to the process standard output; this ABI version does not capture or redirect it.

## Threading and lifetime

- A runtime handle is single-threaded. Serialize all evaluations using one handle.
- Do not free a handle while an evaluation is running.
- Do not reuse a handle after mosh_runtime_free.
- Pass valid NUL-terminated UTF-8 source strings.
- Never pass arbitrary or stale pointers to the ABI.
- Panics are caught at the ABI boundary and converted to an error JSON result where possible.

## Device backends

The default runtime uses the built-in Linux/Android procfs/sysfs backend where available. Rust hosts can inject a custom DeviceBackend through Interpreter::with_device_backend.

The C ABI currently uses the default backend; a C callback table for custom native device backends is not yet part of the ABI. Android framework APIs, iOS protected-resource APIs, and Windows WinRT/Win32 APIs therefore still require their own bridge implementations and physical-device verification.

## Security

The interpreter is not a sandbox. Filesystem operations and imported modules run with the host process's operating-system privileges. Do not execute untrusted Mosh source in a privileged host process.
