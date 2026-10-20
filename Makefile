# ⚡ Blitz — build, run, and install into OS app locations
# Prefer cargo over Homebrew `dx` (often Deno's dx, not dioxus-cli).

CARGO  ?= cargo
RUSTUP ?= rustup

APP_NAME    := Blitz
BIN_NAME    := blitz
BUNDLE_ID   := com.blitz.app
VERSION     := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
ASSETS_DIR  := assets

# Optional staging / prefix (Linux + Windows paths; macOS uses APPDIR)
DESTDIR ?=
PREFIX  ?= $(HOME)/.local

# --- OS detection (uname; Windows via OS=Windows_NT or MINGW/MSYS) ---
ifeq ($(OS),Windows_NT)
  UNAME_S := Windows_NT
else
  UNAME_S := $(shell uname -s 2>/dev/null || echo Unknown)
endif

RELEASE_BIN := target/release/$(BIN_NAME)
ifeq ($(UNAME_S),Windows_NT)
  RELEASE_BIN := target/release/$(BIN_NAME).exe
endif
ifneq (,$(filter MINGW% MSYS% CYGWIN%,$(UNAME_S)))
  RELEASE_BIN := target/release/$(BIN_NAME).exe
  UNAME_S := Windows_NT
endif

# macOS: user-writable by default; system via `make install-system`
MACOS_USER_APPDIR    := $(HOME)/Applications
MACOS_SYSTEM_APPDIR  := /Applications
MACOS_APPDIR         ?= $(MACOS_USER_APPDIR)
MACOS_BUNDLE         := $(DESTDIR)$(MACOS_APPDIR)/$(APP_NAME).app

# Linux under PREFIX (default ~/.local)
LINUX_SHARE   := $(DESTDIR)$(PREFIX)/share/blitz
LINUX_BIN     := $(DESTDIR)$(PREFIX)/bin
LINUX_DESKTOP := $(DESTDIR)$(PREFIX)/share/applications
LINUX_ICON    := $(DESTDIR)$(PREFIX)/share/icons/hicolor/256x256/apps

# Windows (Git Bash / MSYS): %LOCALAPPDATA%\Blitz
ifeq ($(UNAME_S),Windows_NT)
  WIN_LOCALAPPDATA := $(shell powershell.exe -NoProfile -Command '$$env:LOCALAPPDATA' 2>/dev/null | tr -d '\r')
  ifeq ($(WIN_LOCALAPPDATA),)
    WIN_LOCALAPPDATA := $(LOCALAPPDATA)
  endif
  WIN_PREFIX ?= $(WIN_LOCALAPPDATA)/$(APP_NAME)
endif

.DEFAULT_GOAL := help

.PHONY: help deps fetch install uninstall install-system dmg \
	install-macos install-linux install-windows \
	uninstall-macos uninstall-linux uninstall-windows \
	run run-release release build build-release \
	test check clean fmt clippy

help: ## List available targets
	@printf '%s\n' \
		'Targets:' \
		'  help            Show this help (default)' \
		'  deps / fetch    rustup check + cargo fetch (no app install)' \
		'  install         Release build + install desktop app (user dirs)' \
		'  dmg             macOS only: build release and generate portable Blitz.dmg' \
		'  uninstall       Remove installed app' \
		'  install-system  macOS only: install to /Applications (needs sudo)' \
		'  run             Dev build + run (cargo run)' \
		'  run-release     Release build + run' \
		'  release         Alias for run-release' \
		'  build           Debug build' \
		'  build-release   Release build' \
		'  test / check / clean / fmt / clippy' \
		'' \
		'Install paths (no sudo by default):' \
		'  macOS:   ~/Applications/Blitz.app' \
		'           (system: sudo make install-system → /Applications)' \
		'  Linux:   ~/.local/share/blitz/ + ~/.local/bin/blitz' \
		'           + ~/.local/share/applications/blitz.desktop' \
		'  Windows: %LOCALAPPDATA%\\Blitz\\ (+ Start Menu via install.ps1)' \
		'' \
		'Overrides: PREFIX=$(PREFIX)  DESTDIR=  MACOS_APPDIR=$(MACOS_APPDIR)' \
		'' \
		'Desktop deps (Dioxus WebView):' \
		'  macOS:   system WebKit (Xcode CLT usually enough)' \
		'  Linux:   webkit2gtk, gtk3, libxdo-dev, appindicator (see README / CI workflow)' \
		'  Windows: WebView2 runtime'

# --- cargo-only (old `make install` behavior) ---

deps fetch: ## rustup check + fetch crate deps
	@command -v $(RUSTUP) >/dev/null 2>&1 || { \
		echo 'error: rustup not found — install from https://rustup.rs'; >&2; \
		exit 1; \
	}
	@command -v $(CARGO) >/dev/null 2>&1 || { \
		echo 'error: cargo not found — ensure rustup default toolchain installed' >&2; \
		exit 1; \
	}
	$(RUSTUP) show
	$(CARGO) fetch
	@echo ''
	@echo 'Deps fetched. Optional: rustup component add rustfmt clippy'
	@echo 'Dioxus desktop needs a system WebView (see make help).'
	@echo 'Install the app with: make install'

# --- app install / uninstall ---

install: build-release ## Build release + install into OS app location
ifeq ($(UNAME_S),Darwin)
	@$(MAKE) install-macos
else ifeq ($(UNAME_S),Linux)
	@$(MAKE) install-linux
else ifeq ($(UNAME_S),Windows_NT)
	@$(MAKE) install-windows
else
	@echo "error: unsupported OS '$(UNAME_S)' — use Darwin, Linux, or Windows (Git Bash/MSYS)" >&2
	@exit 1
endif

uninstall: ## Remove installed app
ifeq ($(UNAME_S),Darwin)
	@$(MAKE) uninstall-macos
else ifeq ($(UNAME_S),Linux)
	@$(MAKE) uninstall-linux
else ifeq ($(UNAME_S),Windows_NT)
	@$(MAKE) uninstall-windows
else
	@echo "error: unsupported OS '$(UNAME_S)'" >&2
	@exit 1
endif

install-system: build-release ## macOS: install to /Applications (sudo)
ifeq ($(UNAME_S),Darwin)
	@$(MAKE) install-macos MACOS_APPDIR=$(MACOS_SYSTEM_APPDIR)
else
	@echo 'error: install-system is macOS-only (use make install on Linux/Windows)' >&2
	@exit 1
endif

# --- macOS .app bundle ---

install-macos: ## Package Blitz.app into MACOS_APPDIR
	@test -f "$(RELEASE_BIN)" || { echo 'error: missing $(RELEASE_BIN)' >&2; exit 1; }
	@mkdir -p "$(MACOS_APPDIR)"
	@echo "Installing $(MACOS_BUNDLE) ..."
	@rm -rf "$(MACOS_BUNDLE)"
	@mkdir -p "$(MACOS_BUNDLE)/Contents/MacOS"
	@mkdir -p "$(MACOS_BUNDLE)/Contents/Resources/assets"
	@cp "$(RELEASE_BIN)" "$(MACOS_BUNDLE)/Contents/MacOS/$(APP_NAME)"
	@chmod +x "$(MACOS_BUNDLE)/Contents/MacOS/$(APP_NAME)"
	@cp -R "$(ASSETS_DIR)/." "$(MACOS_BUNDLE)/Contents/Resources/assets/"
	@if [ -f packaging/macos/AppIcon.icns ]; then \
		cp packaging/macos/AppIcon.icns "$(MACOS_BUNDLE)/Contents/Resources/AppIcon.icns"; \
	fi
	@sed \
		-e 's|<string>0\.1\.0</string>|<string>$(VERSION)</string>|g' \
		packaging/macos/Info.plist > "$(MACOS_BUNDLE)/Contents/Info.plist"
	@echo ''
	@echo "Installed: $(MACOS_BUNDLE)"
	@echo "  Open:    open \"$(MACOS_BUNDLE)\""
	@echo "  Remove:  make uninstall"
	@echo "  System:  sudo make install-system  → $(MACOS_SYSTEM_APPDIR)/$(APP_NAME).app"
	@echo "  Note:    CSS/logo are baked into the binary — always reinstall after rebuild."

dmg: build-release ## macOS only: build release and generate Blitz.dmg installer
ifeq ($(UNAME_S),Darwin)
	@test -f "$(RELEASE_BIN)" || { echo 'error: missing $(RELEASE_BIN)' >&2; exit 1; }
	@echo "Creating macOS DMG installer..."
	@rm -rf target/dmg-stage target/$(APP_NAME).dmg
	@mkdir -p target/dmg-stage/$(APP_NAME).app/Contents/MacOS
	@mkdir -p target/dmg-stage/$(APP_NAME).app/Contents/Resources/assets
	@cp "$(RELEASE_BIN)" target/dmg-stage/$(APP_NAME).app/Contents/MacOS/$(APP_NAME)
	@chmod +x target/dmg-stage/$(APP_NAME).app/Contents/MacOS/$(APP_NAME)
	@cp -R "$(ASSETS_DIR)/." target/dmg-stage/$(APP_NAME).app/Contents/Resources/assets/
	@if [ -f packaging/macos/AppIcon.icns ]; then \
		cp packaging/macos/AppIcon.icns target/dmg-stage/$(APP_NAME).app/Contents/Resources/AppIcon.icns; \
	fi
	@sed \
		-e 's|<string>0\.1\.0</string>|<string>$(VERSION)</string>|g' \
		packaging/macos/Info.plist > target/dmg-stage/$(APP_NAME).app/Contents/Info.plist
	@ln -s /Applications target/dmg-stage/Applications
	@hdiutil create -volname "$(APP_NAME)" -srcfolder target/dmg-stage -ov -format UDZO target/$(APP_NAME).dmg
	@rm -rf target/dmg-stage
	@echo ''
	@echo "Generated DMG: target/$(APP_NAME).dmg"
	@ls -lh target/$(APP_NAME).dmg
else
	@echo "error: dmg target is macOS-only" >&2
	@exit 1
endif

uninstall-macos:
	@if [ -d "$(MACOS_USER_APPDIR)/$(APP_NAME).app" ]; then \
		rm -rf "$(MACOS_USER_APPDIR)/$(APP_NAME).app"; \
		echo "Removed $(MACOS_USER_APPDIR)/$(APP_NAME).app"; \
	fi
	@if [ -d "$(MACOS_SYSTEM_APPDIR)/$(APP_NAME).app" ]; then \
		if [ -w "$(MACOS_SYSTEM_APPDIR)/$(APP_NAME).app" ] || [ -w "$(MACOS_SYSTEM_APPDIR)" ]; then \
			rm -rf "$(MACOS_SYSTEM_APPDIR)/$(APP_NAME).app"; \
			echo "Removed $(MACOS_SYSTEM_APPDIR)/$(APP_NAME).app"; \
		else \
			echo "System install present — run: sudo rm -rf $(MACOS_SYSTEM_APPDIR)/$(APP_NAME).app"; \
		fi; \
	fi
	@if [ ! -d "$(MACOS_USER_APPDIR)/$(APP_NAME).app" ] && [ ! -d "$(MACOS_SYSTEM_APPDIR)/$(APP_NAME).app" ]; then \
		echo "No $(APP_NAME).app found under ~/Applications or /Applications"; \
	fi

# --- Linux ---

install-linux: ## Install binary, assets, desktop entry under PREFIX
	@test -f "$(RELEASE_BIN)" || { echo 'error: missing $(RELEASE_BIN)' >&2; exit 1; }
	@echo "Installing under $(DESTDIR)$(PREFIX) ..."
	@mkdir -p "$(LINUX_SHARE)/assets" "$(LINUX_BIN)" "$(LINUX_DESKTOP)" "$(LINUX_ICON)"
	@cp "$(RELEASE_BIN)" "$(LINUX_SHARE)/$(BIN_NAME)"
	@chmod +x "$(LINUX_SHARE)/$(BIN_NAME)"
	@cp -R "$(ASSETS_DIR)/." "$(LINUX_SHARE)/assets/"
	@ln -sfn "../share/blitz/$(BIN_NAME)" "$(LINUX_BIN)/$(BIN_NAME)"
	@# also short name for PATH convenience
	@ln -sfn "../share/blitz/$(BIN_NAME)" "$(LINUX_BIN)/blitz"
	@if [ -f "$(ASSETS_DIR)/logo.png" ]; then \
		cp "$(ASSETS_DIR)/logo.png" "$(LINUX_ICON)/blitz.png"; \
	fi
	@EXEC="$(PREFIX)/bin/blitz"; \
	ICON="$(PREFIX)/share/icons/hicolor/256x256/apps/blitz.png"; \
	sed -e "s|@EXEC@|$$EXEC|g" -e "s|@ICON@|$$ICON|g" \
		packaging/linux/blitz.desktop.in > "$(LINUX_DESKTOP)/blitz.desktop"
	@chmod 644 "$(LINUX_DESKTOP)/blitz.desktop"
	@command -v update-desktop-database >/dev/null 2>&1 && \
		update-desktop-database "$(LINUX_DESKTOP)" 2>/dev/null || true
	@echo ''
	@echo "Installed:"
	@echo "  App dir:  $(LINUX_SHARE)"
	@echo "  Binary:   $(LINUX_BIN)/blitz"
	@echo "  Desktop:  $(LINUX_DESKTOP)/blitz.desktop"
	@echo "  Remove:   make uninstall"
	@echo "  Ensure $(PREFIX)/bin is on PATH"

uninstall-linux:
	@rm -f "$(LINUX_BIN)/$(BIN_NAME)" "$(LINUX_BIN)/blitz"
	@rm -f "$(LINUX_DESKTOP)/blitz.desktop"
	@rm -f "$(LINUX_ICON)/blitz.png"
	@rm -rf "$(LINUX_SHARE)"
	@command -v update-desktop-database >/dev/null 2>&1 && \
		update-desktop-database "$(LINUX_DESKTOP)" 2>/dev/null || true
	@echo "Removed Blitz from $(DESTDIR)$(PREFIX)"

# --- Windows (Git Bash / MSYS → PowerShell script) ---

install-windows: ## Copy to %LOCALAPPDATA%\Blitz + Start Menu
	@echo "Installing via scripts/install.ps1 ..."
	@REPO_WIN=$$(cd "$(CURDIR)" && (pwd -W 2>/dev/null || cygpath -w "$(CURDIR)" 2>/dev/null || echo "$(CURDIR)")); \
	if [ -n "$(WIN_PREFIX)" ]; then \
		powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/install.ps1 \
			-RepoRoot "$$REPO_WIN" -Prefix "$(WIN_PREFIX)"; \
	else \
		powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/install.ps1 \
			-RepoRoot "$$REPO_WIN"; \
	fi

uninstall-windows:
	@if [ -n "$(WIN_PREFIX)" ]; then \
		powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/install.ps1 \
			-Uninstall -Prefix "$(WIN_PREFIX)"; \
	else \
		powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/install.ps1 -Uninstall; \
	fi

# --- run / build ---

run: ## Dev: cargo run
	$(CARGO) run

run-release: ## Release: cargo run --release
	$(CARGO) run --release

release: run-release ## Alias → run-release

build: ## Debug: cargo build
	$(CARGO) build

build-release: ## Release: cargo build --release
	$(CARGO) build --release

test: ## cargo test
	$(CARGO) test

check: ## cargo check
	$(CARGO) check

clean: ## cargo clean
	$(CARGO) clean

fmt: ## cargo fmt (requires rustfmt)
	@$(CARGO) fmt --version >/dev/null 2>&1 || { \
		echo 'error: rustfmt missing — rustup component add rustfmt' >&2; \
		exit 1; \
	}
	$(CARGO) fmt

clippy: ## cargo clippy (requires clippy)
	@$(CARGO) clippy --version >/dev/null 2>&1 || { \
		echo 'error: clippy missing — rustup component add clippy' >&2; \
		exit 1; \
	}
	$(CARGO) clippy -- -D warnings
