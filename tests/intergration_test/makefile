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
copy-snapshot:
	@mkdir -p "$(SS_DIR)/$(TIMESTAMP)/server" && mkdir -p "$(SS_DIR)/$(TIMESTAMP)/me"
	@echo "==> Copying Server snapshots from $(OMS_CONTAINER) ..."
	-@docker cp $(OMS_CONTAINER):"$(CONTAINER_SS_PATH)/" "$(SS_DIR)/$(TIMESTAMP)/server/" || echo "No Server snapshots found in $(OMS_CONTAINER)."
	@echo "==> Copying ME snapshots from $(ME_CONTAINER) ..."
	-@docker cp $(ME_CONTAINER):"$(CONTAINER_SS_PATH)/" "$(SS_DIR)/$(TIMESTAMP)/me/" || echo "No ME snapshots found in $(ME_CONTAINER)." 	
	@echo "==> Done. Files copied to $(SS_DIR)/$(TIMESTAMP)"

inter-test:
	@echo "Waiting for services to start..." 
	@docker-compose up -d --abort-on-container-exit; sleep 5
	@echo "Running integration tests"
	@../../target/debug/mvp_client ./place_order.case
	@$(MAKE) copy-snapshot
	@docker-compose stop

.PHONY: inter-test copy-snapshot