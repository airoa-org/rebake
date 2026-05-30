# rebake.enrich

Data enrichment modules for adding computed fields.

## TF Buffer Enricher

Builds a TF buffer from `/tf` and `/tf_static` topics.

::: rebake.enrich.TfBufferEnricherConfig
    options:
      members:
        - build

::: rebake.enrich.TfBufferEnricher
    options:
      members:
        - run

## TF Chain Enricher

Computes transform chains between frames.

### Frame Pair

::: rebake.enrich.FramePair
    options:
      members:
        - source
        - target

### Configuration and Enricher

::: rebake.enrich.TfChainEnricherConfig
    options:
      members:
        - build

::: rebake.enrich.TfChainEnricher
    options:
      members:
        - run

## Delta Joint Position Enricher

Computes delta (change) in joint positions between frames.

::: rebake.enrich.DeltaJointPositionEnricherConfig
    options:
      members:
        - topic_names
        - build

::: rebake.enrich.DeltaJointPositionEnricher
    options:
      members:
        - run

## Delta Transform Enricher

Computes delta transforms between frames. `DeltaTransformEnricherConfig` requires `delta_reference_frame`; use `"previous_target_frame"` for body-frame action deltas.

::: rebake.enrich.DeltaTransformEnricherConfig
    options:
      members:
        - build

::: rebake.enrich.DeltaTransformEnricher
    options:
      members:
        - run

## Hand Command Enricher

Extracts hand command data.

::: rebake.enrich.HandCommandEnricherConfig
    options:
      members:
        - build

::: rebake.enrich.HandCommandEnricher
    options:
      members:
        - run

## Head Command Enricher

Extracts head command data.

::: rebake.enrich.HeadCommandEnricherConfig
    options:
      members:
        - build

::: rebake.enrich.HeadCommandEnricher
    options:
      members:
        - run
