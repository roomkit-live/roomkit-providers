//! The provider's options default as Python's keywords.

use std::time::Duration;

use roomkit_providers_opentelemetry::OpenTelemetryProviderOptions;

#[test]
fn open_telemetry_provider_options_defaults_as_python() {
    let defaults = OpenTelemetryProviderOptions::default();
    // roomkit.telemetry.OpenTelemetryProvider.__init__(tracer_provider) = None
    assert!(defaults.tracer_provider.is_none());
    // roomkit.telemetry.OpenTelemetryProvider.__init__(meter_provider) = None
    assert!(defaults.meter_provider.is_none());
    // roomkit.telemetry.OpenTelemetryProvider.__init__(service_name) = 'roomkit'
    assert_eq!(defaults.service_name, "roomkit");
    // roomkit.telemetry.OpenTelemetryProvider.__init__(metadata) = None
    assert!(defaults.metadata.is_empty());
    // roomkit.telemetry.OpenTelemetryProvider.__init__(shutdown_flush_timeout) = 4.0
    assert_eq!(defaults.shutdown_flush_timeout, Duration::from_secs(4));
}
