# Off-target unit tests for the portable MacAtrium core (host gcc, no Toolbox).
# artcaps.c compiles here with -DARTCAPS_HOST_TEST and toolbox.c with
# -DTOOLBOX_HOST_TEST, each dropping its Toolbox half and keeping the pure logic
# (art-capability gating, docs/44; CD entry parse + name match, docs/45).
CC      ?= cc
CFLAGS  ?= -Wall -Wextra -std=c99 -I../src -DARTCAPS_HOST_TEST -DTOOLBOX_HOST_TEST

host_test: host_test.c ../src/json.c ../src/catalog.c ../src/model.c ../src/artcaps.c ../src/toolbox.c
	$(CC) $(CFLAGS) $^ -o $@

.PHONY: test clean
test: host_test
	./host_test

clean:
	rm -f host_test
