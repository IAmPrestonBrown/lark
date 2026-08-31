/* Struct, union, enum, and bit field declarations. Rule S-1. Test type T3. */

struct simple {
    int first;
    char second;
};

struct with_bit_fields {
    unsigned int flag : 1;
    unsigned int small : 3;
    unsigned int : 0;
    unsigned int rest : 8;
};

struct nested {
    struct simple inner;
    struct {
        int anonymous_member;
    } unnamed;
};

union overlapping {
    int as_int;
    char as_bytes[4];
};

enum plain_enum { first_value, second_value, third_value };
enum with_values { one = 1, ten = 10, eleven };
enum trailing_comma { alpha, beta, };

struct with_flexible_array {
    int length;
    int items[];
};

struct simple global_simple = { 1, 'a' };
union overlapping global_union = { .as_int = 5 };
enum with_values global_enum = ten;

int main(void)
{
    struct nested local;
    local.inner.first = 1;
    local.unnamed.anonymous_member = 2;
    return local.inner.first + local.unnamed.anonymous_member - 3;
}
