# tokio_bridge shared library Makefile
#
# Builds libtokio_bridge.so - shared library for Rust <-> PHP communication
# This library must be loaded by both Rust binary and PHP extension
# to share TLS (Thread-Local Storage) between them.

CC ?= gcc
AR ?= ar

# Compiler flags
CFLAGS = -fPIC -O2 -Wall -Wextra -std=c11

# Debug build (make DEBUG=1)
ifdef DEBUG
CFLAGS += -g -DDEBUG
endif

# Output files
SHARED_LIB = libtokio_bridge.so
STATIC_LIB = libtokio_bridge.a
OBJECTS = bridge.o

# Install paths
PREFIX ?= /usr/local
LIBDIR ?= $(PREFIX)/lib
INCLUDEDIR ?= $(PREFIX)/include

.PHONY: all clean install uninstall

all: $(SHARED_LIB) $(STATIC_LIB)

# Shared library (main target - used at runtime)
$(SHARED_LIB): $(OBJECTS)
	$(CC) -shared -o $@ $(OBJECTS)

# Static library (for linking into test binaries)
$(STATIC_LIB): $(OBJECTS)
	$(AR) rcs $@ $(OBJECTS)

# Object file
bridge.o: bridge.c bridge.h
	$(CC) $(CFLAGS) -c -o $@ bridge.c

clean:
	rm -f $(OBJECTS) $(SHARED_LIB) $(STATIC_LIB)

install: $(SHARED_LIB)
	install -d $(LIBDIR)
	install -d $(INCLUDEDIR)
	install -m 755 $(SHARED_LIB) $(LIBDIR)/
	install -m 644 bridge.h $(INCLUDEDIR)/tokio_bridge.h
	ldconfig 2>/dev/null || true

uninstall:
	rm -f $(LIBDIR)/$(SHARED_LIB)
	rm -f $(INCLUDEDIR)/tokio_bridge.h
	ldconfig 2>/dev/null || true

# Print variables for debugging
print-%:
	@echo '$*=$($*)'
