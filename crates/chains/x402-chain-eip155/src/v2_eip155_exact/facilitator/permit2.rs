use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::Eip155MetaTransactionProvider;
use crate::v2_eip155_exact::{Permit2PaymentPayload, Permit2PaymentRequirements};

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
    todo!("Permit2")
}
