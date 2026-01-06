use std::sync::Arc;
use url::Url;

#[derive(Debug, Clone, Default)]
pub enum BaseUrl {
    #[default]
    None,
    Some(Arc<Url>),
}

impl BaseUrl {
    pub fn new(url: Url) -> Self {
        Self::Some(Arc::new(url))
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
}
