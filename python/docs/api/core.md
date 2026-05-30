# rebake.core

Core data structures for the rebake pipeline.

## Context

The `Context` class is the central data container that flows through the pipeline.

::: rebake.core.Context
    options:
      members:
        - __init__
        - from_tables
        - rosbag_path
        - set_rosbag_path
        - output_dir
        - set_output_dir
        - fps
        - set_fps
        - dataset_topics
        - get_record_batch
        - set_record_batch
        - to_record_batches
        - get_image_data
        - set_image_data
