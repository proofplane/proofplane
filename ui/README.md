# Proofplane UI

Minimal Vite React app for the self-onboarding UI.

## Commands

```sh
npm install
npm run dev
npm run build
npm test
npm run test:smoke
```

The UI runs separately from the Rust API. By default it calls
`http://127.0.0.1:3000`, matching `config/local.yaml`.

## Environment

Copy `.env.example` to `.env.local` to override local values.
