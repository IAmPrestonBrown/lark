/* Old style function definitions. C11 6.9.1. Rule S-1. Test type T3. */

int add(a, b)
int a;
int b;
{
    return a + b;
}

int scale(value, factor)
int value;
int factor;
{
    return value * factor;
}

/* A definition with no declaration list at all. */
int one()
{
    return 1;
}

int main(void)
{
    return add(1, 2) + scale(2, 3) + one() - 10;
}
