import {
  AssetAmount,
  Network,
  PaymentPayloadResult,
  PaymentRequirements,
  Price,
  SchemeNetworkClient,
  SchemeNetworkServer,
} from "@x402/core/types";
import { ExactEvmScheme } from "@x402/evm/exact/server";
import { ClientEvmSigner, Permit2Authorization } from "@x402/evm";
import { getAddress, toHex } from "viem";

export class UptoEvmSchemeServer implements SchemeNetworkServer {
  readonly scheme: string = "upto";
  private readonly exact: ExactEvmScheme;

  constructor() {
    this.exact = new ExactEvmScheme();
  }

  parsePrice(price: Price, network: Network): Promise<AssetAmount> {
    return this.exact.parsePrice(price, network);
  }
  async enhancePaymentRequirements(
    paymentRequirements: PaymentRequirements,
    _supportedKind: {
      x402Version: number;
      scheme: string;
      network: Network;
      extra?: Record<string, unknown>;
    },
    _facilitatorExtensions: string[],
  ): Promise<PaymentRequirements> {
    return paymentRequirements;
  }
}

export class UptoEvmSchemeClient implements SchemeNetworkClient {
  readonly scheme: string = "upto";

  private readonly signer: ClientEvmSigner;

  constructor(signer: ClientEvmSigner) {
    this.signer = signer;
  }

  async createPaymentPayload(
    x402Version: number,
    paymentRequirements: PaymentRequirements,
  ): Promise<PaymentPayloadResult> {
    // Extract chain ID from network (format: "eip155:chainId")
    const caipChainId = paymentRequirements.network;
    const caipChainIdReference = caipChainId.split(":")[1];
    const chainId = parseInt(caipChainIdReference, 10);

    // Random nonce: U256 as decimal
    const nonce = createPermit2Nonce();

    // Calculate timing parameters
    const now = Math.floor(Date.now() / 1000);
    // Lower time bound - allow some clock skew
    const validAfter = (now - 600).toString();
    // Upper time bound is enforced by Permit2's deadline field
    const deadline = (now + paymentRequirements.maxTimeoutSeconds).toString();

    // Build the Permit2 authorization
    const permit2Authorization: Permit2Payload["permit2Authorization"] = {
      from: this.signer.address,
      permitted: {
        token: getAddress(paymentRequirements.asset),
        amount: paymentRequirements.amount,
      },
      spender: "0x4020633461b2895a48930Ff97eE8fCdE8E520002", // x402UptoPermit2ProxyAddress
      nonce,
      deadline,
      witness: {
        to: paymentRequirements.payTo as `0x${string}`,
        validAfter,
        extra: "0x" as `0x${string}`,
      },
    };

    // Sign the Permit2 authorization using EIP-712
    const signature = await this.signPermit2Authorization(
      permit2Authorization,
      chainId,
    );

    // Build the payload
    const payload: Permit2Payload = {
      signature,
      permit2Authorization,
    };

    return {
      x402Version,
      payload,
    };
  }

  private async signPermit2Authorization(
    permit2Authorization: any,
    chainId: number,
  ): Promise<`0x${string}`> {
    // EIP-712 domain for Permit2
    const domain = {
      name: "Permit2",
      chainId,
      verifyingContract:
        "0x000000000022D473030F116dDEE9F6B43aC78BA3" as `0x${string}`, // PERMIT2_ADDRESS
    };

    // EIP-712 types for PermitWitnessTransferFrom
    const types = {
      PermitWitnessTransferFrom: [
        { name: "permitted", type: "TokenPermissions" },
        { name: "spender", type: "address" },
        { name: "nonce", type: "uint256" },
        { name: "deadline", type: "uint256" },
        { name: "witness", type: "Witness" },
      ],
      TokenPermissions: [
        { name: "token", type: "address" },
        { name: "amount", type: "uint256" },
      ],
      Witness: [
        { name: "to", type: "address" },
        { name: "validAfter", type: "uint256" },
        { name: "extra", type: "bytes" },
      ],
    };

    // Build the message with proper types
    const message = {
      permitted: {
        token: permit2Authorization.permitted.token,
        amount: BigInt(permit2Authorization.permitted.amount),
      },
      spender: permit2Authorization.spender,
      nonce: BigInt(permit2Authorization.nonce),
      deadline: BigInt(permit2Authorization.deadline),
      witness: {
        to: permit2Authorization.witness.to,
        validAfter: BigInt(permit2Authorization.witness.validAfter),
        extra: permit2Authorization.witness.extra,
      },
    };

    // Sign using EIP-712
    return await this.signer.signTypedData({
      domain,
      types,
      primaryType: "PermitWitnessTransferFrom",
      message,
    });
  }
}

/**
 * Creates a random 256-bit nonce for Permit2.
 * Permit2 uses uint256 nonces (not bytes32 like EIP-3009).
 *
 * @returns A string representation of the random nonce
 */
export function createPermit2Nonce(): string {
  const randomBytes = crypto.getRandomValues(new Uint8Array(32));
  return BigInt(toHex(randomBytes)).toString();
}

export type Permit2Payload = {
  signature: `0x${string}`;
  permit2Authorization: Permit2Authorization & {
    from: `0x${string}`;
  };
};
