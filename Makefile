APP_NAME := vectorshell-server
CLIENT_NAME := vectorshell-client
IMAGE_NAME ?= vectorshell
IMAGE_TAG ?= latest
CONFIG ?= ./config.toml
GO ?= go
DOCKER ?= docker
DASHBOARD_DIR ?= ./dashboard

.PHONY: help build build-server build-client run up run-server run-client run-repl fmt tidy test clean web-install web-dev web-build web-preview web-lint docker-build docker-run

help:
	@echo "VectorShell Makefile"
	@echo ""
	@echo "Usage:"
	@echo "  make build          Build server and client binaries"
	@echo "  make build-server   Build the server binary"
	@echo "  make build-client   Build the client binary"
	@echo "  make run-server     Run the server with the selected config"
	@echo "  make run-client     Run the client with the selected config"
	@echo "  make run-repl       Run the local REPL"
	@echo "  make test           Run Go tests"
	@echo "  make web-build      Build dashboard for production"
	@echo "  make docker-build   Build the Docker image"

build: build-server build-client

build-server:
	$(GO) build -o ./build/$(APP_NAME) ./cmd/server

build-client:
	$(GO) build -o ./build/$(CLIENT_NAME) ./cmd/client

up: run-server

run: run-server

run-server:
	$(GO) run ./cmd/server -config $(CONFIG)

run-client:
	$(GO) run ./cmd/client -config $(CONFIG)

run-repl:
	$(GO) run ./cmd/repl -config $(CONFIG)

fmt:
	$(GO) fmt ./...

tidy:
	$(GO) mod tidy

test:
	$(GO) test ./...

clean:
	rm -rf ./build

web-install:
	npm install --prefix $(DASHBOARD_DIR)

web-dev:
	npm run --prefix $(DASHBOARD_DIR) dev

web-build: web-install
	npm run --prefix $(DASHBOARD_DIR) build

web-preview:
	npm run --prefix $(DASHBOARD_DIR) preview

web-lint:
	npm run --prefix $(DASHBOARD_DIR) lint

docker-build:
	$(DOCKER) build -t $(IMAGE_NAME):$(IMAGE_TAG) .

docker-run:
	$(DOCKER) run --rm -it -p 8080:8080 -v $(PWD)/data:/app/data $(IMAGE_NAME):$(IMAGE_TAG)
