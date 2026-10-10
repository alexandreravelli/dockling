PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo

.PHONY: build install install-config uninstall deb clean

build:
	$(CARGO) build --release

# Installs the binary and starts the dock automatically at login (all users).
install:
	install -Dm755 target/release/dockling $(DESTDIR)$(PREFIX)/bin/dockling
	install -Dm644 data/dockling.desktop $(DESTDIR)/etc/xdg/autostart/dockling.desktop

# Optional: system-wide template copied to ~/.config/dockling/config.toml for new users.
install-config:
	install -Dm644 config/default.toml $(DESTDIR)/etc/xdg/dockling/config.toml

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/dockling $(DESTDIR)/etc/xdg/autostart/dockling.desktop

# Builds target/debian/dockling_<version>_<arch>.deb (needs: cargo install cargo-deb)
deb:
	$(CARGO) deb

clean:
	$(CARGO) clean
