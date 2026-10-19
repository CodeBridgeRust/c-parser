/*
 * embed_from_c.c — minimal C host for the `c-parser-ffi` cdylib.
 *
 * Demonstrates the JSON-only FFI contract documented in docs/api.md §8:
 * the boundary exchanges only JSON strings (pointer + length), and the
 * returned string is owned by Rust's allocator.
 *
 * Build & run (from the workspace root):
 *
 *   cargo build --release -p c-parser-ffi
 *   cc -std=c11 -Wall -Wextra -o /tmp/embed_from_c \
 *       crates/c-parser-ffi/examples/embed_from_c.c \
 *       -L target/release -lc_parser_ffi -Wl,-rpath,target/release
 *   /tmp/embed_from_c
 *
 * Memory contract:
 *   - The pointer returned by `c_parser_parse` MUST be freed with
 *     `c_parser_free_string` (it is allocated by Rust's allocator).
 *   - Mixing with `free()`/`delete` is undefined behavior.
 *   - Passing `NULL` to `c_parser_free_string` is a safe no-op.
 */

#include <stdint.h>
#include <stdio.h>
#include <string.h>

/* Exact signatures of the exported symbols (see crates/c-parser-ffi/src/lib.rs). */
extern char *c_parser_parse(const uint8_t *source_ptr, size_t source_len,
                            const uint8_t *config_ptr, size_t config_len);
extern void c_parser_free_string(char *ptr);

int main(void) {
    static const char source[] =
        "int add(int a, int b) { return a + b; }\n"
        "int main(void) { return add(1, 2); }\n";

    static const char config[] = "{}"; /* reserved; ignored, but valid JSON */

    char *out = c_parser_parse((const uint8_t *)source, strlen(source),
                               (const uint8_t *)config, strlen(config));
    if (out == NULL) {
        fprintf(stderr, "c_parser_parse returned NULL\n");
        return 1;
    }

    /* The result is a self-contained JSON document with language "c". */
    if (strstr(out, "\"language\":\"c\"") == NULL) {
        fprintf(stderr, "unexpected output: %s\n", out);
        c_parser_free_string(out);
        return 1;
    }

    printf("OK: parsed JSON (%zu bytes)\n", strlen(out));

    /* Free through Rust's allocator. */
    c_parser_free_string(out);

    /* Passing NULL must be a safe no-op. */
    c_parser_free_string(NULL);
    puts("OK: c_parser_free_string(NULL) did not crash");

    return 0;
}
