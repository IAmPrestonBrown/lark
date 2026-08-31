/* Every operator that C11 6.5 defines. Rule S-1. Test type T3. */

#include <stddef.h>

struct box {
    int value;
    int items[3];
};

static int identity(int value)
{
    return value;
}

int main(void)
{
    int a = 6;
    int b = 3;
    int result = 0;
    int *pointer = &a;
    struct box container = { .value = 1, .items = { 1, 2, 3 } };
    struct box *handle = &container;

    /* Arithmetic and relational. */
    result = a + b - a * b / b % b;
    result = (a < b) + (a > b) + (a <= b) + (a >= b) + (a == b) + (a != b);

    /* Bitwise and shift. */
    result = (a & b) | (a ^ b);
    result = (a << 1) >> 1;
    result = ~a;

    /* Logical. */
    result = (a && b) || (!a);

    /* Assignment, in every compound form. */
    result = a;
    result += b;
    result -= b;
    result *= b;
    result /= b;
    result %= b;
    result <<= 1;
    result >>= 1;
    result &= b;
    result |= b;
    result ^= b;

    /* Increment and decrement, prefix and postfix. */
    result = a++ + ++a;
    result = a-- - --a;

    /* Conditional and comma. */
    result = a ? b : a;
    result = (a, b, a + b);

    /* Unary, sizeof, and alignof. */
    result = -a + +b;
    result = (int) sizeof(int);
    result = (int) sizeof a;
    result = (int) _Alignof(double);

    /* Indirection, address, member access, and subscript. */
    result = *pointer;
    result = container.value + handle->value;
    result = container.items[1] + handle->items[2];

    /* A call, a cast, and a compound literal. */
    result = identity((int) 1.5);
    result = (struct box){ .value = 9 }.value;

    return result - 9;
}
