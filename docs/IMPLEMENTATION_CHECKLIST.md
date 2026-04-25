# AYX Implementation Checklist

## Foundation
- [x] Create Rust workspace and core crates.
- [x] Establish CLI command tree and global output envelope.
- [x] Implement `config.yaml` parser + baseline secret/auth fields.

## Mongo First-Class
- [x] Implement embedded mode discovery from RuntimeSettings.xml.
- [x] Implement managed external mode shape (URI + host/port/auth/TLS settings).
- [ ] Implement live AlteryxGallery and AlteryxService DB clients.
- [x] Implement dry-run/apply safety gates and mutation audits.
- [x] Add inventory + backup/restore orchestration commands.

## API Priority Slice
- [x] Implement API connection/auth client (PAT + OAuth2 client credentials).
- [x] Implement users/workflows read commands.
- [x] Implement workflow owner transfer via API endpoint.

## Quality and Release
- [x] Add unit test suites for Mongo/API core behaviors.
- [ ] Add containerized integration tests.
- [ ] Add Windows/Linux/macOS CI release pipeline.
- [x] Generate docs/cli-spec.md and docs/cli-schema.json.

## Workflow Cloud Conversion
- [x] Add native `workflow convert-cloud` support for Desktop `.yxmd` workflows.
- [x] Reuse the shared workflow XML model for Desktop-to-Cloud conversion and macro/schema handling.
- [x] Add smoke coverage for the CLI command and the converter library entry point.
