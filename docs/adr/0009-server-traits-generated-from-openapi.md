# Server traits generated from the OpenAPI contract

**Superseded by [ADR 0014](0014-hand-written-http-layer-checked-against-the-contract.md)** after plan M3.1 found that the generated router cannot serve the spec's `:action` paths.

HTTP handlers implement server traits and models generated from the vendored `openapi.yaml` by openapi-generator's `rust-axum` generator, mirroring Java's generated `*Api` interfaces. A missing endpoint or a wrong shape therefore fails to compile. A black-box test also validates real responses against the spec.

## Considered options

Hand-written handlers with `serde` types and response validation alone is the fallback if the generator's output proves unusable for part of the spec. Switching to it for any part of the spec needs a superseding ADR.
