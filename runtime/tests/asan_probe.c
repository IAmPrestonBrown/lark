/* A probe that reports whether AddressSanitizer runs in this environment.
 *
 * A sandbox can stop the sanitizer from mapping its shadow memory, and the
 * process then hangs before `main`. The Makefile runs this first, with a time
 * limit, so a real test never hangs. */

int main(void) {
    return 0;
}
