CLEANUP_MINUTES ?= 15
CLEANUP_LABEL_KEY ?= pulse-gate-test
CLEANUP_LABEL_VALUE ?= true
CLEANUP_NAME_PREFIX ?= pulse-gate-test-

# Define the target: build (will download the dependencies and compile the project)
build:
	cargo build

# Define the target: build-prod
build-prod:
	cargo build --release

# Define the target: clean
clean:
	cargo clean

# Define the target: update
update:
	cargo update

# Define the target: test
test:
	@bash -lc 'cargo test -- --nocapture; TEST_STATUS=$$?; ./scripts/container-cleanup.sh --label $(CLEANUP_LABEL_KEY) $(CLEANUP_LABEL_VALUE) --name-prefix $(CLEANUP_NAME_PREFIX) -y $(CLEANUP_MINUTES) || true; exit $$TEST_STATUS'

# Define the target: check
check:
	cargo check

# Define the target: lint
lint:
	cargo clippy

# Define the target: lint-fix
lint-fix:
	cargo clippy --fix

# Define the target: run
run:
	cargo run

# Define the target: run-release (will compile while eliminating the debug statements)
run-release:
	cargo run --release

# Define the target: cloc
cloc:
	cloc --exclude-dir=target  .