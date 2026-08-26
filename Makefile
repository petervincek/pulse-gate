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
	cargo test

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