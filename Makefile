PREFIX = $(DESTDIR)/usr/local
BINDIR = $(PREFIX)/bin
INSTALL_PROGRAM ?= install

all: target/release/cntr

target/release/cntr:
	cargo build --release --bin cntr

install: all
	$(INSTALL_PROGRAM) -D target/release/cntr $(BINDIR)/cntr

test:
	cargo test
