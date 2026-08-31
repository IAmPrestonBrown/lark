/* A file that uses several system headers. Rule C-1 reads them, rule C-2 puts
   the names in this module, and rule L-6 needs the types for the casts. Rule
   S-1 requires no diagnostic. Test type T3. */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <stddef.h>

static const char *const message = "corpus";

int main(void)
{
    size_t length = strlen(message);
    uint32_t narrowed = (uint32_t) length;
    FILE *out = stdout;
    ptrdiff_t difference = (ptrdiff_t) 0;

    if (narrowed != (uint32_t) 6) {
        fprintf(stderr, "unexpected length\n");
        return EXIT_FAILURE;
    }
    fprintf(out, "%s has %zu bytes\n", message, length);
    return (int) difference;
}
