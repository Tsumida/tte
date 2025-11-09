test:
	cargo test

cov:
	# ignore /src/pbcode
	cargo tarpaulin --ignore-tests --exclude-files src/pbcode/* --out Html

bench:
	cargo build --release && cargo bench