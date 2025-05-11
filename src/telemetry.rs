use opentelemetry::{global, trace::TracerProvider as _, KeyValue};
use opentelemetry_sdk::{
    metrics::{MeterProviderBuilder, PeriodicReader, SdkMeterProvider},
    trace::{RandomIdGenerator, Sampler, SdkTracerProvider},
    Resource,
};
use opentelemetry_semantic_conventions::{
    attribute::{DEPLOYMENT_ENVIRONMENT_NAME, SERVICE_VERSION},
    SCHEMA_URL,
};
use serde::{Deserialize, Serialize};
use std::env;
use tracing_opentelemetry::{MetricsLayer, OpenTelemetryLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Telemetry protocol to use for OTLP export
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum TelemetryProtocol {
    #[serde(rename = "http/protobuf")]
    HTTP,
    #[serde(rename = "grpc")]
    GRPC,
}

impl TelemetryProtocol {
    /// Determines telemetry protocol from environment variables if OTEL is configured
    pub fn from_env() -> Option<Self> {
        let is_enabled = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
            || env::var("OTEL_EXPORTER_OTLP_HEADERS").is_ok()
            || env::var("OTEL_EXPORTER_OTLP_PROTOCOL").is_ok();
        if is_enabled {
            let protocol = match env::var("OTEL_EXPORTER_OTLP_PROTOCOL") {
                Ok(string) => match string.as_str() {
                    "http/protobuf" | "http" => TelemetryProtocol::HTTP,
                    "grpc" => TelemetryProtocol::GRPC,
                    _ => TelemetryProtocol::HTTP,
                },
                Err(_) => TelemetryProtocol::HTTP,
            };
            Some(protocol)
        } else {
            None
        }
    }
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

/// Generates a semantic OpenTelemetry `Resource` describing this service
fn resource() -> Resource {
    let deployment_env = env::var("DEPLOYMENT_ENV").unwrap_or_else(|_| "develop".to_string());
    Resource::builder()
        .with_service_name(env!("CARGO_PKG_NAME"))
        .with_schema_url(
            [
                KeyValue::new(SERVICE_VERSION, env!("CARGO_PKG_VERSION")),
                KeyValue::new(DEPLOYMENT_ENVIRONMENT_NAME, deployment_env),
            ],
            SCHEMA_URL,
        )
        .build()
}

/// Initializes the OpenTelemetry metrics provider
fn init_meter_provider(telemetry_protocol: &TelemetryProtocol) -> SdkMeterProvider {
    let exporter = opentelemetry_otlp::MetricExporter::builder();

    // Configure exporter based on selected protocol
    let exporter = match telemetry_protocol {
        TelemetryProtocol::HTTP => exporter
            .with_http()
            .with_temporality(opentelemetry_sdk::metrics::Temporality::default())
            .build(),
        TelemetryProtocol::GRPC => exporter
            .with_tonic()
            .with_temporality(opentelemetry_sdk::metrics::Temporality::default())
            .build(),
    };
    let exporter = exporter.expect("Failed to build OTLP metric exporter");

    // Set up periodic push-based metric reader
    let reader = PeriodicReader::builder(exporter)
        .with_interval(std::time::Duration::from_secs(30))
        .build();

    // Add stdout exporter for local development inspection
    let stdout_reader =
        PeriodicReader::builder(opentelemetry_stdout::MetricExporter::default()).build();

    // Assemble and register the meter provider globally
    let meter_provider = MeterProviderBuilder::default()
        .with_resource(resource())
        .with_reader(reader)
        .with_reader(stdout_reader)
        .build();

    global::set_meter_provider(meter_provider.clone());

    meter_provider
}

/// Initializes the OpenTelemetry tracer provider
fn init_tracer_provider(telemetry_protocol: &TelemetryProtocol) -> SdkTracerProvider {
    let exporter = opentelemetry_otlp::SpanExporter::builder();
    // Choose transport protocol
    let exporter = match telemetry_protocol {
        TelemetryProtocol::HTTP => exporter.with_http().build(),
        TelemetryProtocol::GRPC => exporter.with_tonic().build(),
    };
    let exporter = exporter.expect("Failed to build OTLP span exporter");

    // Construct and return a tracer provider
    SdkTracerProvider::builder()
        // Customize sampling strategy
        .with_sampler(Sampler::ParentBased(Box::new(Sampler::TraceIdRatioBased(
            1.0,
        ))))
        // If export trace to AWS X-Ray, you can use XrayIdGenerator
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(resource())
        .with_batch_exporter(exporter)
        .build()
}


/// Wrapper for telemetry providers, for graceful shutdown
pub struct Telemetry {
    pub tracer_provider: Option<SdkTracerProvider>,
    pub meter_provider: Option<SdkMeterProvider>,
}

impl Telemetry {
    /// Initializes telemetry from environment variables if enabled
    pub fn new() -> Self {
        let telemetry_protocol = TelemetryProtocol::from_env();
        match telemetry_protocol {
            Some(telemetry_protocol) => {
                let tracer_provider = init_tracer_provider(&telemetry_protocol);
                let meter_provider = init_meter_provider(&telemetry_protocol);
                let tracer = tracer_provider.tracer("tracing-otel-subscriber");

                // Register tracing subscriber with OpenTelemetry layers
                tracing_subscriber::registry()
                    // The global level filter prevents the exporter network stack
                    // from reentering the globally installed OpenTelemetryLayer with
                    // its own spans while exporting, as the libraries should not use
                    // tracing levels below DEBUG. If the OpenTelemetry layer needs to
                    // trace spans and events with higher verbosity levels, consider using
                    // per-layer filtering to target the telemetry layer specifically,
                    // e.g. by target matching.
                    .with(tracing_subscriber::filter::LevelFilter::INFO)
                    .with(tracing_subscriber::fmt::layer())
                    .with(MetricsLayer::new(meter_provider.clone()))
                    .with(OpenTelemetryLayer::new(tracer))
                    .init();

                tracing::info!(
                    "OpenTelemetry tracing and metrics exporter is enabled via {:?}",
                    telemetry_protocol
                );
                Self {
                    tracer_provider: Some(tracer_provider),
                    meter_provider: Some(meter_provider),
                }
            }
            None => {
                // Fallback: just use local logging
                tracing_subscriber::registry()
                    .with(tracing_subscriber::fmt::layer())
                    .init();

                tracing::info!("OpenTelemetry is not enabled");

                Self {
                    tracer_provider: None,
                    meter_provider: None,
                }
            }
        }
    }
}

/// Graceful shutdown for Telemetry.
impl Drop for Telemetry {
    fn drop(&mut self) {
        if let Some(tracer_provider) = self.tracer_provider.as_ref() {
            if let Err(err) = tracer_provider.shutdown() {
                eprintln!("{err:?}");
            }
        }
        if let Some(meter_provider) = self.meter_provider.as_ref() {
            if let Err(err) = meter_provider.shutdown() {
                eprintln!("{err:?}");
            }
        }
    }
}
