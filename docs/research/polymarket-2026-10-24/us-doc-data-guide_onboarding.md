> ## Documentation Index
> Fetch the complete documentation index at: https://docs.polymarket.us/llms.txt
> Use this file to discover all available pages before exploring further.

# Onboarding

> Get started with the Polymarket Exchange API

## Step 1: Generate Your Key Pairs

Generate an RSA key pair for each environment you need access to. You will share only the **public keys** with Polymarket.

```bash theme={null}
# Replace 'acme' with your company name

# Development
openssl genrsa -out acme_dev_private_key.pem 2048
openssl rsa -in acme_dev_private_key.pem -pubout -out acme_dev_public_key.pem

# Pre-production
openssl genrsa -out acme_preprod_private_key.pem 2048
openssl rsa -in acme_preprod_private_key.pem -pubout -out acme_preprod_public_key.pem

# Production
openssl genrsa -out acme_prod_private_key.pem 2048
openssl rsa -in acme_prod_private_key.pem -pubout -out acme_prod_public_key.pem
```

Keep your private keys secure. Never share them with anyone.

## Step 2: Submit Your Onboarding Request

Contact [data@polymarket.us](mailto:data@polymarket.us) to receive the Market Data Agreement for read-only market data access.

Once you have completed the Market Data Agreement, create a Google Drive folder containing your public key file(s) and completed document, then email [data@polymarket.us](mailto:data@polymarket.us) with your name or the name of your firm and a Google Drive link to your folder (grant Editor access to [data@polymarket.us](mailto:data@polymarket.us)).

For read-only data access, request these scopes:

| Scope | Description |
| - | - |
| `read:marketdata` | BBO (best bid/offer) and streaming market data |
| `read:l2marketdata` | L2 orderbook depth |
| `read:instruments` | Instrument listings and metadata |

## Step 3: Receive Your Credentials

The Polymarket team will review your submission and provide your Client ID credentials via email for both pre-production and production environments.


This documentation is built and hosted on [Mintlify](https://mintlify.com), a developer documentation platform.