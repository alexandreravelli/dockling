PREFIX ?= /usr/local
DESTDIR ?=
CARGO ?= cargo

.PHONY: build install install-config uninstall deb clean

build:
	$(CARGO) build --release

# Installs the dock (started automatically at login for all users) and, when it was built,
# the settings window with its menu entry.
install:
	install -Dm755 target/release/dockling $(DESTDIR)$(PREFIX)/bin/dockling
	install -Dm644 data/dockling.desktop $(DESTDIR)/etc/xdg/autostart/dockling.desktop
	install -Dm644 docs/logo.svg $(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/dockling.svg
	if [ -f target/release/dockling-settings ]; then \
		install -Dm755 target/release/dockling-settings $(DESTDIR)$(PREFIX)/bin/dockling-settings; \
		install -Dm644 data/dockling-settings.desktop \
			$(DESTDIR)$(PREFIX)/share/applications/dockling-settings.desktop; \
	fi

# Optional: system-wide template copied to ~/.config/dockling/config.toml for new users.
install-config:
	install -Dm644 config/default.toml $(DESTDIR)/etc/xdg/dockling/config.toml

uninstall:
	rm -f $(DESTDIR)$(PREFIX)/bin/dockling $(DESTDIR)$(PREFIX)/bin/dockling-settings \
		$(DESTDIR)/etc/xdg/autostart/dockling.desktop \
		$(DESTDIR)$(PREFIX)/share/applications/dockling-settings.desktop \
		$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/dockling.svg

# Builds target/debian/dockling_<version>_<arch>.deb (needs: cargo install cargo-deb)
deb:
	$(CARGO) deb

clean:
	$(CARGO) clean
