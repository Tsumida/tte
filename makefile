# ==============================================================================
# 🐳 Docker 镜像配置
# ==============================================================================

# Server (原 mvp_server, 现 crates/server)
IMAGE_NAME := mvp_server
IMAGE_TAG := latest
DOCKERFILE_SERVER := Dockerfile.server


# ==============================================================================
# 💾 快照配置
# ==============================================================================
SS_DIR := ./snapshot
# 注意: 容器名称应该与你实际运行的 docker-compose 或 run 命令中的名称保持一致
OMS_CONTAINER := mvp_server
ME_CONTAINER := mvp_me
CONTAINER_SS_PATH := /app/snapshot
TIMESTAMP := $(shell date +%Y%m%d_%H%M%S)

# ==============================================================================
# 🧪 测试与基准测试
# ==============================================================================
ut:
	@cargo test --workspace --lib -- --nocapture

integration-test:
	@cargo test --package server --test integration_test -- --exact --nocapture 

# 代码覆盖率: 针对整个工作空间运行 tarpaulin，忽略 proto 生成的代码
cov:
	# --workspace 用于扫描所有 crates
	# --exclude-files 可以用更宽泛的模式，比如 **/pbcode/*.rs
	cargo tarpaulin --workspace --ignore-tests --exclude-files '**/pbcode/*' --out Html

# 基准测试: 
bench:
	@cargo build --workspace --release
	@cargo bench --package ledger -- --nocapture 

# ==============================================================================
# 构建
# ==============================================================================
build-server:
	@docker build -t $(IMAGE_NAME):$(IMAGE_TAG) -f $(DOCKERFILE_SERVER) .
	@echo "✅ Server 镜像构建完成: $(IMAGE_NAME):$(IMAGE_TAG)"


build-img: build-server
	@echo "✅ 所有开发镜像构建完成。"

# ==============================================================================
# 其他
# ==============================================================================
copy-snapshot:
	@mkdir -p "$(SS_DIR)/$(TIMESTAMP)/server" && mkdir -p "$(SS_DIR)/$(TIMESTAMP)/me"
	@echo "==> Copying Server snapshots from $(OMS_CONTAINER) ..."
	-@docker cp $(OMS_CONTAINER):"$(CONTAINER_SS_PATH)/" "$(SS_DIR)/$(TIMESTAMP)/server/" || echo "No Server snapshots found in $(OMS_CONTAINER)."
	@echo "==> Copying ME snapshots from $(ME_CONTAINER) ..."
	-@docker cp $(ME_CONTAINER):"$(CONTAINER_SS_PATH)/" "$(SS_DIR)/$(TIMESTAMP)/me/" || echo "No ME snapshots found in $(ME_CONTAINER)." 	
	@echo "==> Done. Files copied to $(SS_DIR)/$(TIMESTAMP)"

.PHONY: test integration-test cov bench build-server build-me build-img copy-snapshot