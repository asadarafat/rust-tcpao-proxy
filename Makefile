CARGO ?= cargo
CONFIG ?= config/example.toml

.PHONY: tools doctor fmt lint test test-functional test-functional-strict test-validation-tcpao-proxy test-validation-tcpao-proxy-bgp-route dry-run run-initiator run-terminator

tools:
	@set -e; \
	if [ -f "$$HOME/proxy" ]; then \
		. "$$HOME/proxy"; \
	fi; \
	if command -v sudo_dnf >/dev/null 2>&1; then \
		sudo_dnf install -y rust cargo rustfmt clippy; \
	elif command -v dnf >/dev/null 2>&1; then \
		sudo -E dnf install -y rust cargo rustfmt clippy; \
	else \
		echo "dnf not found; install rust/cargo/rustfmt/clippy manually for this distro"; \
		exit 1; \
	fi
	@rustc --version
	@cargo --version

doctor:
	./scripts/doctor.sh

fmt:
	$(CARGO) fmt --all

lint:
	$(CARGO) clippy --all-targets --all-features -- -D warnings

test:
	$(CARGO) test --all-targets

test-functional:
	env -u TCPAO_PROXY_TEST_NO_AO $(CARGO) test --test functional_tcpao -- --nocapture

test-functional-strict:
	env -u TCPAO_PROXY_TEST_NO_AO TCPAO_PROXY_TEST_REQUIRE_AO=1 $(CARGO) test --test functional_tcpao -- --nocapture

test-validation-tcpao-proxy:
	./scripts/test-validation-tcpao-proxy.sh

test-validation-tcpao-proxy-bgp-route:
	./scripts/test-validation-tcpao-proxy-bgp-route.sh

dry-run:
	$(CARGO) run -- --mode initiator --config $(CONFIG) --dry-run

run-initiator:
	$(CARGO) run -- --mode initiator --config $(CONFIG)

run-terminator:
	$(CARGO) run -- --mode terminator --config $(CONFIG)
