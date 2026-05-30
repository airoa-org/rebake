# rebake.ingest

Ingestors for reading ROS bag files.

## ROS 2 Bag Ingestor

For reading ROS 2 bag files in MCAP format.

::: rebake.ingest.Rosbag2IngestorConfig
    options:
      members:
        - build

::: rebake.ingest.Rosbag2Ingestor
    options:
      members:
        - run
        - ingest

## ROS 1 Bag Ingestor

For reading ROS 1 bag files.

::: rebake.ingest.Rosbag1IngestorConfig
    options:
      members:
        - build

::: rebake.ingest.Rosbag1Ingestor
    options:
      members:
        - run
        - ingest
