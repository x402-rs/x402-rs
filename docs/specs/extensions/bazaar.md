---
Document Type: Extension Specification
Description: x402 extension for resource discovery and cataloging (Bazaar protocol).
Source: https://github.com/coinbase/x402/blob/main/specs/extensions/bazaar.md
Downloaded At: 2026-02-03
---

# Extension: `bazaar`

## Summary

The `bazaar` extension enables **resource discovery and cataloging** for x402-enabled endpoints. Resource servers declare their endpoint specifications (HTTP method, input parameters, and output format) so that facilitators can catalog and index them in a discovery service.

---

## `PaymentRequired`

A resource server advertises its endpoint specification by including the `bazaar` extension in the `extensions` object of the **402 Payment Required** response.

The extension follows the standard v2 pattern:
- **`info`**: Contains the actual discovery data (HTTP method, parameters, output format)
- **`schema`**: JSON Schema that validates the structure of `info`

### Example: GET Endpoint

```json
{
  "x402Version": 2,
  "error": "Payment required",
  "resource": {
    "url": "https://api.example.com/weather",
    "description": "Weather data endpoint",
    "mimeType": "application/json"
  },
  "accepts": [ ... ],
  "extensions": {
    "bazaar": {
      "info": {
        "input": {
          "type": "http",
          "method": "GET",
          "queryParams": {
            "city": "San Francisco"
          }
        },
        "output": {
          "type": "json",
          "example": {
            "city": "San Francisco",
            "weather": "foggy",
            "temperature": 60
          }
        }
      },
      "schema": {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
          "input": {
            "type": "object",
            "properties": {
              "type": { "type": "string", "const": "http" },
              "method": { "type": "string", "enum": ["GET", "HEAD", "DELETE"] },
              "queryParams": {
                "type": "object",
                "properties": {
                  "city": { "type": "string" }
                },
                "required": ["city"]
              }
            },
            "required": ["type", "method"],
            "additionalProperties": false
          },
          "output": {
            "type": "object",
            "properties": {
              "type": { "type": "string" },
              "example": { "type": "object" }
            },
            "required": ["type"]
          }
        },
        "required": ["input"]
      }
    }
  }
}
```

### Example: POST Endpoint

```json
{
  "x402Version": 2,
  "error": "Payment required",
  "resource": {
    "url": "https://api.example.com/search",
    "description": "Search endpoint",
    "mimeType": "application/json"
  },
  "accepts": [ ... ],
  "extensions": {
    "bazaar": {
      "info": {
        "input": {
          "type": "http",
          "method": "POST",
          "bodyType": "json",
          "body": {
            "query": "example"
          }
        },
        "output": {
          "type": "json",
          "example": {
            "results": []
          }
        }
      },
      "schema": {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
          "input": {
            "type": "object",
            "properties": {
              "type": { "type": "string", "const": "http" },
              "method": { "type": "string", "enum": ["POST", "PUT", "PATCH"] },
              "bodyType": { "type": "string", "enum": ["json", "form-data", "text"] },
              "body": { "type": "object" }
            },
            "required": ["type", "method", "bodyType", "body"],
            "additionalProperties": false
          },
          "output": {
            "type": "object",
            "properties": {
              "type": { "type": "string" },
              "example": { "type": "object" }
            },
            "required": ["type"]
          }
        },
        "required": ["input"]
      }
    }
  }
}
```

---

## Discovery Info Structure

### Input Types

The `info.input` object describes how to call the endpoint.

#### Query Parameter Methods (GET, HEAD, DELETE)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | Yes | Always `"http"` |
| `method` | string | Yes | One of `"GET"`, `"HEAD"`, `"DELETE"` |
| `queryParams` | object | No | Query parameter examples |
| `headers` | object | No | Custom header examples |

#### Body Methods (POST, PUT, PATCH)

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | Yes | Always `"http"` |
| `method` | string | Yes | One of `"POST"`, `"PUT"`, `"PATCH"` |
| `bodyType` | string | Yes | One of `"json"`, `"form-data"`, `"text"` |
| `body` | object/string | Yes | Request body example |
| `queryParams` | object | No | Query parameter examples |
| `headers` | object | No | Custom header examples |

### Output Types

The `info.output` object (optional) describes the expected response format:

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | Yes | Response content type (e.g., `"json"`, `"text"`) |
| `format` | string | No | Additional format information |
| `example` | any | No | Example response value |

---

## Schema Validation

The `schema` field contains a JSON Schema (Draft 2020-12) that validates the structure of `info`.

**Requirements:**
- Must use JSON Schema Draft 2020-12
- Must define an `input` property (required)
- May define an `output` property (optional)
- Must validate that `input.type` equals `"http"`
- Must validate the appropriate `method` enum based on operation type

Facilitators **must** validate `info` against `schema` before cataloging.

---

## Facilitator Behavior

When a facilitator receives a `PaymentPayload` containing the `bazaar` extension, it should:

1. **Validate** the `info` field against the provided `schema`
2. **Extract** the discovery information (resource URL, method, input/output specs)

How a facilitator stores, indexes, and exposes discovered resources is an implementation detail. Facilitators may choose to catalog resources in a database, expose them via a discovery API, or process them in any manner they see fit.

---

## Client Behavior

Clients are expected to echo the `bazaar` extension from `PaymentRequired` into their `PaymentPayload`. If the extension is omitted, discovery cataloging will not occur.

---

## Backwards Compatibility

The `bazaar` extension was formalized in x402 v2. Discovery functionality unofficially existed in x402 v1 through the `outputSchema` field.

Facilitators are **not expected** to support v1. If v1 support is desired:

| V1 Location | V2 Location |
|-------------|-------------|
| `accepts[0].outputSchema` | `extensions.bazaar` |
| `accepts[0].resource` | `resource.url` |
| `accepts[0].description` | `description` (top-level) |
| `accepts[0].mimeType` | `mimeType` (top-level) |

V1 had no formal schema validation.
