PREFIX = $(DESTDIR)/usr/local
BINDIR = $(PREFIX)/bin
INSTALL_PROGRAM ?= install
CARGO ?= cargo

TARGET = target/release/cntr

all: $(TARGET)

$(TARGET):
	$(CARGO) build --release

install: all
	$(INSTALL_PROGRAM) -D $(TARGET) $(BINDIR)/cntr

.PHONY: clean
clean:
	rm -rf $(TARGET)
