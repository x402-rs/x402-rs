use axum::http::Request;
use std::task::{Context, Poll};
use tower::{Layer, Service};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TraceId(pub String);

#[derive(Clone)]
pub struct TraceIdLayer;

impl<S> Layer<S> for TraceIdLayer {
    type Service = RequestIdService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestIdService { inner }
    }
}

#[derive(Clone)]
pub struct RequestIdService<S> {
    inner: S,
}

impl<S, B> Service<Request<B>> for RequestIdService<S>
where
    S: Service<Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        let request_id = Uuid::now_v7().to_string();
        req.extensions_mut().insert(crate::TraceId(request_id));
        self.inner.call(req)
    }
}
