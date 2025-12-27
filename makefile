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


copy-snapshot:
	@set -e; \
	TIMESTAMP=$$(date +%Y%m%d_%H%M%S); \
	OUT_DIR=$(SS_DIR); \
	\
	echo "==> Copying OMS snapshots from oms-server ..."; \
	if docker ps --format '{{.Names}}' | grep -q '^oms-server$$'; then \
		docker cp oms-server:/app/snapshot/. $$OUT_DIR/; \
	else \
		echo "OMS container oms-server not running"; \
	fi; \
	\
	for pair in BTCUSDT ETHUSDT; do \
		container=me_$$pair; \
		echo "==> Copying ME snapshots from $$container ..."; \
		if docker ps --format '{{.Names}}' | grep -q "^$$container$$"; then \
			docker cp $$container:/app/snapshot/. $$OUT_DIR/; \
		else \
			echo "Container $$container not running"; \
		fi; \
	done; \
	\
	echo "==> Done. Files copied to $$OUT_DIR"

test:
	@echo "Cleaning up previous test data..." && rm -rf $(SS_DIR) && mkdir -p $(SS_DIR)
	@echo "Waiting for services to start..." 
	@chmod +x ./bin/mvp_client
	@docker-compose -f $(DOCKER_COMPOSE_FILE) down  && sleep 2 && docker-compose -f $(DOCKER_COMPOSE_FILE) up -d && sleep 3
	@echo "Running integration tests" && ./bin/mvp_client ./tests/integration/testcase_massive.case 
# 	@echo "Running integration tests" && go test -timeout 300s -v -count=1 -run TestMassiveOrders  ./tests/integration/oms/... &> ./tmp/test.log
	@echo "Wait kafka to be consumed..." && sleep 10
	@echo "Dump snapshot" && ./bin/mvp_client ./tests/integration/testcase_snapshot.case && sleep 2 && $(MAKE) copy-snapshot
	@echo "Checking snapshot consistency..." && python3 tests/data/snapshot_check.py --dir=$(SS_DIR)
	@echo "Checking oms-redis consistency..." && go test -v -count=1 ./tests/integration/tex/...

check-redis:
	@echo "Cleaning up previous test data..." && rm -rf $(SS_DIR) && mkdir -p $(SS_DIR) && touch ./tmp/test2.log && touch ./tmp/test.log
	@docker-compose -f $(DOCKER_COMPOSE_FILE) down  && sleep 2 && docker-compose -f $(DOCKER_COMPOSE_FILE) up -d && sleep 3
	@echo "Check snapshot consistency..." && python3 ./tests/data/snapshot_check.py --dir=$(SS_DIR)
	@echo "Check snapshot and redis..." && go test -timeout 30s -v -count=1 -run TestOrderInRedis  ./tests/integration/oms/... > ./tmp/test2.log



# ==============================================================================
# 构建
# ==============================================================================
build-server:
	@docker build -t $(IMAGE_NAME):$(IMAGE_TAG) -f $(DOCKERFILE_SERVER) .
	@echo "✅ Server 镜像构建完成: $(IMAGE_NAME):$(IMAGE_TAG)"


build-img: build-server
	@echo "✅ 所有开发镜像构建完成。"

.PHONY: test integration-test cov bench build-server build-me build-img copy-snapshot