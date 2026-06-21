# Off-target unit tests for the portable MacAtrium core (host gcc, no Toolbox).
CC      ?= cc
CFLAGS  ?= -Wall -Wextra -std=c99 -I../src

host_test: host_test.c ../src/json.c ../src/catalog.c ../src/model.c
	$(CC) $(CFLAGS) $^ -o $@

.PHONY: test clean
test: host_test
	./host_test

clean:
	rm -f host_test
