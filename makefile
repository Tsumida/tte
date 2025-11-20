test:
	@cargo test --lib -- --nocapture

intergrtation-test:
	@cargo test --package tte --test integration_test -- --exact --nocapture 

cov:
	# ignore /src/pbcode
	cargo tarpaulin --ignore-tests --exclude-files src/pbcode/* --out Html

bench:
	cargo build --release && cargo bench

build-debug:
	@rustup target add x86_64-unknown-linux-musl
	@cargo build --release --bin mvp_server --target x86_64-unknown-linux-musl
	@docker build -t mvp_server:latest -f ./Dockerfile.server .