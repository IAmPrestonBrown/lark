/* The type descriptors that the runtime tests share. */

#include "test_types.h"

static const uint32_t NODE_PTRS[] = {
    (uint32_t)offsetof(node, left),
    (uint32_t)offsetof(node, right),
};

static const uint32_t BIG_PTRS[] = {
    (uint32_t)offsetof(big, next),
};

const lark_typeinfo PLAIN_TYPE = {
    "plain", sizeof(plain), (uint32_t)_Alignof(plain), 0, NULL, 0, NULL,
};

const lark_typeinfo NODE_TYPE = {
    "node", sizeof(node), (uint32_t)_Alignof(node), 2, NODE_PTRS, 0, NULL,
};

const lark_typeinfo BIG_TYPE = {
    "big", sizeof(big), (uint32_t)_Alignof(big), 1, BIG_PTRS, 0, NULL,
};

void test_start(lark_roots roots) {
    lark_gc_config config = lark_gc_config_default();
    config.roots = roots;
    /* A large limit keeps a collection from starting on its own, so every test
     * decides when the collector runs. */
    config.heap_limit = (size_t)64u * 1024u * 1024u;
    lark_startup_with(config);
}

bool test_roots_supported(lark_roots roots) {
    if (roots == LARK_ROOTS_SHADOW_STACK) {
        return true;
    }
    return !lark_gc_capabilities().moving;
}
