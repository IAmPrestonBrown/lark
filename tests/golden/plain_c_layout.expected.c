/* Generated from plain_c_layout.lark. Do not edit. */
#include "plain_c_layout.lark.h"

typedef struct node node;


#include <stddef.h>

/* lark: local types */
typedef struct node node;
typedef int(* visit_fn)(node* item, size_t depth);

/* lark: forward declarations */
static int visit_one(node* item, size_t depth);
static int walk(node* head, visit_fn visit);

// The emitted C for a plain C11 file.
//
// Rule C-3a puts every `#include` first, rule X-6a puts a local typedef next,
// and the forward declarations come after both, because each can name a type
// from either. Rule X-5d leaves a symbol that a header declares external.
// covers: C-3a, X-5d, X-6a


#line 9 "plain_c_layout.lark"
/* lark: `node` is declared above */

#line 11 "plain_c_layout.lark"
/* lark: `visit_fn` is declared above */

#line 13 "plain_c_layout.lark"
struct node {
    int value;
    node* next;
};

#line 18 "plain_c_layout.lark"
static int visit_one(node* item, size_t depth)
{
    return item->value +(int) depth;
}

#line 23 "plain_c_layout.lark"
static int walk(node* head, visit_fn visit)
{
    int total = 0;
    for (node* item = head; item != NULL; item = item->next) {
        total += visit(item, 0);
    }
    return total + visit_one(head, 1);
}
