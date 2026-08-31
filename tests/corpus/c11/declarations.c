/* Every declarator form that C11 6.7.6 allows. Rule S-1 requires that Lark
   accepts the file with no diagnostic. This fixture is test type T3. */

int plain;
int *pointer;
int **pointer_to_pointer;
int array[4];
int array_of_array[2][3];
int *array_of_pointer[4];
int (*pointer_to_array)[4];
int (*pointer_to_function)(int, int);
int *(*returns_pointer)(void);
int (*array_of_function_pointer[3])(void);

const int read_only = 1;
volatile int changes;
const volatile int both;
int *const constant_pointer = &plain;
const int *pointer_to_constant;

extern int elsewhere;
static int private_to_file;
static const int private_constant = 2;

int with_initializer = 7;
int list_a = 1, list_b = 2, *list_c = &plain;

int function_declaration(int, int);
int named_parameters(int first, int second);
int no_parameters(void);
int variadic(const char *format, ...);
void returns_nothing(void);

struct forward_reference;
struct forward_reference *incomplete_pointer;

int main(void)
{
    return 0;
}
