So, this doc is a thinking process for how to do x402 requests.

So, when do x402 request, you could get 402 status code or other. On "other" - you do a pass through.
On 402 - we should parse the requirements, select an applicable requirement, sign the payment and then retry with the
passed payment.

We have two x402 versions. V1: The requirements are in the response body.

```json
{
  "error": "X-PAYMENT header is required",
  "accepts": [
    {
      "scheme": "exact",
      "network": "base-sepolia",
      "maxAmountRequired": "1000",
      "resource": "http://localhost:3001/protected-route",
      "description": "Access to premium content",
      "mimeType": "application/json",
      "payTo": "0xfa3F54AE9C4287CA09a486dfaFaCe7d1d4095d93",
      "maxTimeoutSeconds": 300,
      "asset": "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
      "outputSchema": {
        "input": {
          "type": "http",
          "method": "GET",
          "discoverable": true
        }
      },
      "extra": {
        "name": "USDC",
        "version": "2"
      }
    }
  ],
  "x402Version": 1
}
```

For v2 protocol, the payment requirements are in the header `payment-required`, encoded as base64.

```
payment-required: eyJ4NDAyVmVyc2lvbiI6MiwiZXJyb3IiOiJQYXltZW50IHJlcXVpcmVkIiwicmVzb3VyY2UiOnsidXJsIjoiaHR0cDovL2xvY2FsaG9zdDozMDAwL3Byb3RlY3RlZC1yb3V0ZSIsImRlc2NyaXB0aW9uIjoiQWNjZXNzIHRvIHByZW1pdW0gY29udGVudCIsIm1pbWVUeXBlIjoiIn0sImFjY2VwdHMiOlt7InNjaGVtZSI6ImV4YWN0IiwibmV0d29yayI6ImVpcDE1NTo4NDUzMiIsImFtb3VudCI6IjEwMDAwMCIsImFzc2V0IjoiMHgwMzZDYkQ1Mzg0MmM1NDI2NjM0ZTc5Mjk1NDFlQzIzMThmM2RDRjdlIiwicGF5VG8iOiIweGZhM0Y1NEFFOUM0Mjg3Q0EwOWE0ODZkZmFGYUNlN2QxZDQwOTVkOTMiLCJtYXhUaW1lb3V0U2Vjb25kcyI6MzAwLCJleHRyYSI6eyJuYW1lIjoiVVNEQyIsInZlcnNpb24iOiIyIn19XX0=
```

which is decoded to:

```json
{
  "x402Version": 2,
  "error": "Payment required",
  "resource": {
    "url": "http://localhost:3000/protected-route",
    "description": "Access to premium content",
    "mimeType": ""
  },
  "accepts": [
    {
      "scheme": "exact",
      "network": "eip155:84532",
      "amount": "100000",
      "asset": "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
      "payTo": "0xfa3F54AE9C4287CA09a486dfaFaCe7d1d4095d93",
      "maxTimeoutSeconds": 300,
      "extra": {
        "name": "USDC",
        "version": "2"
      }
    }
  ]
}
```

On the requestor side we should be able to register schemes. One scheme handles a combination of protocol scheme, chain namespace, and scheme per se.
Like, "v1-eip155-exact" is for protocol v1, any eip155 chain, and "exact" scheme.

