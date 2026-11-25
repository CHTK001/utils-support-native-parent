/*
 * anydoc_ffi C-ABI smoke test for Linux (Docker / CI).
 * dlopen the cdylib, dlsym the 5 exported symbols, exercise:
 *   version -> convert_bytes(RTF, auto) -> error path (garbage bytes).
 * Exit 0 = all assertions passed.
 */
#include <dlfcn.h>
#include <stdio.h>
#include <string.h>

typedef const char *(*version_fn)(void);
typedef const char *(*last_error_fn)(void);
typedef void (*free_string_fn)(char *);
typedef char *(*convert_file_fn)(const char *);
typedef char *(*convert_bytes_fn)(const unsigned char *, size_t, const char *);

int main(int argc, char **argv) {
    if (argc < 2) {
        fprintf(stderr, "usage: %s <libanydoc_ffi.so path>\n", argv[0]);
        return 2;
    }
    void *h = dlopen(argv[1], RTLD_NOW);
    if (!h) {
        fprintf(stderr, "ASSERT FAIL dlopen: %s\n", dlerror());
        return 1;
    }
    printf("ASSERT ok   dlopen %s\n", argv[1]);

    version_fn version = (version_fn)dlsym(h, "anydoc_version");
    last_error_fn last_error = (last_error_fn)dlsym(h, "anydoc_last_error");
    free_string_fn free_string = (free_string_fn)dlsym(h, "anydoc_free_string");
    convert_file_fn convert_file = (convert_file_fn)dlsym(h, "anydoc_convert_file");
    convert_bytes_fn convert_bytes = (convert_bytes_fn)dlsym(h, "anydoc_convert_bytes");
    if (!version || !last_error || !free_string || !convert_file || !convert_bytes) {
        fprintf(stderr, "ASSERT FAIL dlsym: %s\n", dlerror());
        return 1;
    }
    printf("ASSERT ok   dlsym 5 exports\n");
    printf("VERSION=%s\n", version());

    /* RTF content signature {\rtf1 -> auto-detect must work */
    const char *rtf = "{\\rtf1\\ansi\\deff0 Hello from anydoc Docker smoke}";
    char *md = convert_bytes((const unsigned char *)rtf, strlen(rtf), NULL);
    if (!md) {
        fprintf(stderr, "ASSERT FAIL convert_bytes(rtf): %s\n", last_error());
        return 1;
    }
    printf("MD=%s\n", md);
    int ok = strstr(md, "Hello from anydoc") != NULL;
    free_string(md);
    if (!ok) {
        fprintf(stderr, "ASSERT FAIL markdown output mismatch\n");
        return 1;
    }
    printf("ASSERT ok   rtf convert\n");

    /* error path: garbage bytes must return NULL and set last_error */
    const unsigned char garbage[10] = {0x00, 0x01, 0x02, 'g', 'a', 'r', 'b', 'a', 'g', 'e'};
    char *bad = convert_bytes(garbage, sizeof(garbage), NULL);
    if (bad) {
        free_string(bad);
        fprintf(stderr, "ASSERT FAIL garbage input unexpectedly converted\n");
        return 1;
    }
    const char *err = last_error();
    if (!err || !strstr(err, "unrecognized")) {
        fprintf(stderr, "ASSERT FAIL last_error: %s\n", err ? err : "(null)");
        return 1;
    }
    printf("ASSERT ok   error path: %s\n", err);

    dlclose(h);
    printf("DOCKER_SMOKE_OK\n");
    return 0;
}
