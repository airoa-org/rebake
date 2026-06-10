# Hardware Acceleration

rebake encodes on CPU by default (SVT-AV1 / x264 / x265), so no GPU is required. This page covers container setup only when you choose VA-API codecs (AMD / Intel) or NVENC codecs (NVIDIA). For choosing codecs, see [encoding](encoding.md#choosing-a-codec).

## VA-API (AMD / Intel)

First confirm the host has a render device (`ls /dev/dri/renderD*`), then start the VA-API compose overlay.

```bash
cd docker
docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
docker compose exec rebake-dev bash
```

The image includes AMD and Intel VA-API drivers, and libva normally chooses from the `/dev/dri` device. If auto-detection fails, set the driver for the GPU or iGPU that provides `/dev/dri`, not necessarily the CPU vendor.

- `radeonsi`: AMD GPU / iGPU
- `iHD`: newer Intel GPU / iGPU
- `i965`: older Intel GPU / iGPU when `iHD` does not work

```bash
LIBVA_DRIVER_NAME=radeonsi docker compose -f docker-compose.yml -f docker-compose.vaapi.yml up -d --build
```

## NVENC (NVIDIA)

You need three things: an NVIDIA driver with NVENC support (H.264 / H.265 on GTX 900 series or newer, AV1 on RTX 4000 series or newer), the [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html), and host-generated CDI device definitions.

```bash
sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml
nvidia-ctk cdi list
```

If your host has the `nvidia-cdi-refresh` systemd unit, enable it so definitions follow driver updates. Check with `systemctl list-unit-files | grep nvidia-cdi-refresh`.

```bash
sudo systemctl enable --now nvidia-cdi-refresh.path
```

The NVENC image layers on top of the base image, so build the base first, then start the NVENC overlay.

```bash
cd docker
docker compose -f docker-compose.yml build rebake-dev
docker compose -f docker-compose.yml -f docker-compose.nvenc.yml up -d --build
docker compose exec rebake-dev bash
```

If the container cannot see the GPU, check both of these on the host:

```bash
nvidia-ctk cdi list
docker run --rm --device nvidia.com/gpu=all rebake:nvenc nvidia-smi
```
