/* A header that the programmer wrote, with the same stem as the module.
 *
 * Rule X-4b keeps the generated header out of this name. */
#ifndef OWN_HEADER_H
#define OWN_HEADER_H

#define LABEL_LIMIT 32

typedef struct Label {
    const char *text;
    int length;
} Label;

int label_length(const Label *item);

#endif
