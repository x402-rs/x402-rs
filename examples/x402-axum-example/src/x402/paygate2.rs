use std::sync::Arc;
use http::Uri;
use url::Url;
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

#[derive(Debug, Clone)]
pub struct PriceTagContainer<TPriceTag>(Arc<Vec<TPriceTag>>);

pub struct X402Paygate2<TPriceTag, TFacilitator> {
    facilitator: TFacilitator,
    settle_before_execution: bool,
    base_url: Arc<Url>,
}
