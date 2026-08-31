/* Generated from interfaces.lark. Do not edit. */
#include "interfaces.lark.h"

typedef struct Person Person;

typedef struct lk_interfaces__Greet__vtable {
    void (*by_value)(void *);
    void (*by_pointer)(void *);
} lk_interfaces__Greet__vtable;
typedef struct Greet { void *obj; const lk_interfaces__Greet__vtable *vt; } Greet;
extern const char lk_interfaces__Greet__id;
extern const lk_interfaces__Greet__vtable lk_interfaces__Greet__Person__vt;
extern const lark_typeinfo lk_interfaces__Person__ti;

/* lark: forward declarations */
static void lk_interfaces__Greet__Person__by_value(Person this);
static void lk_interfaces__Greet__Person__by_pointer(Person* this);
static void calls(Person* p);

// The emitted C for an interface. Rule O-19 gives a direct call for a concrete
// receiver, and rule O-18 adapts the receiver to the form the method declares.
// covers: O-18, O-19, T-12

#line 5 "interfaces.lark"
struct Person {
    int age;
};

#line 9 "interfaces.lark"
/* lark: interface Greet */

#line 14 "interfaces.lark"

static void lk_interfaces__Greet__Person__by_value(Person this) { }
static void lk_interfaces__Greet__Person__by_pointer(Person* this) { }

#line 19 "interfaces.lark"
static void calls(Person* p) {
    /* lark: shadow stack frame, 1 managed locals, 0 temporaries */
    struct { lark_frame_hdr h; void **s[1]; void *t[1]; } _lk_frame;
    _lk_frame.h.slots = _lk_frame.s;
    _lk_frame.h.temps = _lk_frame.t;
    _lk_frame.h.nslots = 0u;
    _lk_frame.h.ntemps = 0u;
    _lk_frame.t[0] = 0;
    lark_frame_push(&_lk_frame.h);
    _lk_frame.s[0] = (void **)&p; _lk_frame.h.nslots = 1u;

    lk_interfaces__Greet__Person__by_value(*p);
    lk_interfaces__Greet__Person__by_pointer(p);
    lark_frame_pop(&_lk_frame.h);
}

/* lark: tables */
const char lk_interfaces__Greet__id = 0;

static void lk_interfaces__Greet__Person__by_value__thunk(void *lk_self) {
    lk_interfaces__Greet__Person__by_value(*(Person *)lk_self);
}
static void lk_interfaces__Greet__Person__by_pointer__thunk(void *lk_self) {
    lk_interfaces__Greet__Person__by_pointer((Person *)lk_self);
}
const lk_interfaces__Greet__vtable lk_interfaces__Greet__Person__vt = {
    lk_interfaces__Greet__Person__by_value__thunk,
    lk_interfaces__Greet__Person__by_pointer__thunk,
};

static const lark_itable_ent lk_interfaces__Person__itabs[] = {
    { &lk_interfaces__Greet__id, &lk_interfaces__Greet__Person__vt },
};

const lark_typeinfo lk_interfaces__Person__ti = {
    "Person", sizeof(Person), (uint32_t)_Alignof(Person), 0u, 0, (uint32_t)(sizeof(lk_interfaces__Person__itabs) / sizeof(lk_interfaces__Person__itabs[0])), lk_interfaces__Person__itabs
};
