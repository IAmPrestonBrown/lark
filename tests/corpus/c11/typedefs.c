/* Typedef declarations, including the forms that make a name a type only after
   the parser reads the declaration. Rule L-6 and rule S-1. Test type T3. */

typedef int integer;
typedef integer *integer_pointer;
typedef integer array_of_four[4];
typedef int (*binary_operation)(int, int);
typedef void (*callback)(void *context);

typedef struct point {
    int x;
    int y;
} point;

typedef union value {
    int as_int;
    double as_double;
} value;

typedef enum color {
    red,
    green,
    blue
} color;

typedef struct node node;

struct node {
    int value;
    node *next;
};

integer count = 3;
integer_pointer to_count = &count;
point origin = { 0, 0 };
color favorite = blue;

static int add(int a, int b)
{
    return a + b;
}

int main(void)
{
    binary_operation operation = add;
    /* A cast to a typedef name needs the name table. Rule L-6. */
    integer total = (integer) operation(1, 2);
    array_of_four numbers = { 1, 2, 3, 4 };
    value holder;
    holder.as_int = numbers[0];
    return total - 3 + holder.as_int - 1;
}
