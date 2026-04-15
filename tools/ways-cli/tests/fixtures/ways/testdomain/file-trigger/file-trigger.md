---
description: environment configuration dotenv secrets
vocabulary: env config dotenv secrets environment variable
files: \.env$
scope: agent
curve:
  type: Exponential
  half_life: 30000
---
# Environment Config

Never commit .env files. Use .env.example for templates.
