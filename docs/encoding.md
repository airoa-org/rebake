# Encoding

These settings decide what camera and depth pixels become as videos. The default, software AV1, is practical for both quality and size, so you can skip this page until you have a reason to change it.

There are two entry points, and both drive the same Encoder code: `video_config` / `depth_config` blocks in pipeline settings (see [configuration: stage reference](configuration.md#stage-reference)), and `rebake-cli export` flags (see [CLI](cli.md#export)). `export` uses throughput-oriented defaults; those differences are listed under [export defaults](#export-defaults).

## RGB video

### Choosing a codec

Choose by trading off file size and decode speed.

| Goal | `codec` | Quality knob, typical range |
|---|---|---|
| Smallest files, slower decode | `AV1` (default) | `crf` 25 to 35 |
| Faster decode, useful when training reads often | `H264` | `crf` 18 to 28 |
| Middle ground | `H265` | `crf` 22 to 32 |
| Fast encode on AMD / Intel GPU | `H264_VAAPI` `H265_VAAPI` `AV1_VAAPI` | `qp` |
| Fast encode on NVIDIA GPU | `H264_NVENC` `H265_NVENC` `AV1_NVENC` | `qp` |

There are two quality knobs. Software codecs use top-level `crf`; hardware codecs use `qp` inside `codec_config`. Lower means higher quality and larger files. The hardware `qp` defaults were measured on project recordings to meet a visually near-source target (VMAF 93 or higher), so start with the defaults.

`crf` is one shared field, and the default `"34"` is tuned for AV1. When switching to `H264` or `H265`, set `crf` in the range shown above.

### Common fields

The minimal setting is frame rate plus codec.

```yaml
video_config:
  fps: 30
  codec_config:
    codec: AV1
```

- `fps` (integer, default `100`): video frame rate. Set it to the synchronizer `fps`. A mismatch is not an error, but rows and video frames will not line up.
- `gop` (integer, default `20`): keyframe interval. Smaller seeks faster and makes larger files.
- `crf` (string, default `"34"`): software codec quality. Hardware codecs ignore it.
- `scaling` (default `Bicubic`): interpolation used by `resize`. `Bilinear` / `FastBilinear` are fast; `Lanczos` is sharp.
- `resize` (optional, `{width, height}`): exact output size. Both values must be positive even numbers. Aspect ratio is not preserved automatically. Omit to keep source size.

### Codec-specific settings

Choose a codec with `codec_config.codec`, then write that codec's fields in the same block. For hyphen versus underscore spelling, see [key spelling](#key-spelling).

`AV1` (software, SVT-AV1):

| Key | Range | Default | Meaning |
|---|---|---|---|
| `preset` | 0 to 13 | 10 | Encoding effort. Lower is slower and smaller at the same `crf` |
| `lp` | 0 to 6 | automatic | Parallelism, roughly thread count |
| `lookahead` | -1 to 120 | unset | Lookahead frames. Higher can improve quality and uses more memory |
| `film-grain` | 0 to 50 | unset | Grain synthesis. Around 8 can shrink live-action footage |
| `film-grain-denoise` | true/false | unset | Denoise input before grain synthesis |
| `fast-decode` | 0 to 2 | unset | Make decoding lighter |
| `pin` | 0 to N | unset | Pin encoder threads to the first N cores; 0 disables |

`H264` (x264) and `H265` (x265):

| Key | Default | Meaning |
|---|---|---|
| `preset` | `medium` | `ultrafast` `superfast` `veryfast` `faster` `fast` `medium` `slow` `slower` `veryslow`. Faster means larger files |
| `tune` | none | A list of content tunes. Use at most one perceptual tune (H264: `film` `animation` `grain` `stillimage` `psnr` `ssim`; H265: `psnr` `ssim` `grain` `animation`). `fastdecode` / `zerolatency` can combine |
| `threads` | automatic | Thread count |
| `frame-threads` (H265 only) | automatic | Frame-level parallelism |

VA-API (AMD / Intel). Common keys are `qp` (quality), `device` (default `/dev/dri/renderD128`), `profile`, `b-depth` (0 to 7), and `async-depth` (1 to 64):

| `codec` | `qp` range and default | Notes |
|---|---|---|
| `H264_VAAPI` | 0 to 51, default 21 | `profile` default `high`, `async-depth` default 16 |
| `H265_VAAPI` | 0 to 51, default 29 | No B frames, due to AMD hardware constraints |
| `AV1_VAAPI` | 0 to 255, default 110 | Quality is passed as `-global_quality` |

NVENC (NVIDIA). Common keys are `qp`, `gpu` (GPU index), `preset` (`P1` fastest to `P7` best compression), `tune` (`Hq` / `Ll` / `Ull`), `profile`, `b_frames` (0 to 7), and `rc_lookahead` (0 to 120):

| `codec` | `qp` range and default | `preset` | `b_frames` |
|---|---|---|---|
| `H264_NVENC` | 0 to 51, default 26 | `P5` | 1 |
| `H265_NVENC` | 0 to 51, default 25 | `P4` | 0 |
| `AV1_NVENC` | 0 to 255, default 130 | `P7` | 0 |

Only `H264_NVENC` defaults `tune` to `Hq`, `profile` to `high`, and `rc_lookahead` to 32. The other two leave those unset. If `b_frames` is greater than 0, set `gop` greater than `b_frames + 1`.

## Depth video

Depth is a metric millimeter value, so ordinary visual compression can make it unusable. rebake configures depth separately with `depth_config` and supports two modes.

- `FFV1` (lossless): preserves 16-bit distance values exactly. Files are larger, and the container is `.mkv`.
- Everything else (10-bit quantized): maps `[1, depth_max_mm]` to 10 bits. The step is about `depth_max_mm / 1023`, so the default 4092 mm gives about 4 mm steps. Zero and out-of-range values become invalid. Files are smaller and use `.mp4`.

```yaml
depth_config:
  fps: 30
  codec_config:
    codec: FFV1
```

- `depth_max_mm` (integer, default `4092`): maximum distance kept by quantization. Lower it for more precision at close range; raise it if you need farther depths. Ignored by `FFV1`.
- `fps` (integer, default `30`)
- `codec_config`: choose from the table below. Depth has no `gop`.

| `codec` | Quality knob, range | Default |
|---|---|---|
| `FFV1` | none, lossless | |
| `AV1` (software) | `crf` (0 to 63) | 4 (`preset` is 4) |
| `H265_VAAPI` | `qp` (0 to 51) | 18 |
| `AV1_VAAPI` | `global_quality` (0 to 255) | 35 |
| `H265_NVENC` | `qp` (0 to 51) | 10 |
| `AV1_NVENC` | `qp` (0 to 255) | 20 |

rebake automatically adds the Encoder settings needed to keep quantized values intact, such as full-range handling where required. For how depth is stored inside the video file, see [intermediate format](intermediate-format.md#videos).

## export defaults

When `rebake-cli export` is used without `--video-config`, it overrides some values per codec for throughput. Fields not shown here use the YAML defaults.

| `--codec` | crf | gop | Other |
|---|---|---|---|
| `av1` | 34 | 20 | |
| `h264` | 15 | 2 | `preset` `fast` |
| `h265` | 18 | 100 | `preset` `superfast`, `frame-threads` 6 |
| `av1_vaapi` | | 100 | quality 124, different from YAML default 110 |
| `h264_vaapi` | | 20 | |
| `h265_vaapi` | | 100 | |
| `h264_nvenc` | | 20 | |
| `h265_nvenc` | | 100 | |
| `av1_nvenc` | | 20 | |

`--qp` overrides quality for `h264_vaapi` and the three NVENC RGB codecs; `av1_vaapi` and `h265_vaapi` use fixed defaults. For depth, `--depth-qp` overrides NVENC depth `qp`. To change anything else, pass one of the examples in `config/export/` through `--video-config`.

## Key spelling

Video and codec configs reject unknown keys, so spelling mistakes become errors.

| Target | Spelling | Alternate spelling |
|---|---|---|
| AV1 / VA-API / x265 compound keys (`film-grain` `fast-decode` `frame-threads` `b-depth` `async-depth`) | hyphen | none; underscore is rejected |
| NVENC compound keys (`b_frames` `rc_lookahead`) | underscore | hyphen also works |
| Software `preset` / `tune` values | lowercase (`medium` `film`) | initial uppercase also works |
| NVENC `preset` / `tune` values | `P1` to `P7`, `Hq` / `Ll` / `Ull` | lowercase also works |
| `scaling` values | initial uppercase (`Bicubic`) | none |

Value ranges are checked when the stage runs, not when YAML is loaded.

## Hardware requirements

Hardware codecs run through the environment's `ffmpeg` command, so you need an ffmpeg build with the corresponding Encoder and a GPU device visible from the container.

- VA-API: H.264 / H.265 on AMD RDNA 2 or newer, or Intel Gen 8 or newer. AV1 on AMD RDNA 3 or newer, or Intel Arc.
- NVENC: H.264 / H.265 on Maxwell (GTX 900 series) or newer. AV1 on Ada Lovelace (RTX 4000 series) or newer.

Container setup is in [hardware acceleration](hardware.md).

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| Video and table rows are misaligned, with no error | `video_config.fps` differs from the synchronizer `fps` | Set them to the same value |
| A config key is rejected | Spelling, hyphen, or underscore mismatch | Check the [key spelling](#key-spelling) table |
| A stage fails during execution | Value is out of range | Keep values within the ranges in the tables |
| AMD AV1 shows black padding at the edge | Known RDNA 3 (VCN 4) width-padding issue | Resize width to a multiple of 64, or change codec |

Hardware visibility errors, such as missing `/dev/dri`, are covered in [hardware acceleration](hardware.md).
