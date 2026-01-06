use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::Uri;
use std::convert::Infallible;
use std::sync::Arc;
use tower::Service;
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto::v1::V1PriceTag;
use x402_rs::proto::v2;

#[derive(Debug, Clone)]
pub struct ResourceInfoBuilder {
    pub description: String,
    pub mime_type: String,
    pub url: Option<String>,
}

impl Default for ResourceInfoBuilder {
    fn default() -> Self {
        Self {
            description: "".to_string(),
            mime_type: "application/json".to_string(),
            url: None,
        }
    }
}

impl ResourceInfoBuilder {
    // Determine the resource URL (static or dynamic)
    pub fn as_resource_info(&self, base_url: &Url, request_uri: &Uri) -> v2::ResourceInfo {
        v2::ResourceInfo {
            description: self.description.clone(),
            mime_type: self.mime_type.clone(),
            url: self.url.clone().unwrap_or_else(|| {
                let mut url = base_url.clone();
                url.set_path(request_uri.path());
                url.set_query(request_uri.query());
                url.to_string()
            }),
        }
    }
}

pub struct V1Paygate<TFacilitator> {
    pub facilitator: TFacilitator,
    pub settle_before_execution: bool,
    pub base_url: Arc<Url>,
    pub accepts: Arc<Vec<V1PriceTag>>,
    pub resource: Arc<ResourceInfoBuilder>,
}

impl<TFacilitator> V1Paygate<TFacilitator>
where TFacilitator: Facilitator {
    #[cfg_attr(
        feature = "telemetry",
        instrument(name = "x402.handle_request", skip_all)
    )]
    pub async fn call<
        ReqBody,
        ResBody,
        S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
    >(
        self,
        inner: S,
        req: http::Request<ReqBody>,
    ) -> Result<Response, Infallible>
    where
        S::Response: IntoResponse,
        S::Error: IntoResponse,
        S::Future: Send,
    {
        todo!()
    }
}


#[derive(Debug, thiserror::Error)]
enum PaygateError {
    #[error(transparent)]
    Verification(#[from] VerificationError),
}

#[derive(Debug, thiserror::Error)]
enum VerificationError {
    #[error("{0} header is required")]
    PaymentHeaderRequired(String),
}
