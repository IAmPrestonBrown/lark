/* The test harness.
 *
 * The harness prints one line per case when LARK_TEST_VERBOSE is set, and only
 * the failures and the totals otherwise. A gate that runs the suite three
 * times stays readable. */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "lark_test.h"

static const char *current_name;
static int checks_run;
static int checks_failed;
static int cases_run;
static int cases_failed;
static int case_failures;
static int cases_skipped;
static bool case_skipped;
static const char *skip_reason;
static int verbose = -1;

/* Reports whether the harness prints one line per case. */
static bool is_verbose(void) {
    if (verbose < 0) {
        const char *value = getenv("LARK_TEST_VERBOSE");
        verbose = (value != NULL && strcmp(value, "0") != 0) ? 1 : 0;
    }
    return verbose == 1;
}

void lark_test_run_case(const char *name, void (*body)(void)) {
    current_name = name;
    case_failures = 0;
    case_skipped = false;
    skip_reason = NULL;
    cases_run += 1;
    if (is_verbose()) {
        /* A flush before the body names the test that hangs. */
        printf("run  %s\n", name);
        fflush(stdout);
    }
    body();
    if (case_failures > 0) {
        cases_failed += 1;
        printf("FAIL %s\n", name);
        fflush(stdout);
    } else if (case_skipped) {
        cases_skipped += 1;
        if (is_verbose()) {
            printf("skip %s (%s)\n", name, skip_reason == NULL ? "" : skip_reason);
            fflush(stdout);
        }
    } else if (is_verbose()) {
        printf("ok   %s\n", name);
        fflush(stdout);
    }
}

bool lark_test_check(bool condition, const char *text, const char *file, int line) {
    checks_run += 1;
    if (condition) {
        return true;
    }
    checks_failed += 1;
    case_failures += 1;
    printf("     %s:%d: in %s: %s\n", file, line, current_name, text);
    fflush(stdout);
    return false;
}

void lark_test_skip(const char *reason) {
    case_skipped = true;
    skip_reason = reason;
}

int lark_test_report(void) {
    printf("%d cases, %d failed, %d skipped. %d checks, %d failed.\n",
           cases_run, cases_failed, cases_skipped, checks_run, checks_failed);
    fflush(stdout);
    return cases_failed == 0 ? 0 : 1;
}
