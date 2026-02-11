CARGO ?= cargo
CONFIG ?= config/example.toml

.PHONY: doctor fmt lint test test-functional dry-run run-initiator run-terminator

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
