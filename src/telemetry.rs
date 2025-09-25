use opentelemetry::{KeyValue, Value, global, trace::TracerProvider as _};
use opentelemetry_sdk::{
    Resource,
    metrics::{MeterProviderBuilder, PeriodicReader, SdkMeterProvider},
    trace::{RandomIdGenerator, Sampler, SdkTracerProvider},
};
use opentelemetry_semantic_conventions::{
    SCHEMA_URL,
    attribute::{DEPLOYMENT_ENVIRONMENT_NAME, SERVICE_VERSION},
};
use serde::{Deserialize, Serialize};
use std::env;
use tracing_opentelemetry::{MetricsLayer, OpenTelemetryLayer};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Supported telemetry transport protocols for exporting OTLP data.
///
/// The default is HTTP if not explicitly configured.
#[allow(clippy::upper_case_acronyms)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum TelemetryProtocol {
    /// Use `http/protobuf` protocol for OTLP export.
    #[serde(rename = "http/protobuf")]
    HTTP,
    /// Use `grpc` protocol for OTLP export.
    #[serde(rename = "grpc")]
    GRPC,
}

impl TelemetryProtocol {
    /// Detects the telemetry protocol based on OTEL-related environment variables.
    ///
    /// Returns `Some(TelemetryProtocol)` if telemetry is enabled, or `None` if
    /// no relevant environment variables are set.
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

/// Describes the local service's identity and metadata for telemetry purposes.
///
/// This includes:
/// - The service `name` and `version` (used in span and metric resources),
/// - The `deployment` environment (e.g., "dev", "staging", "prod").
///
/// These values can be provided manually via [`Telemetry::with_name`], [`Telemetry::with_version`], [`Telemetry::with_deployment`],
/// or overridden using environment variables:
/// - `OTEL_SERVICE_NAME`
/// - `OTEL_SERVICE_VERSION`
/// - `OTEL_SERVICE_DEPLOYMENT`
#[derive(Clone, Debug, Default)]
pub struct Telemetry {
    /// Optional service name (e.g., `"x402-facilitator"`).
    ///
    /// May be overridden by the `OTEL_SERVICE_NAME` environment variable.
    pub name: Option<Value>,
    /// Optional service version (e.g., `"0.3.0"`).
    ///
    /// May be overridden by the `OTEL_SERVICE_VERSION` environment variable.
    pub version: Option<Value>,
    /// Optional deployment environment (e.g., `"production"` or `"develop"`).
    ///
    /// May be overridden by the `OTEL_SERVICE_DEPLOYMENT` environment variable.
    pub deployment: Option<Value>,
}

impl Telemetry {
    /// Creates a new, empty [`Telemetry`] instance.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the service name.
    #[allow(dead_code)]
    pub fn with_name(&self, name: impl Into<Value>) -> Self {
        let mut this = self.clone();
        this.name = Some(name.into());
        this
    }

    /// Sets the service version.
    #[allow(dead_code)]
    pub fn with_version(&self, version: impl Into<Value>) -> Self {
        let mut this = self.clone();
        this.version = Some(version.into());
        this
    }

    /// Sets the deployment environment for this service (e.g., `"production"`, `"staging"`).
    ///
    /// This value is used for populating the `deployment.environment` semantic attribute.
    #[allow(dead_code)]
    pub fn with_deployment(&self, deployment: impl Into<Value>) -> Self {
        let mut this = self.clone();
        this.deployment = Some(deployment.into());
        this
    }

    /// Resolves the service name for telemetry.
    ///
    /// Order of precedence:
    /// 1. `OTEL_SERVICE_NAME` env variable (if non-empty),
    /// 2. Otherwise, fallback to locally set value in `self.name`.
    pub fn name(&self) -> Option<Value> {
        env::var("OTEL_SERVICE_NAME")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Value::from)
            .or_else(|| self.name.clone())
    }

    /// Resolves the service version for telemetry.
    ///
    /// Order of precedence:
    /// 1. `OTEL_SERVICE_VERSION` env variable (if non-empty),
    /// 2. Otherwise, fallback to locally set value in `self.version`.
    pub fn version(&self) -> Option<Value> {
        env::var("OTEL_SERVICE_VERSION")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Value::from)
            .or_else(|| self.version.clone())
    }

    /// Resolves the service deployment environment.
    ///
    /// Order of precedence:
    /// 1. `OTEL_SERVICE_DEPLOYMENT` env variable (if non-empty),
    /// 2. Otherwise, fallback to locally set value in `self.deployment`.
    pub fn deployment(&self) -> Option<Value> {
        env::var("OTEL_SERVICE_DEPLOYMENT")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(Value::from)
            .or_else(|| self.deployment.clone())
    }

    /// Builds an OpenTelemetry [`Resource`] describing the service for use in traces and metrics.
    ///
    /// This includes:
    /// - Service name (if set or inferred from `OTEL_SERVICE_NAME`)
    /// - Service version (from `OTEL_SERVICE_VERSION` or `self.version`)
    /// - Deployment environment (from `OTEL_SERVICE_DEPLOYMENT` or `self.deployment`)
    ///
    /// The semantic attributes are attached with the OpenTelemetry semantic conventions (see [`SCHEMA_URL`]).
    pub fn resource(&self) -> Resource {
        let mut builder = Resource::builder();
        if let Some(name) = self.name() {
            builder = builder.with_service_name(name)
        }
        let mut attributes = Vec::<KeyValue>::with_capacity(2);
        if let Some(version) = self.version() {
            attributes.push(KeyValue::new(SERVICE_VERSION, version));
        }
        if let Some(deployment) = self.deployment() {
            attributes.push(KeyValue::new(DEPLOYMENT_ENVIRONMENT_NAME, deployment));
        }
        if !attributes.is_empty() {
            builder = builder.with_schema_url(attributes, SCHEMA_URL);
        }
        builder.build()
    }

    /// Initializes the OpenTelemetry tracer provider.
    fn init_tracer_provider(&self, telemetry_protocol: &TelemetryProtocol) -> SdkTracerProvider {
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
            .with_resource(self.resource())
            .with_batch_exporter(exporter)
            .build()
    }

    /// Initializes the OpenTelemetry metrics provider
    fn init_meter_provider(&self, telemetry_protocol: &TelemetryProtocol) -> SdkMeterProvider {
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
            .with_resource(self.resource())
            .with_reader(reader)
            .with_reader(stdout_reader)
            .build();

        global::set_meter_provider(meter_provider.clone());

        meter_provider
    }

    /// Initializes and registers tracing and metrics exporters using OpenTelemetry OTLP exporters.
    ///
    /// If telemetry-related environment variables are present (e.g., `OTEL_EXPORTER_OTLP_ENDPOINT`),
    /// this configures and activates:
    /// - Distributed tracing via `tracing-opentelemetry`
    /// - Metrics collection via `opentelemetry_sdk::metrics`
    ///
    /// Otherwise, it defaults to console logging via `tracing-subscriber`.
    ///
    /// Returns a [`TelemetryProviders`] struct that performs graceful exporter shutdown on `Drop`.
    pub fn register(&self) -> TelemetryProviders {
        let telemetry_protocol = TelemetryProtocol::from_env();
        match telemetry_protocol {
            Some(telemetry_protocol) => {
                let tracer_provider = self.init_tracer_provider(&telemetry_protocol);
                let meter_provider = self.init_meter_provider(&telemetry_protocol);
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
                TelemetryProviders {
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

                TelemetryProviders {
                    tracer_provider: None,
                    meter_provider: None,
                }
            }
        }
    }
}

/// Holds optional OpenTelemetry telemetry providers.
///
/// Returned by [`Telemetry::register`] and designed to ensure
/// a graceful shutdown of tracing and metrics exporters.
pub struct TelemetryProviders {
    /// Tracer provider for OpenTelemetry spans
    pub tracer_provider: Option<SdkTracerProvider>,
    /// Metrics provider for OpenTelemetry metrics
    pub meter_provider: Option<SdkMeterProvider>,
}

/// Drops the telemetry providers with graceful shutdown.
///
/// This ensures that all telemetry data is flushed before the application exits.
/// Any shutdown errors are printed to stderr.
impl Drop for TelemetryProviders {
    fn drop(&mut self) {
        if let Some(tracer_provider) = self.tracer_provider.as_ref()
            && let Err(err) = tracer_provider.shutdown()
        {
            eprintln!("{err:?}");
        }
        if let Some(meter_provider) = self.meter_provider.as_ref()
            && let Err(err) = meter_provider.shutdown()
        {
            eprintln!("{err:?}");
        }
    }
}
