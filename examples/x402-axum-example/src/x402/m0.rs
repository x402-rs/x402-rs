use axum::body::Body;
use axum::response::{IntoResponse, Response};
use http::{HeaderMap, HeaderValue, StatusCode};
use std::convert::Infallible;
use std::fmt::Display;
use tower::Service;
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto;
use x402_rs::proto::client::Transport;
use x402_rs::proto::v1::V1PriceTag;
use x402_rs::proto::{SettleRequest, SettleResponse, VerifyRequest, v1};
use x402_rs::util::Base64Bytes;

/// A service-level helper struct responsible for verifying and settling
/// x402 payments based on request headers and known payment requirements.
pub struct X402Paygate<TPaymentRequirements, TFacilitator> {
    pub facilitator: TFacilitator,
    pub settle_before_execution: bool,
    /// Whether to settle payment before executing the request (true) or after (false)
    pub payment_requirements: Vec<TPaymentRequirements>,
    pub description: Option<String>,
    pub mime_type: Option<String>, // TODO ARC!!
    /// Optional resource URL. If not set, it will be derived from a request URI.
    pub resource: Option<Url>,
}

// impl<TPriceTag, TFacilitator> X402Paygate<TPriceTag, TFacilitator>
// where
//     TFacilitator: Facilitator,
// {
//     pub async fn call<
//         ReqBody,
//         ResBody,
//         S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
//     >(
//         self,
//         inner: S,
//         req: http::Request<ReqBody>,
//     ) -> Result<Response, Infallible>
//     where
//         S::Response: IntoResponse,
//         S::Error: IntoResponse,
//         S::Future: Send,
//     {
//         Ok(self.handle_request(inner, req).await)
//     }
//
//     /// Orchestrates the full payment lifecycle: verifies the request, calls to the inner handler, and settles the payment, returns proper HTTP response.
//     #[cfg_attr(
//         feature = "telemetry",
//         instrument(name = "x402.handle_request", skip_all)
//     )]
//     pub async fn handle_request<
//         ReqBody,
//         ResBody,
//         S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
//     >(
//         self,
//         inner: S,
//         req: http::Request<ReqBody>,
//     ) -> Response
//     where
//         S::Response: IntoResponse,
//         S::Error: IntoResponse,
//         S::Future: Send,
//     {
//         let payment_payload = match self.extract_payment_payload(req.headers()).await {
//             Ok(payment_payload) => payment_payload,
//             Err(err) => {
//                 #[cfg(feature = "telemetry")]
//                 tracing::event!(Level::INFO, status = "failed", "No valid payment provided");
//                 return err.into_response();
//             }
//         };
//         let verify_request = match self.verify_payment(payment_payload).await {
//             Ok(verify_request) => verify_request,
//             Err(err) => return err.into_response(),
//         };
//
//         if self.settle_before_execution {
//             // Settlement before execution: settle payment first, then call inner handler
//             #[cfg(feature = "telemetry")]
//             tracing::debug!("Settling payment before request execution");
//
//             let verify_request = VerifyRequest::from(serde_json::to_value(verify_request).unwrap());
//
//             let settlement = match self.settle_payment(&verify_request).await {
//                 Ok(settlement) => settlement,
//                 Err(err) => return err.into_response(),
//             };
//
//             let header_value = match self.settlement_to_header(settlement) {
//                 Ok(header) => header,
//                 Err(response) => return *response,
//             };
//
//             // Settlement succeeded, now execute the request
//             let response = match Self::call_inner(inner, req).await {
//                 Ok(response) => response,
//                 Err(err) => return err.into_response(),
//             };
//
//             // Add payment response header
//             let mut res = response;
//             res.headers_mut().insert("X-Payment-Response", header_value);
//             res.into_response()
//         } else {
//             // Settlement after execution (default): call inner handler first, then settle
//             #[cfg(feature = "telemetry")]
//             tracing::debug!("Settling payment after request execution");
//
//             let response = match Self::call_inner(inner, req).await {
//                 Ok(response) => response,
//                 Err(err) => return err.into_response(),
//             };
//
//             if response.status().is_client_error() || response.status().is_server_error() {
//                 return response.into_response();
//             }
//
//             let settle_request = SettleRequest::from(serde_json::to_value(verify_request).unwrap());
//
//             let settlement = match self.settle_payment(&settle_request).await {
//                 Ok(settlement) => settlement,
//                 Err(err) => return err.into_response(),
//             };
//
//             let header_value = match self.settlement_to_header(settlement) {
//                 Ok(header) => header,
//                 Err(response) => return *response,
//             };
//
//             let mut res = response;
//             res.headers_mut().insert("X-Payment-Response", header_value);
//             res.into_response()
//         }
//     }
//
//     /// Converts a [`SettleResponse`] into an HTTP header value.
//     ///
//     /// Returns an error response if conversion fails.
//     fn settlement_to_header(
//         &self,
//         settlement: SettleResponse,
//     ) -> Result<HeaderValue, Box<Response>> {
//         let json = serde_json::to_vec(&settlement).map_err(|err| {
//             X402Error::settlement_failed(
//                 err,
//                 vec![], // self.payment_requirements.as_ref().clone()
//             )
//             .into_response()
//         })?;
//         let payment_header = Base64Bytes::encode(json);
//
//         HeaderValue::from_bytes(payment_header.as_ref()).map_err(|err| {
//             let response = X402Error::settlement_failed(
//                 err,
//                 vec![], // self.payment_requirements.as_ref().clone()
//             )
//             .into_response();
//             Box::new(response)
//         })
//     }
//
//     /// Attempts to settle a verified payment on-chain. Returns [`SettleResponse`] on success or emits a 402 error.
//     #[cfg_attr(
//         feature = "telemetry",
//         instrument(name = "x402.settle_payment", skip_all, err)
//     )]
//     pub async fn settle_payment(
//         &self,
//         settle_request: &SettleRequest,
//     ) -> Result<SettleResponse, X402Error> {
//         let settle_response: proto::SettleResponse =
//             self.facilitator.settle(settle_request).await.map_err(|e| {
//                 X402Error::settlement_failed(
//                     e,
//                     vec![], // self.payment_requirements.as_ref().clone()
//                 )
//             })?;
//         let settle_response_v1: v1::SettleResponse =
//             serde_json::from_value(settle_response.0.clone()).unwrap();
//
//         match settle_response_v1 {
//             v1::SettleResponse::Success { .. } => Ok(settle_response),
//             v1::SettleResponse::Error { reason, network } => {
//                 Err(X402Error::settlement_failed(
//                     reason,
//                     vec![], // self.payment_requirements.as_ref().clone(),
//                 ))
//             }
//         }
//     }
//
//     /// Calls the inner service with proper telemetry instrumentation.
//     async fn call_inner<
//         ReqBody,
//         ResBody,
//         S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
//     >(
//         mut inner: S,
//         req: http::Request<ReqBody>,
//     ) -> Result<http::Response<ResBody>, S::Error>
//     where
//         S::Future: Send,
//     {
//         #[cfg(feature = "telemetry")]
//         {
//             inner
//                 .call(req)
//                 .instrument(tracing::info_span!("inner"))
//                 .await
//         }
//         #[cfg(not(feature = "telemetry"))]
//         {
//             inner.call(req).await
//         }
//     }
//
//     /// Parses the `X-Payment` header and returns a decoded [`PaymentPayload`], or constructs a 402 error if missing or malformed as [`X402Error`].
//     pub async fn extract_payment_payload(
//         &self,
//         headers: &HeaderMap,
//     ) -> Result<v1::PaymentPayload<String, serde_json::Value>, X402Error> {
//         let payment_header = extract_payment_header(headers);
//         match payment_header {
//             None => {
//                 println!("No Payment Header");
//             }
//             Some(payment_header) => match payment_header {
//                 Transport::V1(bytes) => {
//                     println!("payment header v1")
//                 }
//                 Transport::V2(bytes) => {
//                     println!("payment header v2")
//                 }
//             },
//         }
//
//         let payment_header = headers.get("X-Payment");
//         let supported = self.facilitator.supported().await.map_err(|e| {
//             X402Error(v1::PaymentRequired {
//                 x402_version: v1::X402Version1,
//                 error: Some(format!("Unable to retrieve supported payment schemes: {e}")),
//                 accepts: vec![],
//             })
//         })?;
//         match payment_header {
//             None => {
//                 // let requirements = self
//                 //     .payment_requirements
//                 //     .as_ref()
//                 //     .iter()
//                 //     .map(|r| {
//                 //         let mut r = r.clone();
//                 //         let network = r.network;
//                 //         let extra = supported
//                 //             .kinds
//                 //             .iter()
//                 //             .find(|s| s.network == network.to_string())
//                 //             .cloned()
//                 //             .and_then(|s| s.extra);
//                 //         if let Some(extra) = extra {
//                 //             r.extra = Some(json!({
//                 //                 "feePayer": extra.fee_payer
//                 //             }));
//                 //             r
//                 //         } else {
//                 //             r
//                 //         }
//                 //     })
//                 //     .collect::<Vec<_>>();
//                 let requirements = vec![];
//                 Err(X402Error::payment_header_required(requirements))
//             }
//             Some(payment_header) => {
//                 let base64 = Base64Bytes::from(payment_header.as_bytes())
//                     .decode()
//                     .map_err(|err| X402Error::invalid_payment_header(vec![]))?;
//                 let p = serde_json::from_slice::<v1::PaymentPayload<String, serde_json::Value>>(
//                     base64.as_ref(),
//                 )
//                 .map_err(|_| X402Error::invalid_payment_header(vec![]))?;
//                 println!("pp.0 {:?}", p);
//                 Ok(p)
//                 // match p {
//                 //     Ok(payment_payload) => Ok(payment_payload),
//                 //     Err(_) => Err(X402Error::invalid_payment_header(
//                 //         // self.payment_requirements.as_ref().clone(),
//                 //         vec![]
//                 //     )),
//                 // }
//             }
//         }
//     }
//
//     /// Finds the payment requirement entry matching the given payload's scheme and network.
//     fn find_matching_payment_requirements(
//         &self,
//         payment_payload: &v1::PaymentPayload<String, serde_json::Value>,
//     ) -> Option<serde_json::Value> {
//         // self.payment_requirements
//         //     .iter()
//         //     .find(|requirement| {
//         //         requirement.scheme == payment_payload.scheme
//         //             && requirement.network == payment_payload.network
//         //     })
//         //     .cloned()
//         None
//     }
//
//     /// Verifies the provided payment using the facilitator and known requirements. Returns a [`VerifyRequest`] if the payment is valid.
//     #[cfg_attr(
//         feature = "telemetry",
//         instrument(name = "x402.verify_payment", skip_all, err)
//     )]
//     pub async fn verify_payment(
//         &self,
//         payment_payload: v1::PaymentPayload<String, serde_json::Value>,
//     ) -> Result<VerifyRequest, X402Error> {
//         let selected = self
//             .find_matching_payment_requirements(&payment_payload)
//             .ok_or(X402Error::no_payment_matching(
//                 // self.payment_requirements.as_ref().clone(),
//                 vec![],
//             ))?;
//         let verify_request = v1::VerifyRequest {
//             x402_version: v1::X402Version1,
//             payment_payload,
//             payment_requirements: selected,
//         };
//         let verify_request =
//             proto::VerifyRequest::from(serde_json::to_value(verify_request).unwrap());
//         let verify_response = self
//             .facilitator
//             .verify(&verify_request)
//             .await
//             .map_err(|e| {
//                 X402Error::verification_failed(
//                     e,
//                     vec![], // self.payment_requirements.as_ref().clone()
//                 )
//             })?;
//
//         let verify_response_v1: v1::VerifyResponse =
//             serde_json::from_value(verify_response.0.clone()).unwrap();
//
//         match verify_response_v1 {
//             v1::VerifyResponse::Valid { .. } => Ok(verify_request),
//             v1::VerifyResponse::Invalid { reason, .. } => Err(X402Error::verification_failed(
//                 reason,
//                 vec![], // self.payment_requirements.as_ref().clone(),
//             )),
//         }
//     }
// }

// #[derive(Debug)]
// /// Wrapper for producing a `402 Payment Required` response with context.
// pub struct X402Error(v1::PaymentRequired);
//
// static ERR_PAYMENT_HEADER_REQUIRED: &'static str = "X-PAYMENT header is required";
// static ERR_INVALID_PAYMENT_HEADER: &'static str = "Invalid or malformed payment header";
// static ERR_NO_PAYMENT_MATCHING: &'static str = "Unable to find matching payment requirements";
//
// /// Middleware application error with detailed context.
// ///
// /// Encapsulates a `402 Payment Required` response that can be returned
// /// when payment verification or settlement fails.
// impl X402Error {
//     // pub fn payment_header_required(payment_requirements: Vec<v1::PaymentRequired>) -> Self {
//     //     let payment_required_response = v1::PaymentRequired {
//     //         error: ERR_PAYMENT_HEADER_REQUIRED.clone(),
//     //         accepts: payment_requirements,
//     //         x402_version: X402Version::V1,
//     //     };
//     //     Self(payment_required_response)
//     // }
//
//     pub fn payment_header_required(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
//         let payment_required_response = v1::PaymentRequired {
//             error: Some(ERR_PAYMENT_HEADER_REQUIRED.to_string()),
//             accepts: payment_requirements,
//             x402_version: v1::X402Version1,
//         };
//         Self(payment_required_response)
//     }
//
//     // pub fn invalid_payment_header(payment_requirements: Vec<PaymentRequirements>) -> Self {
//     //     let payment_required_response = PaymentRequiredResponse {
//     //         error: ERR_INVALID_PAYMENT_HEADER.clone(),
//     //         accepts: payment_requirements,
//     //         x402_version: X402Version::V1,
//     //     };
//     //     Self(payment_required_response)
//     // }
//
//     pub fn invalid_payment_header(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
//         let payment_required_response = v1::PaymentRequired {
//             error: Some(ERR_INVALID_PAYMENT_HEADER.to_string()),
//             accepts: payment_requirements,
//             x402_version: v1::X402Version1,
//         };
//         Self(payment_required_response)
//     }
//
//     // pub fn no_payment_matching(payment_requirements: Vec<PaymentRequirements>) -> Self {
//     //     let payment_required_response = PaymentRequiredResponse {
//     //         error: ERR_NO_PAYMENT_MATCHING.clone(),
//     //         accepts: payment_requirements,
//     //         x402_version: X402Version::V1,
//     //     };
//     //     Self(payment_required_response)
//     // }
//
//     pub fn no_payment_matching(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
//         let payment_required_response = v1::PaymentRequired {
//             error: Some(ERR_NO_PAYMENT_MATCHING.to_string()),
//             accepts: payment_requirements,
//             x402_version: v1::X402Version1,
//         };
//         Self(payment_required_response)
//     }
//
//     // pub fn verification_failed<E2: Display>(
//     //         error: E2,
//     //         payment_requirements: Vec<PaymentRequirements>,
//     //     ) -> Self {
//     //         let payment_required_response = PaymentRequiredResponse {
//     //             error: format!("Verification Failed: {error}"),
//     //             accepts: payment_requirements,
//     //             x402_version: X402Version::V1,
//     //         };
//     //         Self(payment_required_response)
//     //     }
//
//     pub fn verification_failed<E2: Display>(
//         error: E2,
//         payment_requirements: Vec<v1::PaymentRequirements>,
//     ) -> Self {
//         let payment_required_response = v1::PaymentRequired {
//             error: Some(format!("Verification Failed: {error}")),
//             accepts: payment_requirements,
//             x402_version: v1::X402Version1,
//         };
//         Self(payment_required_response)
//     }
//
//     //
//     // pub fn settlement_failed<E2: Display>(
//     //     error: E2,
//     //     payment_requirements: Vec<PaymentRequirements>,
//     // ) -> Self {
//     //     let payment_required_response = PaymentRequiredResponse {
//     //         error: format!("Settlement Failed: {error}"),
//     //         accepts: payment_requirements,
//     //         x402_version: X402Version::V1,
//     //     };
//     //     Self(payment_required_response)
//     // }
//
//     pub fn settlement_failed<E2: Display>(
//         error: E2,
//         payment_requirements: Vec<v1::PaymentRequirements>,
//     ) -> Self {
//         let payment_required_response = v1::PaymentRequired {
//             error: Some(format!("Settlement Failed: {error}")),
//             accepts: payment_requirements,
//             x402_version: v1::X402Version1,
//         };
//         Self(payment_required_response)
//     }
// }
//
// impl IntoResponse for X402Error {
//     fn into_response(self) -> Response {
//         let payment_required_response_bytes =
//             serde_json::to_vec(&self.0).expect("serialization failed");
//         let body = Body::from(payment_required_response_bytes);
//         Response::builder()
//             .status(StatusCode::PAYMENT_REQUIRED)
//             .header("Content-Type", "application/json")
//             .body(body)
//             .expect("Fail to construct response")
//     }
// }

/// Wrapper for producing a `402 Payment Required` response with context.
#[derive(Debug)]
pub struct X402Error(v1::PaymentRequired);

static ERR_PAYMENT_HEADER_REQUIRED: &'static str = "X-PAYMENT header is required";
static ERR_INVALID_PAYMENT_HEADER: &'static str = "Invalid or malformed payment header";
static ERR_NO_PAYMENT_MATCHING: &'static str = "Unable to find matching payment requirements";

/// Middleware application error with detailed context.
///
/// Encapsulates a `402 Payment Required` response that can be returned
/// when payment verification or settlement fails.
impl X402Error {
    /// Direct constructor for when we already have a PaymentRequired response
    pub fn from_payment_required(payment_required: v1::PaymentRequired) -> Self {
        Self(payment_required)
    }

    pub fn payment_header_required(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(ERR_PAYMENT_HEADER_REQUIRED.to_string()),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn invalid_payment_header(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(ERR_INVALID_PAYMENT_HEADER.to_string()),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn no_payment_matching(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(ERR_NO_PAYMENT_MATCHING.to_string()),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn verification_failed<E2: Display>(
        error: E2,
        payment_requirements: Vec<v1::PaymentRequirements>,
    ) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(format!("Verification Failed: {error}")),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn settlement_failed<E2: Display>(
        error: E2,
        payment_requirements: Vec<v1::PaymentRequirements>,
    ) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(format!("Settlement Failed: {error}")),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }
}

impl IntoResponse for X402Error {
    fn into_response(self) -> Response {
        let payment_required_response_bytes =
            serde_json::to_vec(&self.0).expect("serialization failed");
        let body = Body::from(payment_required_response_bytes);
        Response::builder()
            .status(StatusCode::PAYMENT_REQUIRED)
            .header("Content-Type", "application/json")
            .body(body)
            .expect("Fail to construct response")
    }
}

impl<TFacilitator> X402Paygate<v1::PaymentRequirements, TFacilitator>
where
    TFacilitator: Facilitator,
{
    /// Calls the inner service with proper telemetry instrumentation.
    async fn call_inner<
        ReqBody,
        ResBody,
        S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
    >(
        mut inner: S,
        req: http::Request<ReqBody>,
    ) -> Result<http::Response<ResBody>, S::Error>
    where
        S::Future: Send,
    {
        inner.call(req).await
    }

    /// Parses the `X-Payment` header and returns a decoded [`PaymentPayload`], or constructs a 402 error if missing or malformed as [`X402Error`].
    pub async fn extract_payment_payload(
        &self,
        headers: &HeaderMap,
    ) -> Result<v1::PaymentPayload<String, serde_json::Value>, X402Error> {
        println!("  [extract_payment_payload] Checking for X-Payment header...");
        println!("  [extract_payment_payload] Available headers: {:?}", headers.keys().collect::<Vec<_>>());

        let payment_header = headers.get("X-Payment");
        println!("  [extract_payment_payload] X-Payment header present: {}", payment_header.is_some());

        match payment_header {
            None => {
                println!("  [extract_payment_payload] ❌ No X-Payment header found");
                println!("  [extract_payment_payload] Returning payment requirements for 402 response");

                // Get supported schemes from facilitator to enrich payment requirements with extra data
                println!("  [extract_payment_payload] Getting supported schemes from facilitator...");
                let supported = self.facilitator.supported().await.map_err(|e| {
                    println!("  [extract_payment_payload] ❌ Failed to get supported schemes: {}", e);
                    X402Error(v1::PaymentRequired {
                        x402_version: v1::X402Version1,
                        error: Some(format!("Unable to retrieve supported payment schemes: {e}")),
                        accepts: vec![],
                    })
                })?;

                println!("  [extract_payment_payload] ✅ Got supported schemes: {:?}", supported);
                Err(X402Error::payment_header_required(self.payment_requirements.clone()))
            }
            Some(payment_header) => {
                println!("  [extract_payment_payload] ✅ X-Payment header found");
                println!("  [extract_payment_payload] Header value: {:?}", payment_header);

                let base64_result = Base64Bytes::from(payment_header.as_bytes()).decode();
                println!("  [extract_payment_payload] Base64 decode attempt...");

                let base64 = base64_result.map_err(|err| {
                    println!("  [extract_payment_payload] ❌ Base64 decode failed: {}", err);
                    X402Error::invalid_payment_header(self.payment_requirements.clone())
                })?;

                println!("  [extract_payment_payload] ✅ Base64 decoded successfully");
                println!("  [extract_payment_payload] Decoded bytes length: {}", base64.len());

                let payment_payload_result: Result<v1::PaymentPayload<String, serde_json::Value>, _> =
                    serde_json::from_slice(base64.as_ref());
                println!("  [extract_payment_payload] JSON deserialization attempt...");

                let payment_payload = payment_payload_result.map_err(|e| {
                    println!("  [extract_payment_payload] ❌ JSON deserialization failed: {}", e);
                    X402Error::invalid_payment_header(self.payment_requirements.clone())
                })?;

                println!("  [extract_payment_payload] ✅ Payment payload extracted successfully");
                println!("  [extract_payment_payload] Payload scheme: {}", payment_payload.scheme);
                println!("  [extract_payment_payload] Payload network: {}", payment_payload.network);
                println!("  [extract_payment_payload] Payload data: {:?}", payment_payload.payload);
                Ok(payment_payload)
            }
        }
    }

    /// Finds the payment requirement entry matching the given payload's scheme and network.
    fn find_matching_payment_requirements(
        &self,
        payment_payload: &v1::PaymentPayload<String, serde_json::Value>,
    ) -> Option<serde_json::Value> {
        println!("  [find_matching_payment_requirements] Looking for matching requirement...");
        println!("  [find_matching_payment_requirements] Payload scheme: {}", payment_payload.scheme);
        println!("  [find_matching_payment_requirements] Payload network: {}", payment_payload.network);
        println!("  [find_matching_payment_requirements] Available requirements: {}", self.payment_requirements.len());

        // Convert payment requirements to serde_json::Value for comparison
        let matched = self.payment_requirements
            .iter()
            .find(|requirement| {
                let matches = requirement.scheme == payment_payload.scheme
                    && requirement.network == payment_payload.network;
                println!("  [find_matching_payment_requirements] Checking requirement: scheme={}, network={}, matches={}",
                    requirement.scheme, requirement.network, matches);
                matches
            });

        match matched {
            Some(matched_requirement) => {
                println!("  [find_matching_payment_requirements] ✅ Found matching requirement");
                let json_value = serde_json::to_value(matched_requirement).ok();
                println!("  [find_matching_payment_requirements] Converted to JSON: {}", json_value.is_some());
                json_value
            }
            None => {
                println!("  [find_matching_payment_requirements] ❌ No matching requirement found");
                None
            }
        }
    }

    /// Verifies the provided payment using the facilitator and known requirements. Returns a [`VerifyRequest`] if the payment is valid.
    pub async fn verify_payment(
        &self,
        payment_payload: v1::PaymentPayload<String, serde_json::Value>,
    ) -> Result<VerifyRequest, X402Error> {
        println!("  [verify_payment] Starting payment verification...");
        println!("  [verify_payment] Payment payload scheme: {}", payment_payload.scheme);
        println!("  [verify_payment] Payment payload network: {}", payment_payload.network);

        let selected = self
            .find_matching_payment_requirements(&payment_payload)
            .ok_or(X402Error::no_payment_matching(vec![]))?;

        println!("  [verify_payment] ✅ Found matching payment requirements");
        println!("  [verify_payment] Selected requirements: {:?}", selected);

        let verify_request = v1::VerifyRequest {
            x402_version: v1::X402Version1,
            payment_payload: payment_payload.clone(),
            payment_requirements: selected.clone(),
        };

        println!("  [verify_payment] Created verify request");

        let verify_request_json = serde_json::to_value(verify_request.clone()).unwrap();
        println!("  [verify_payment] Verify request JSON: {:?}", verify_request_json);

        let verify_request_proto =
            proto::VerifyRequest::from(verify_request_json);

        println!("  [verify_payment] Sending request to facilitator...");
        let verify_response = self
            .facilitator
            .verify(&verify_request_proto)
            .await
            .map_err(|e| {
                println!("  [verify_payment] ❌ Facilitator verification failed: {}", e);
                X402Error::verification_failed(e, vec![])
            })?;

        println!("  [verify_payment] ✅ Received response from facilitator");
        println!("  [verify_payment] Facilitator response: {:?}", verify_response);

        let verify_response_v1: v1::VerifyResponse =
            serde_json::from_value(verify_response.0.clone()).unwrap();

        println!("  [verify_payment] Parsed facilitator response");

        match verify_response_v1 {
            v1::VerifyResponse::Valid { .. } => {
                println!("  [verify_payment] ✅ Payment verified successfully by facilitator");
                Ok(verify_request_proto)
            }
            v1::VerifyResponse::Invalid { reason, .. } => {
                println!("  [verify_payment] ❌ Payment verification failed: {}", reason);
                Err(X402Error::verification_failed(reason, vec![]))
            }
        }
    }

    /// Attempts to settle a verified payment on-chain. Returns [`SettleResponse`] on success or emits a 402 error.
    pub async fn settle_payment(
        &self,
        settle_request: &SettleRequest,
    ) -> Result<SettleResponse, X402Error> {
        let settle_response: proto::SettleResponse = self
            .facilitator
            .settle(settle_request)
            .await
            .map_err(|e| X402Error::settlement_failed(e, vec![]))?;
        let settle_response_v1: v1::SettleResponse =
            serde_json::from_value(settle_response.0.clone()).unwrap();

        match settle_response_v1 {
            v1::SettleResponse::Success { .. } => Ok(settle_response),
            v1::SettleResponse::Error { reason, network } => {
                Err(X402Error::settlement_failed(reason, vec![]))
            }
        }
    }

    /// Converts a [`SettleResponse`] into an HTTP header value.
    ///
    /// Returns an error response if conversion fails.
    fn settlement_to_header(
        &self,
        settlement: SettleResponse,
    ) -> Result<HeaderValue, Box<Response>> {
        let json = serde_json::to_vec(&settlement)
            .map_err(|err| X402Error::settlement_failed(err, vec![]).into_response())?;
        let payment_header = Base64Bytes::encode(json);

        HeaderValue::from_bytes(payment_header.as_ref()).map_err(|err| {
            let response = X402Error::settlement_failed(err, vec![]).into_response();
            Box::new(response)
        })
    }

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
        println!("\n=== X402 PAYMENT GATEWAY FLOW START ===");
        println!("Request URI: {:?}", req.uri());
        println!("Request method: {:?}", req.method());
        println!("Payment requirements configured: {:?}", self.payment_requirements);

        // Extract payment payload from headers
        println!("\n--- STEP 1: Extracting payment payload ---");
        let payment_payload = match self.extract_payment_payload(req.headers()).await {
            Ok(payment_payload) => {
                println!("✓ Successfully extracted payment payload");
                payment_payload
            }
            Err(err) => {
                println!("✗ Failed to extract payment payload, returning error");
                return Ok(err.into_response());
            }
        };

        // Verify the payment meets requirements
        println!("\n--- STEP 2: Verifying payment ---");
        let verify_request = match self.verify_payment(payment_payload).await {
            Ok(verify_request) => {
                println!("✓ Payment verification successful");
                verify_request
            }
            Err(err) => {
                println!("✗ Payment verification failed, returning error");
                return Ok(err.into_response());
            }
        };

        println!("\n--- STEP 3: Calling inner service ---");
        // FIXME: Implement settle_before_execution logic later
        // For now, always settle after successful execution

        // Call inner service first
        let response = match Self::call_inner(inner, req).await {
            Ok(response) => {
                println!("✓ Inner service completed successfully");
                println!("Response status: {}", response.status());
                response
            }
            Err(err) => {
                println!("✗ Inner service failed");
                return Ok(err.into_response());
            }
        };

        // Only settle if request was successful
        if response.status().is_client_error() || response.status().is_server_error() {
            println!("⚠️  Request failed with status {}, skipping settlement", response.status());
            return Ok(response.into_response());
        }

        println!("\n--- STEP 4: Settling payment ---");
        // Convert verify request to settle request
        let settle_request = SettleRequest::from(serde_json::to_value(verify_request).unwrap());
        println!("Settle request created");

        // Attempt settlement
        let settlement = match self.settle_payment(&settle_request).await {
            Ok(settlement) => {
                println!("✓ Payment settlement successful");
                settlement
            }
            Err(err) => {
                println!("✗ Payment settlement failed");
                return Ok(err.into_response());
            }
        };

        println!("\n--- STEP 5: Finalizing response ---");
        // Convert settlement to header value
        let header_value = match self.settlement_to_header(settlement) {
            Ok(header) => {
                println!("✓ Settlement header created");
                header
            }
            Err(response) => {
                println!("✗ Failed to create settlement header");
                return Ok(*response);
            }
        };

        // Add payment response header and return
        let mut res = response;
        res.headers_mut().insert("X-Payment-Response", header_value);
        println!("✓ Added X-Payment-Response header to response");
        println!("=== X402 PAYMENT GATEWAY FLOW COMPLETE ===\n");
        Ok(res.into_response())
    }
}

fn extract_payment_header(header_map: &HeaderMap) -> Option<Transport<&[u8]>> {
    let x_payment = header_map.get("X-Payment");
    if let Some(x_payment) = x_payment {
        return Some(Transport::V1(x_payment.as_bytes()));
    }
    let payment_signature = header_map.get("Payment-Signature");
    if let Some(payment_signature) = payment_signature {
        return Some(Transport::V2(payment_signature.as_bytes()));
    }
    None
}
