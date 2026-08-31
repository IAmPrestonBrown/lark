/* Storage classes, qualifiers, and alignment. Rule S-1. Test type T3. */

#include <stdint.h>

static int internal_linkage;
extern int external_linkage;
_Thread_local int per_thread;
static _Thread_local int private_per_thread;

const char *const constant_pointer_to_constant = "text";
volatile unsigned int hardware_register;
_Atomic int shared_counter;
_Atomic(int) also_atomic;

_Alignas(16) char aligned_buffer[64];
_Alignas(double) char aligned_to_a_type[8];

static inline int small(int a)
{
    return a + 1;
}

_Noreturn void never_returns(void);

int restricted(int *restrict left, int *restrict right);
int sized_parameter(int values[static 4]);
int qualified_parameter(char text[const 8]);
int unsized_parameter(int values[]);

typedef int8_t small_signed;
typedef uint64_t big_unsigned;

small_signed narrow = -1;
big_unsigned wide = 1;

int main(void)
{
    register int fast = small(1);
    auto int ordinary = 1;
    return fast - ordinary - 1;
}
