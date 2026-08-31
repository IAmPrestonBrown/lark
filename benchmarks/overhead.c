/* The plain C half of the `overhead` benchmark.
 *
 * The algorithm matches `overhead.lark` line for line. This half calls
 * `malloc` and `free`, so the difference between the two runs is what managed
 * memory costs: the shadow stack of rule M-10, the polls of rule M-16, and the
 * collector itself.
 *
 * This file builds with `cc` alone. See `run.sh`. */

#define _POSIX_C_SOURCE 199309L

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct Node {
    struct Node *next;
    int value;
} Node;

static double now_seconds(void) {
    struct timespec moment;
    clock_gettime(CLOCK_MONOTONIC, &moment);
    return (double)moment.tv_sec + (double)moment.tv_nsec / 1e9;
}

static long long round_trip(int length) {
    Node *head = NULL;
    for (int index = 0; index < length; index += 1) {
        Node *item = malloc(sizeof *item);
        if (item == NULL) {
            abort();
        }
        item->next = head;
        item->value = index;
        head = item;
    }
    long long total = 0;
    for (Node *walk = head; walk != NULL; walk = walk->next) {
        total += (long long)walk->value;
    }
    /* The managed half drops the list here and the collector reclaims it. */
    while (head != NULL) {
        Node *next = head->next;
        free(head);
        head = next;
    }
    return total;
}

int main(int argc, char **argv) {
    int rounds = 400;
    for (int index = 1; index < argc; index += 1) {
        if (strcmp(argv[index], "--quick") == 0) {
            rounds = 20;
        }
    }

    double start = now_seconds();
    long long total = 0;
    for (int round = 0; round < rounds; round += 1) {
        total += round_trip(20000);
    }
    double elapsed = now_seconds() - start;

    /* The same row shape that `bench_report` prints, so one table holds both. */
    printf("malloc\toverhead\t%.3f\t%d\t%d\t%d\t%lld\n", elapsed * 1000.0,
           rounds * 20000, 0, 0, total);
    return 0;
}
