use alloy_primitives::{Address, U256};

use crate::proto::v2;
use crate::scheme::v1_eip155_exact::types::{ExactEvmPayload, PaymentRequirementsExtra};

pub use crate::scheme::v1_eip155_exact::types::ExactScheme;

pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, ExactEvmPayload>;
pub type PaymentRequirements = v2::PaymentRequirements<ExactScheme, U256, Address, PaymentRequirementsExtra>;