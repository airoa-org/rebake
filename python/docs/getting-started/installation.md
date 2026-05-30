# Installation

rebake requires FFmpeg with SVT-AV1 support for video encoding. We recommend using the provided Docker environment.

## Using Docker (Recommended)

### 1. Build and start the container

```bash
cd rebake-rs/docker
docker compose up -d --build
```

### 2. Enter the container and install rebake

```bash
docker compose exec rebake-dev bash
cd python
uv sync
```

## Optional Dependencies

### For running tests

```bash
uv sync --extra test
```

### For building documentation

```bash
uv sync --extra docs
```
