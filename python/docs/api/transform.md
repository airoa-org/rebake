# rebake.transform

Output format transformers for exporting data.

## LeRobot v2.1 Transformer

Transforms data to LeRobot v2.1 dataset format.

::: rebake.transform.LeRobotV21TransformerConfig
    options:
      members:
        - outdir
        - robot_model
        - video_config
        - build

::: rebake.transform.LeRobotV21Transformer
    options:
      members:
        - run
        - transform

## Video Encoder Configuration

Optional configuration for video encoding used by the transformer.

::: rebake.transform.VideoEncoderConfig
    options:
      members:
        - fps
        - gop
        - crf
        - scaling
        - codec_config

## Scaling Flag

Enumeration for video scaling options.

::: rebake.transform.ScalingFlag
