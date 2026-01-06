use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::Uri;
use std::convert::Infallible;
use std::sync::Arc;
use tower::Service;
use url::Url;
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

pub struct V1Paygate<TFacilitator, TInner, TReq> {
    pub facilitator: TFacilitator,
    pub settle_before_execution: bool,
    pub base_url: Arc<Url>,
    pub accepts: Arc<Vec<V1PriceTag>>,
    pub resource: Arc<ResourceInfoBuilder>,
    pub inner: TInner,
    pub req: TReq,
}

impl<TFacilitator, TInner, TReq> V1Paygate<TFacilitator, TInner, TReq> {
    pub async fn call(self) -> Result<Response, Infallible> {
        todo!()
    }
}
