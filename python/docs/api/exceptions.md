# rebake.exceptions

Exception classes for error handling.

## Exception Hierarchy

All rebake exceptions inherit from `RebakeError`:

```
RebakeError (base)
├── IngestError      - ROS bag reading errors
├── SynchronizeError - Time synchronization errors
├── EnrichError      - Data enrichment errors
├── EncodeError      - Video encoding errors
├── DecodeError      - Video decoding errors
└── TransformError   - Output transformation errors
```

## Base Exception

::: rebake.exceptions.RebakeError

## Stage-Specific Exceptions

::: rebake.exceptions.IngestError

::: rebake.exceptions.SynchronizeError

::: rebake.exceptions.EnrichError

::: rebake.exceptions.EncodeError

::: rebake.exceptions.DecodeError

::: rebake.exceptions.TransformError
