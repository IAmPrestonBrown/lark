/* Type descriptors that the runtime tests share.
 *
 * The transpiler emits one of these per managed type. A test writes them by
 * hand, so the runtime tests need no transpiler. */

#ifndef LARK_TEST_TYPES_H
#define LARK_TEST_TYPES_H

#include <stddef.h>

#include "lark_rt.h"

/* A type with no managed field. */
typedef struct plain {
    int value;
    int other;
} plain;

/* A node of a graph, with two managed fields. */
typedef struct node {
    struct node *left;
    struct node *right;
    int value;
} node;

/* A type larger than every size class, so it needs a page of its own. */
typedef struct big {
    char bytes[100000];
    struct big *next;
} big;

extern const lark_typeinfo PLAIN_TYPE;
extern const lark_typeinfo NODE_TYPE;
extern const lark_typeinfo BIG_TYPE;

/* Starts the runtime with one root mechanism, for one test. */
void test_start(lark_roots roots);

/* Reports whether the linked collector accepts a root mechanism.
 *
 * A moving collector must write a new address into every root, and rule M-13
 * conservative scanning cannot say which words are roots, so it accepts only
 * rule M-10 shadow stack roots. */
bool test_roots_supported(lark_roots roots);

/* Declares a shadow stack frame and pushes it.
 *
 * This is the shape that the emitter produces. The address slots hold the
 * address of each managed local. The value slots hold each managed temporary.
 * See rules M-10 and M-27.
 *
 * C11 has no zero length array, so each array holds at least one entry. The
 * counts in the header say how many the collector reads. */
#define LARK_FRAME_MIN(n) ((n) < 1 ? 1 : (n))

#define TEST_FRAME(frame_name, local_count, temp_count)                        \
    struct {                                                                   \
        lark_frame_hdr h;                                                      \
        void **s[LARK_FRAME_MIN(local_count)];                                 \
        void *t[LARK_FRAME_MIN(temp_count)];                                   \
    } frame_name;                                                              \
    do {                                                                       \
        frame_name.h.slots = frame_name.s;                                     \
        frame_name.h.temps = frame_name.t;                                     \
        frame_name.h.nslots = 0;                                               \
        frame_name.h.ntemps = (temp_count);                                    \
        for (uint32_t frame_index = 0; frame_index < LARK_FRAME_MIN(temp_count); \
             frame_index += 1) {                                               \
            frame_name.t[frame_index] = NULL;                                  \
        }                                                                      \
        lark_frame_push(&frame_name.h);                                        \
    } while (0)

#endif /* LARK_TEST_TYPES_H */
