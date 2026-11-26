# roomkit-providers-opentelemetry

RoomKit's telemetry on OpenTelemetry.

RoomKit's core reports the spans and metrics of a room's work to the
provider its kit is given. This crate's `OpenTelemetryProvider` hands them to
an OpenTelemetry SDK tracer and meter provider, whose exporter the
application chooses: a message's inbound, its broadcast and the agent's turn
under it show as one trace, a turn that runs in another process included.

| Name | What it is |
| --- | --- |
| `OpenTelemetryProvider` | The provider, as RoomKit's Python `roomkit.telemetry.OpenTelemetryProvider` |
| `OpenTelemetryProviderOptions` | Its options: the tracer and meter providers, the service name, the tags of every span, the close's bound |

## Use

```rust,no_run
use std::sync::Arc;

use opentelemetry_sdk::trace::SdkTracerProvider;
use roomkit::{RoomKit, RoomKitOptions};
use roomkit_providers_opentelemetry::{OpenTelemetryProvider, OpenTelemetryProviderOptions};

let telemetry = OpenTelemetryProvider::new(OpenTelemetryProviderOptions {
    tracer_provider: Some(SdkTracerProvider::builder().build()),
    ..OpenTelemetryProviderOptions::default()
});
let kit = RoomKit::new(RoomKitOptions {
    telemetry: Some(Arc::new(telemetry).into()),
    ..Default::default()
});
```

The provider starts each span under the current span's context, which the
core gives by `roomkit::telemetry::get_current_telemetry_ctx`: a provider of
another backend is written on the same public names.

## Example

`examples/one_trace.rs` runs a room's message through the kit and prints the
spans it exported to memory, one trace:

```sh
cargo run -p roomkit-providers-opentelemetry --example one_trace
```
