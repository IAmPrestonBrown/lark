/* A small test harness for the runtime.
 *
 * The runtime is plain C with no transpiler involved, so its tests are plain C
 * too. Test type T8 in docs/test-strategy.md describes them. */

#ifndef LARK_TEST_H
#define LARK_TEST_H

#include <stdbool.h>
#include <stddef.h>

/* Runs one test and reports the result. */
void lark_test_run_case(const char *name, void (*body)(void));

/* Records one check. Returns the condition, so a caller can stop early. */
bool lark_test_check(bool condition, const char *text, const char *file, int line);

/* Records that the running case does not apply, and says why. */
void lark_test_skip(const char *reason);

/* Prints the totals and returns the process exit code. */
int lark_test_report(void);

/* Checks a condition. */
#define CHECK(cond) lark_test_check((cond), #cond, __FILE__, __LINE__)

/* Checks a condition and leaves the test when it fails. */
#define REQUIRE(cond)                                        \
    do {                                                     \
        if (!lark_test_check((cond), #cond, __FILE__, __LINE__)) { \
            return;                                          \
        }                                                    \
    } while (0)

/* Leaves the test when a capability is absent, and records why.
 *
 * A collector supplies a different set of capabilities, and a test that asks
 * for one the collector lacks is not a failure. Rule R-1 gives the same answer
 * to the transpiler at build time. */
#define SKIP_UNLESS(cond, reason)   \
    do {                            \
        if (!(cond)) {              \
            lark_test_skip(reason); \
            return;                 \
        }                           \
    } while (0)

/* Runs one test by its function name. */
#define RUN(fn) lark_test_run_case(#fn, fn)

#endif /* LARK_TEST_H */
