/* Generic selection. C11 6.5.1.1. Rule S-1. Test type T3. */

#define name_of(x) _Generic((x), \
    int: 1, \
    long: 2, \
    float: 3, \
    double: 4, \
    char *: 5, \
    default: 0)

static int width_of(int value)
{
    return _Generic(value, int: 4, long: 8, default: 0);
}

int main(void)
{
    int an_int = 1;
    double a_double = 1.0;
    char *a_string = "text";

    int total = name_of(an_int) + name_of(a_double) + name_of(a_string);
    total += _Generic(1, int: 0, default: 100);
    total += width_of(an_int);
    return total - 14;
}
