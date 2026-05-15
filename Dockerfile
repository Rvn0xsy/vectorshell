# syntax=docker/dockerfile:1.7

FROM golang:1.25-bookworm AS builder
WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY . ./

RUN --mount=type=cache,target=/root/.cache/go-build \
    --mount=type=cache,target=/go/pkg/mod \
    CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -o /out/vectorshell-server ./cmd/server

FROM debian:bookworm-slim AS runtime
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /out/vectorshell-server /app/vectorshell-server
COPY config.example.toml /app/config.toml
COPY data /app/data
COPY build /app/build

RUN mkdir -p /app/data /app/build/clients

EXPOSE 8080

ENTRYPOINT ["/app/vectorshell-server"]
CMD ["-config", "/app/config.toml"]
