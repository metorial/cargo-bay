.PHONY: help build test run dev docker-build docker-up docker-down docker-logs jwt-token clean

help: ## Show this help message
	@echo 'Usage: make [target]'
	@echo ''
	@echo 'Available targets:'
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-20s\033[0m %s\n", $$1, $$2}'

build: ## Build the project
	cargo build --release

test: ## Run tests
	cargo test

run: ## Run the service locally
	cargo run

dev: ## Run with auto-reload (requires cargo-watch)
	cargo watch -x run

docker-build: ## Build Docker image
	docker build -t docker-registry-proxy:latest .

docker-up: ## Start services with docker-compose
	docker-compose -f docker-compose.example.yml up -d

docker-up-prod: ## Start services with Traefik (production)
	docker-compose -f docker-compose.advanced.yml up -d

docker-down: ## Stop docker-compose services
	docker-compose -f docker-compose.example.yml down

docker-down-prod: ## Stop production services
	docker-compose -f docker-compose.advanced.yml down

docker-logs: ## View docker-compose logs
	docker-compose -f docker-compose.example.yml logs -f

docker-logs-prod: ## View production logs
	docker-compose -f docker-compose.advanced.yml logs -f

docker-restart: ## Restart docker-compose services
	docker-compose -f docker-compose.example.yml restart

jwt-token: ## Generate JWT token (usage: make jwt-token SECRET=mysecret USER=john REPOS=alpine,nginx)
	@if [ -z "$(SECRET)" ]; then echo "Error: SECRET is required. Usage: make jwt-token SECRET=mysecret USER=john"; exit 1; fi
	@if [ -z "$(USER)" ]; then echo "Error: USER is required. Usage: make jwt-token SECRET=mysecret USER=john"; exit 1; fi
	@if [ -n "$(REPOS)" ]; then \
		cargo run --example generate_jwt -- "$(SECRET)" "$(USER)" "$(REPOS)"; \
	else \
		cargo run --example generate_jwt -- "$(SECRET)" "$(USER)"; \
	fi

setup: ## Setup development environment
	@echo "Setting up development environment..."
	@if [ ! -f config.toml ]; then cp config.example.toml config.toml; echo "Created config.toml from example"; fi
	@if [ ! -f .env ]; then cp .env.example .env; echo "Created .env from example"; fi
	@echo "Setup complete! Edit config.toml and .env with your settings."

clean: ## Clean build artifacts
	cargo clean
	docker-compose -f docker-compose.example.yml down -v
	docker-compose -f docker-compose.advanced.yml down -v

check: ## Run cargo check
	cargo check

fmt: ## Format code
	cargo fmt

lint: ## Run clippy
	cargo clippy -- -D warnings

all: fmt lint test build ## Run format, lint, test, and build
