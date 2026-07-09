# Off-target unit tests for the portable MacAtrium core (host gcc, no Toolbox).
# artcaps.c compiles here with -DARTCAPS_HOST_TEST, which drops its Toolbox probe
# and keeps only the pure capability-gating arithmetic (docs/44).
CC      ?= cc
CFLAGS  ?= -Wall -Wextra -std=c99 -I../src -DARTCAPS_HOST_TEST

host_test: host_test.c ../src/json.c ../src/catalog.c ../src/model.c ../src/artcaps.c
	$(CC) $(CFLAGS) $^ -o $@

.PHONY: test clean
test: host_test
	./host_test

clean:
	rm -f host_test
