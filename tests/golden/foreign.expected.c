/* Generated from foreign.lark. Do not edit. */
#include "foreign.lark.h"

typedef struct Person Person;

extern const lark_typeinfo lk_foreign__Person__ti;

/* lark: forward declarations */
int main(void);

/* lark: foreign call helpers, rule M-19 */
static int lk_leave__i(int lk_value) {
    lark_leave_safe();
    return lk_value;
}

// The emitted C for a foreign call. Rule M-19 puts the thread in the safe
// state around a `gc_safe` call, and rule M-20 leaves a `gc_leaf` call alone.
// Rule M-21 makes an unmarked extern safe.
// covers: M-19, M-20, M-21

#line 6 "foreign.lark"
struct Person {
    int age;
};

// Rule M-21. No marker, so the safe contract.
#line 11 "foreign.lark"
int unmarked_extern(int value);

#line 13 "foreign.lark"
int marked_safe(int value);

#line 15 "foreign.lark"
int marked_leaf(int value);

#line 17 "foreign.lark"
int main(void) {
    /* lark: runtime startup, rule I-3 */
    lark_gc_config _lk_config = lark_gc_config_default();
    _lk_config.roots = LARK_ROOTS_SHADOW_STACK;
    _lk_config.torture = false;
    lark_startup_with(_lk_config);

    /* lark: shadow stack frame, 1 managed locals, 1 temporaries */
    struct { lark_frame_hdr h; void **s[1]; void *t[1]; } _lk_frame;
    _lk_frame.h.slots = _lk_frame.s;
    _lk_frame.h.temps = _lk_frame.t;
    _lk_frame.h.nslots = 0u;
    _lk_frame.h.ntemps = 1u;
    _lk_frame.t[0] = 0;
    lark_frame_push(&_lk_frame.h);

    Person* p = (_lk_frame.t[0] = lark_new(&lk_foreign__Person__ti, 0), *(Person *)_lk_frame.t[0] = (Person){ .age = 1 }, (Person *)_lk_frame.t[0]); _lk_frame.s[0] = (void **)&p; _lk_frame.h.nslots = 1u;
    int total = (lark_enter_safe(), lk_leave__i(unmarked_extern(1))) + (lark_enter_safe(), lk_leave__i(marked_safe(2))) + marked_leaf(3);
    { int _lk_result = (total + p->age); lark_frame_pop(&_lk_frame.h); return _lk_result; }
}

/* lark: tables */

const lark_typeinfo lk_foreign__Person__ti = {
    "Person", sizeof(Person), (uint32_t)_Alignof(Person), 0u, 0, 0u, 0
};
