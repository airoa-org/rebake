# Hardware-accelerated encoding

[日本語版](hardware_ja.md)

rebake encodes video on the CPU by default (SVT-AV1 / x264 / x265) and requires no GPU.
The codecs below are optional accelerators; enable them only if you have the matching
hardware.

## VA-API hardware encoding (AMD / Intel)

For GPU-accelerated video encoding (AMD/Intel), pass the VA-API override file:

```bash
cd docker
docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
docker compose exec rebake-dev bash
```

Prerequisites:
- `/dev/dri/renderD128` must exist on the host (`ls /dev/dri/renderD*`)

The Docker image includes VA-API drivers for AMD and Intel. By default, `LIBVA_DRIVER_NAME` is left unset so libva can auto-select the matching driver for the mounted `/dev/dri` device. If auto-detection fails, pass a driver name explicitly when starting the container.

Choose the driver by the GPU/iGPU that provides `/dev/dri`, not by the CPU vendor alone:

- `radeonsi`: AMD GPU/iGPU
- `iHD`: modern Intel GPU/iGPU
- `i965`: older Intel GPU/iGPU, if `iHD` is not supported

```bash
LIBVA_DRIVER_NAME=radeonsi docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
LIBVA_DRIVER_NAME=iHD docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
```

## NVIDIA NVENC hardware encoding

GPU-accelerated video encoding on NVIDIA requires:

- NVIDIA driver compatible with NVENC. H.264 and H.265 NVENC are supported on most NVIDIA GPUs from Maxwell onwards (GTX 900-series and later); AV1 NVENC additionally requires Ada Lovelace or newer (RTX 4000-series and later)
- Docker with the [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html) installed
- NVIDIA CDI devices generated on the host:

```bash
sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml
nvidia-ctk cdi list
```

If your host has the `nvidia-cdi-refresh` systemd units (check with `systemctl list-unit-files | grep nvidia-cdi-refresh`), enable the path unit so the CDI spec is refreshed after driver/toolkit changes:

```bash
sudo systemctl enable --now nvidia-cdi-refresh.path
```

Once those are in place, bring up the dev container with the NVENC compose override. `Dockerfile.nvenc` layers on top of the base `rebake:latest` image, so the base image must be built first:

```bash
cd docker
docker compose -f docker-compose.yml build rebake-dev
docker compose -f docker-compose.yml -f docker-compose.nvenc.yml up -d --build
docker compose exec rebake-dev bash
```

If the dev container cannot reach the GPU, run these checks on the host to confirm CDI is wired up and the freshly built image can see the device:

```bash
nvidia-ctk cdi list
docker run --rm --device nvidia.com/gpu=all rebake:nvenc nvidia-smi
```
