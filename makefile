# 定义镜像名称和标签
IMAGE_NAME := mvp_server
IMAGE_TAG := latest
DOCKERFILE := Dockerfile.server

test:
	@cargo test --lib -- --nocapture

intergrtation-test:
	@cargo test --package tte --test integration_test -- --exact --nocapture 

cov:
	# ignore /src/pbcode
	cargo tarpaulin --ignore-tests --exclude-files src/pbcode/* --out Html

bench:
	cargo build --release && cargo bench

build-dev:
# 	@rustup target add x86_64-unknown-linux-musl
# 	@cargo build --release --bin mvp_server --target x86_64-unknown-linux-musl
	@docker build -t $(IMAGE_NAME):$(IMAGE_TAG) -f $(DOCKERFILE) .
	@echo "✅ 镜像构建完成: $(IMAGE_NAME):$(IMAGE_TAG)"