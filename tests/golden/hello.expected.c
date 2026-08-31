/* Generated from hello.lark. Do not edit. */
#include "hello.lark.h"

typedef struct Point Point;


/* lark: forward declarations */
static int area(struct Point p);
int report(struct Point p);

// A fixture for test type T5.
// The harness compares the emitted C against hello.expected.c.
// The comparison shows the cost of every construct.
// covers: X-2, X-4, X-4a, X-5, X-5b

#line 6 "hello.lark"
#include "stdio.lark.h"

#line 8 "hello.lark"
/* lark: Point is in hello.lark.h */

#line 13 "hello.lark"
static int calls = 0;

#line 15 "hello.lark"
static int area(struct Point p) {
    calls = calls + 1;
    return p.x * p.y;
}

#line 20 "hello.lark"
int report(struct Point p) {
    return area(p);
}
