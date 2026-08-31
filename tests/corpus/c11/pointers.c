/* Declarators that nest a pointer, an array, and a function. Rule S-1 and
   rule L-6. Test type T3. */

typedef unsigned long size_type;

struct table {
    const char *name;
    int (*compare)(const void *, const void *);
    void (*release)(void *);
};

static int compare_int(const void *left, const void *right)
{
    const int *a = (const int *) left;
    const int *b = (const int *) right;
    return *a - *b;
}

static void release_nothing(void *item)
{
    (void) item;
}

struct table global_table = { "numbers", compare_int, release_nothing };

int (*returns_function_pointer(void))(const void *, const void *);
int *(*array_of_returning_pointer[2])(void);
char *(*const fixed_slot)(int) = 0;

int main(void)
{
    int values[3] = { 3, 1, 2 };
    size_type count = (size_type) 3;
    int (*compare)(const void *, const void *) = global_table.compare;
    int result = compare(&values[0], &values[1]);
    return result - 2 + (int) count - 3;
}
