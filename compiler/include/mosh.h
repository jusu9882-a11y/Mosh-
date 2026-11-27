#ifndef MOSH_H
#define MOSH_H

#ifdef __cplusplus
extern "C" {
#endif

typedef struct MoshRuntime MoshRuntime;

/* Create and destroy a single-threaded interpreter runtime. */
MoshRuntime *mosh_runtime_new(void);
void mosh_runtime_free(MoshRuntime *runtime);

/*
 * Evaluate a complete NUL-terminated UTF-8 Mosh source string.
 * Returns an owned JSON string:
 *   {"ok":true,"value":"..."}
 *   {"ok":false,"error":"..."}
 * Release every returned string with mosh_string_free().
 */
char *mosh_runtime_eval(MoshRuntime *runtime, const char *source_utf8);

/* Returns an owned version string; release it with mosh_string_free(). */
char *mosh_runtime_version(void);

/* Release a string returned by the Mosh C ABI. */
void mosh_string_free(char *value);

#ifdef __cplusplus
}
#endif

#endif
