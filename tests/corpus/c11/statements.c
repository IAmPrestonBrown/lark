/* Every statement form that C11 6.8 allows. Rule S-1 requires that Lark
   accepts the file with no diagnostic. This fixture is test type T3. */

#include <stddef.h>

static int classify(int value)
{
    switch (value) {
    case 0:
        return 10;
    case 1:
    case 2:
        return 20;
    default:
        break;
    }
    return 30;
}

int main(void)
{
    int total = 0;
    int index = 0;

    /* Selection. */
    if (total == 0) {
        total = 1;
    } else if (total == 1) {
        total = 2;
    } else {
        total = 3;
    }

    /* Iteration, in all three forms. */
    while (index < 3) {
        index += 1;
    }
    do {
        index -= 1;
    } while (index > 0);
    for (int i = 0; i < 3; i += 1) {
        if (i == 1) {
            continue;
        }
        total += i;
    }
    for (;;) {
        break;
    }

    /* A nested block makes a new scope. */
    {
        int shadowed = total;
        {
            int inner = shadowed;
            total = inner;
        }
    }

    /* A jump and a label. */
    if (total > 100) {
        goto done;
    }
    total += classify(1);

done:
    ;
    _Static_assert(sizeof(int) >= 2, "int holds at least two bytes");
    return total - 22;
}
