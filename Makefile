# Stellar Guilds — developer tasks
CARGO ?= cargo
CONTRACT_DIR := contracts/guild-vault
WASM := target/wasm32v1-none/release/guild_vault.wasm
MAX_WASM_KB := 60

.PHONY: all build test fmt fmt-check lint check wasm size clean

all: check

## Compile the contract for the Soroban VM (wasm32v1-none).
build:
	stellar contract build

## Run the unit test suite.
test:
	$(CARGO) test

## Format the workspace.
fmt:
	$(CARGO) fmt

## Verify formatting without rewriting files.
fmt-check:
	$(CARGO) fmt --check

## Lint with warnings denied.
lint:
	$(CARGO) clippy --all-targets -- -D warnings

## Everything CI runs.
check: fmt-check lint test

## Fail if the contract exceeds the size budget.
size: build
	@size=$$(wc -c < $(WASM)); \
	echo "guild_vault.wasm: $$size bytes"; \
	if [ $$size -gt $$(( $(MAX_WASM_KB) * 1024 )) ]; then \
		echo "ERROR: wasm exceeds $(MAX_WASM_KB) KiB budget" >&2; exit 1; \
	fi

clean:
	$(CARGO) clean
