# The Lark runtime.
#
# Rule C-3.3 gives the warning set. Rule C-3.4 runs every test under the
# sanitizers.
#
# AddressSanitizer needs to map a large shadow region, and a sandbox can stop
# it. The `check-asan` target probes first, with a time limit. Set
# LARK_REQUIRE_ASAN=1 to fail rather than skip. Continuous integration sets it.

CC ?= cc
BUILD ?= build
LIMIT ?= 120
RUN = tools/run-with-limit.sh

# The collector to link. Chapter 10 section 4 names each one, and `gc.strategy`
# in `lark.toml` selects it for a build. Every collector satisfies the same
# seam, so the same test suite runs against each.
GC ?= precise-marksweep
GC_SOURCE_precise-marksweep = gc-marksweep/lark_marksweep.c
GC_SOURCE_arena = gc-arena/lark_arena.c
GC_SOURCE_semispace = gc-semispace/lark_semispace.c
GC_SOURCE_generational = gc-generational/lark_generational.c
GC_SOURCE = $(GC_SOURCE_$(GC))
GC_ALL = precise-marksweep arena semispace generational

WARNINGS = -Wall -Wextra -Wpedantic -Wconversion -Wshadow -Werror
CFLAGS = -std=c11 $(WARNINGS) -Iinclude -Icore -pthread -g
UBSAN = -fsanitize=undefined -fno-sanitize-recover=undefined
ASAN = -fsanitize=address -fno-omit-frame-pointer

SOURCES = core/lark_core.c $(GC_SOURCE)
TESTS = tests/lark_test.c tests/main.c tests/test_alloc.c tests/test_blockmap.c \
        tests/test_barrier.c tests/test_collect.c tests/test_contract.c tests/test_graphs.c \
        tests/test_growth.c tests/test_moving.c tests/test_roots.c tests/test_stress.c \
        tests/test_threads.c tests/test_types.c
HEADERS = include/lark_rt.h core/lark_internal.h tests/lark_test.h tests/test_types.h

.PHONY: all check check-all check-plain check-ubsan check-asan clean

all: $(BUILD)/liblark_rt.a

$(BUILD):
	mkdir -p $(BUILD)

$(BUILD)/liblark_rt.a: $(SOURCES) $(HEADERS) | $(BUILD)
	@test -n "$(GC_SOURCE)" || { echo "unknown collector: $(GC). Pick one of: $(GC_ALL)"; exit 1; }
	$(CC) $(CFLAGS) -c core/lark_core.c -o $(BUILD)/lark_core.o
	$(CC) $(CFLAGS) -c $(GC_SOURCE) -o $(BUILD)/lark_gc.o
	ar rcs $@ $(BUILD)/lark_core.o $(BUILD)/lark_gc.o

check: check-plain check-ubsan check-asan

# Runs the whole suite against every collector. Each one links its own binary,
# because a program links exactly one collector.
check-all:
	@for gc in $(GC_ALL); do \
	    echo "== collector: $$gc =="; \
	    $(MAKE) --no-print-directory GC=$$gc BUILD=$(BUILD)/$$gc check || exit 1; \
	done

check-plain: $(SOURCES) $(TESTS) $(HEADERS) | $(BUILD)
	@test -n "$(GC_SOURCE)" || { echo "unknown collector: $(GC). Pick one of: $(GC_ALL)"; exit 1; }
	@echo "== runtime tests, $(GC) =="
	$(CC) $(CFLAGS) -Itests -o $(BUILD)/tests $(SOURCES) $(TESTS)
	$(RUN) $(LIMIT) $(BUILD)/tests

check-ubsan: $(SOURCES) $(TESTS) $(HEADERS) | $(BUILD)
	@test -n "$(GC_SOURCE)" || { echo "unknown collector: $(GC). Pick one of: $(GC_ALL)"; exit 1; }
	@echo "== runtime tests, $(GC), UndefinedBehaviorSanitizer =="
	$(CC) $(CFLAGS) $(UBSAN) -Itests -o $(BUILD)/tests-ubsan $(SOURCES) $(TESTS)
	$(RUN) $(LIMIT) $(BUILD)/tests-ubsan

check-asan: $(SOURCES) $(TESTS) $(HEADERS) | $(BUILD)
	@echo "== runtime tests, $(GC), AddressSanitizer =="
	@$(CC) $(CFLAGS) $(ASAN) -o $(BUILD)/asan-probe tests/asan_probe.c 2>/dev/null; \
	if $(RUN) 10 $(BUILD)/asan-probe >/dev/null 2>&1; then \
	    $(CC) $(CFLAGS) $(ASAN) -Itests -o $(BUILD)/tests-asan $(SOURCES) $(TESTS) && \
	    $(RUN) $(LIMIT) $(BUILD)/tests-asan; \
	elif [ "$(LARK_REQUIRE_ASAN)" = "1" ]; then \
	    echo "FAIL  AddressSanitizer does not run here, and LARK_REQUIRE_ASAN is set."; \
	    exit 1; \
	else \
	    echo "SKIP  AddressSanitizer does not run in this environment."; \
	    echo "      A sandbox can stop it from mapping its shadow memory."; \
	    echo "      Continuous integration runs it with LARK_REQUIRE_ASAN=1."; \
	fi

clean:
	rm -rf $(BUILD)
