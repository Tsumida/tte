# 定义镜像名称和标签
IMAGE_NAME := mvp_server
IMAGE_TAG := latest
DOCKERFILE := Dockerfile.server

IMG_NAME_ME := mvp_me
IMG_TAG_ME := latest
DOCKERFILE_ME := Dockerfile.me

SS_DIR := ./snapshot
OMS_CONTAINER := mvp_server
ME_CONTAINER := mvp_me
CONTAINER_SS_PATH := /app/snapshot
TIMESTAMP := $(shell date +%Y%m%d_%H%M%S)

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
	@echo "✅ mvp_server镜像构建完成: $(IMAGE_NAME):$(IMAGE_TAG)"

copy-snapshot:
	@mkdir -p "$(SS_DIR)/$(TIMESTAMP)/oms" && mkdir -p "$(SS_DIR)/$(TIMESTAMP)/me"
	@echo "==> Copying OMS snapshots from $(OMS_CONTAINER) ..."
	-@docker cp $(OMS_CONTAINER):"$(CONTAINER_SS_PATH)/" "$(SS_DIR)/$(TIMESTAMP)/oms/" || echo "No OMS snapshots found in $(OMS_CONTAINER)."
	@echo "==> Copying ME snapshots from $(ME_CONTAINER) ..."
	-@docker cp $(ME_CONTAINER):"$(CONTAINER_SS_PATH)/" "$(SS_DIR)/$(TIMESTAMP)/me/" || echo "No ME snapshots found in $(ME_CONTAINER)."    
	@echo "==> Done. Files copied to $(SS_DIR)/$(TIMESTAMP)"