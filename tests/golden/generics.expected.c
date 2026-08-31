/* Generated from generics.lark. Do not edit. */
#include "generics.lark.h"

typedef struct Person Person;

extern const lark_typeinfo lk_generics__Person__ti;

/* lark: generic instantiations */

/* lark: forward declarations */
static void uses(void);

// The emitted C for a generic. Rule G-1 gives one definition per distinct
// argument set. Rule G-10 gives a field map only to an instantiation that
// holds a managed field.
// covers: G-1, G-10, G-13, X-5a

#line 6 "generics.lark"
struct Person {
    char* name;
};

#line 10 "generics.lark"
/* lark: generic Box<T>, one definition per instantiation */

#line 14 "generics.lark"
static void uses(void) {
    /* lark: shadow stack frame, 1 managed locals, 0 temporaries */
    struct { lark_frame_hdr h; void **s[1]; void *t[1]; } _lk_frame;
    _lk_frame.h.slots = _lk_frame.s;
    _lk_frame.h.temps = _lk_frame.t;
    _lk_frame.h.nslots = 0u;
    _lk_frame.h.ntemps = 0u;
    _lk_frame.t[0] = 0;
    lark_frame_push(&_lk_frame.h);

    lk_generics__Box__i plain;
    lk_generics__Box__G6Person* boxed; _lk_frame.s[0] = (void **)&boxed; _lk_frame.h.nslots = 1u;
    lark_frame_pop(&_lk_frame.h);
}

/* lark: tables */

static const uint32_t lk_generics__Person__ti_ptrs[] = { (uint32_t)offsetof(Person, name) };
const lark_typeinfo lk_generics__Person__ti = {
    "Person", sizeof(Person), (uint32_t)_Alignof(Person), 1u, lk_generics__Person__ti_ptrs, 0u, 0
};

/* lark: generic bodies */
static const uint32_t lk_generics__Box__G6Person__ti_ptrs[] = { (uint32_t)offsetof(lk_generics__Box__G6Person, value) };
const lark_typeinfo lk_generics__Box__G6Person__ti = {
    "lk_generics__Box__G6Person", sizeof(lk_generics__Box__G6Person), (uint32_t)_Alignof(lk_generics__Box__G6Person), 1u, lk_generics__Box__G6Person__ti_ptrs, 0u, 0
};
const lark_typeinfo lk_generics__Box__i__ti = {
    "lk_generics__Box__i", sizeof(lk_generics__Box__i), (uint32_t)_Alignof(lk_generics__Box__i), 0u, 0, 0u, 0
};
