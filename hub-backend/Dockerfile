# Build context: hub-backend/ (docker build -t hub-backend ./hub-backend)
#
# linux/arm64 (ARMv8 / AArch64): base images are multi-arch. Build on an ARM64
# machine → native arm64 image. From x86_64 → ARM64:
#   docker buildx build --platform linux/arm64 -t hub-backend ./hub-backend
# (CGO+SQLite uses QEMU under emulation; slower than native.)
FROM golang:1.25-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc libc6-dev libsqlite3-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src

COPY go.mod go.sum ./
RUN go mod download

COPY . .

RUN CGO_ENABLED=1 GOOS=linux go build -trimpath -ldflags="-s -w" -o /out/server ./cmd/server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /out/server /usr/local/bin/server

WORKDIR /data

ENV SHARED_PORT=:18080 \
    GRPC_URL=127.0.0.1:50051

EXPOSE 18080

ENTRYPOINT ["/usr/local/bin/server"]
