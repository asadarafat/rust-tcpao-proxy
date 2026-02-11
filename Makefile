CARGO ?= cargo
CONFIG ?= config/example.toml

.PHONY: tools doctor fmt lint test test-functional dry-run run-initiator run-terminator

tools:
	@if ! command -v rustup >/dev/null 2>&1; then \
		echo "rustup not found; installing..."; \
		curl https://sh.rustup.rs -sSf | sh -s -- -y; \
	fi
	@. "$$HOME/.cargo/env" && \
		rustup toolchain install stable && \
		rustup component add rustfmt clippy && \
		rustc --version && \
		cargo --version

doctor:
	./scripts/doctor.sh

fmt:
	$(CARGO) fmt --all

lint:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

test:
	$(CARGO) test --all-targets

test-functional:
	$(CARGO) test --test functional_tcpao -- --nocapture

dry-run:
	$(CARGO) run -- --mode initiator --config $(CONFIG) --dry-run

run-initiator:
	$(CARGO) run -- --mode initiator --config $(CONFIG)

run-terminator:
	$(CARGO) run -- --mode terminator --config $(CONFIG)
