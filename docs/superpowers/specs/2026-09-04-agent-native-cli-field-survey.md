# Field survey: agent-native CLIs, embedded MCP, and bundled TUIs (2025–2026)

**Date:** 2026-09-04
**Purpose:** The external evidence behind ADR 0004, ADR 0005, and
`docs/roadmap/agent-first-substrate.md`. Kept so the reasoning is auditable and
so future design work starts from citations rather than memory. Claims marked
*unverified* could not be confirmed against a primary source and must not be
repeated as fact.

## 1. Alteryx's own agent surface

- **Agent Studio and the Alteryx MCP Server** were announced together at
  Inspire 2026 (2026-05-20), both in Preview, GA "anticipated later in 2026."
  Agent Studio is the authoring/governance layer (which datasets, workflows,
  and apps are AI-accessible); the MCP Server gives external hosts (Claude,
  ChatGPT, Gemini, Slack, Teams) permission-scoped access to those assets.
  Scope is **Alteryx One only**; no source mentions on-prem Server. Requires
  One Professional/Enterprise. Sources: alteryx.com blog
  "Governing AI and Analytics at Scale with New Advancements in Alteryx One";
  help.alteryx.com/aac/en/agent-studio.html; TechTarget and Enterprise DNA
  Inspire 2026 coverage. Note: one independent search pass did not find this
  announcement at all, which suggests thin discoverability; confirm details
  through internal channels before relying on them.
- **Unofficial wrappers** (both by one author, both disclaiming official
  status): `jupiterbak/AYX-MCP-Wrapper` targets Alteryx Server (collections,
  workflow execution, users, schedules, credentials, DCM; stdio/SSE/HTTP;
  OAuth2 client credentials; 13 stars). `jupiterbak/OpenOne` targets Analytics
  Cloud (25 tools; stdio; 1 star). Neither does cross-workspace governance or
  access audit.
- **No vendor admin CLI exists.** The developer help documents the One API,
  the Server API, and an "AYX Plugin CLI" for SDK development. The One admin
  console is GUI-only; Alteryx's docs recommend curl/Postman for API use.
  AiDIN Copilot (GA 2024) is canvas authoring, unrelated to administration.
- **Net white space for `ayx`:** on-prem Server, cross-workspace governance,
  access audit, CI/offline scripting, and unified One+Server administration.
- *Unverified:* the MCP Server's GA date and auth flow (OAuth type, token
  lifetime).

## 2. How leading CLIs embed MCP

- **The GA pattern is a separate, hosted, OAuth remote server, not a CLI
  subcommand:** Stripe (`mcp.stripe.com`), GitHub (remote server GA
  2025-09-04, OAuth 2.1 + PKCE), Datadog (`mcp.datadoghq.com`), Atlassian Rovo
  MCP (GA 2026-02-04), Sentry (`mcp.sentry.dev`; local stdio flagged
  work-in-progress), Vercel (moved SSE → Streamable HTTP and roughly halved
  CPU; vercel.com/blog/building-efficient-mcp-servers).
- **Local servers that reuse CLI auth:** Azure's `azmcp server start` defaults
  to *namespace mode*, one routing tool per service, because "VS Code only
  supports a maximum of 128 tools across all registered MCP servers"
  (Azure/azure-mcp docs); `--mode all|single` are opt-in. Salesforce's
  `@salesforce/mcp` wraps `sf` with 60+ tools in 15 selectively enabled
  toolsets. Grafana ships `mcp-grafana` with disableable categories "to save
  context window" and, separately, the agent-first `gcx` CLI (§3): the clearest
  precedent for "CLI and MCP as two thin surfaces, not one absorbing the
  other." GitHub keeps `gh` and `github-mcp-server` as separate binaries.
- **Meta-tool collapse:** Twilio's hosted server exposes two tools
  (`twilio__search`, `twilio__retrieve`) over 1,800+ endpoints. Stripe's core
  is four generic tools (`stripe_api_search/details/read/write`) plus a few
  domain tools. Cloudflare **Code Mode MCP** (2026-02-20) is the extreme: two
  tools (`search()` over the OpenAPI spec, `execute()` running JS against it)
  covering 2,500+ endpoints in "roughly 1,000 tokens" versus "1.17 million
  tokens" for one tool per endpoint, per Cloudflare's own post. (Figures of
  32%/81% sometimes attributed to Code Mode do not appear in either Cloudflare
  post; do not cite them.)
- **Tool-bloat costs, best-sourced numbers:** Simon Willison measured the
  GitHub MCP server at 93 tools ≈ 55,000 tokens before any work
  (simonwillison.net/2025/Aug/22/too-many-mcps). Anthropic's Tool Search cut
  50+ MCP tools from ~72K to ~8.7K tokens with accuracy *improving* (Opus 4:
  49% → 74%); its programmatic tool calling cut 43,588 → 27,297 tokens
  (anthropic.com/engineering/advanced-tool-use, 2025-11-24). The
  widely-repeated "agents fail around 107 tools" traces only to aggregator
  blogs; *unverified*, treat as folklore.
- **`rmcp` (official Rust SDK):** v3.2.0 (2026-08-31), implements the
  2026-07-28 spec, stdio (`TokioChildProcess`) and Streamable HTTP
  (`StreamableHttpService`, typically over axum), `schemars 1.0` default
  feature so `JsonSchema` derives yield tool schemas. ~5M monthly downloads.
  No flagship-scale Rust CLI using it was found; `ayx` would be an early
  adopter of a mature SDK.
- **Generic "wrap any CLI as MCP" bridges** (`cli-mcp-server`, `mcp2cli`) are
  hobby-tier and security-sensitive (arbitrary shell execution).

## 3. Agent-native CLI design patterns

- **Grafana `gcx`** (Go, Apache-2.0, GA 2026-07-28, github.com/grafana/gcx),
  rebuilt because engineers "watched the agents retry commands that had
  already failed, confidently reach for flags that didn't exist instead of
  reading the help, and flood their own context windows with output"
  (grafana.com/blog, 2026-04-28). Decisions: `-o json|yaml` with field names
  stable across versions; a documented exit-code and error-shape taxonomy
  (e.g., exit 6 = version incompatible) plus `--on-error abort|fail|ignore`;
  auto-detection of Claude Code/Cursor callers that drops spinners and
  human-friendly truncation; a machine-readable catalog of its own commands
  and flags; `--dry-run` on push/delete with no silent destructive defaults;
  `gcx agent skills list` bundling SKILL.md-style skills in the binary.
- **Arcjet** (blog.arcjet.com/designing-a-cli-for-ai-agents, 2026-06-02):
  exit codes 0–4 for success/general/auth/validation/confirmation-needed; JSON
  errors carrying a code plus a remediation string; local input validation
  before any network call to catch hallucinated ids before side effects.
- **Algolia** (algolia.com/blog/engineering, 2026-05-07): `algolia describe`
  and `algolia schema` for zero-auth introspection; `--output ndjson` for
  streaming; rejection of control characters in input (LLMs generate them);
  OAuth PKCE (`algolia auth login`) instead of privileged admin keys so an agent
  inherits only the invoking user's permissions with attributable audit.
- **Agent Skills spec** (agentskills.io/specification): `SKILL.md` with `name`
  (≤64 chars, matches directory) and `description` (≤1024 chars), optional
  `allowed-tools`, `scripts/`, `references/`; three-tier progressive disclosure
  (~100 tokens at startup, body <5,000 tokens on activation, references on
  demand). GitHub CLI shipped `gh skill` in v2.90.0 (2026-04-16) writing into
  `.claude/skills/`, `.agents/skills/`, and `.github/skills/` simultaneously.
  AGENTS.md is the repo-root convention; llms.txt is for docs sites.
- **MCP vs CLI:** Willison's framing is that a CLI needs zero explanation to
  try ("the agent will try $randomcrap on the first call, the cli will present
  the help menu"), while MCP's token cost is measured (§2) but MCP retains
  value for controlled access agents should not get via raw shell and for
  centralized distribution. Convergence: a CLI with stable `--json` fields,
  discoverable help, and documented exit codes *is* the low-overhead agent
  interface and can be wrapped by MCP later; MCP-first bakes in tool-call
  assumptions that are hard to unwind.
- **Agent detection:** no ratified env var. `CI=true` is closest to universal;
  Vercel's `AI_AGENT` (`@vercel/detect-agent`) is unratified; TTY detection is
  the reliable fallback.

## 4. TUIs bundled with admin CLIs

- **None of the vendor admin CLIs checked bundle a TUI:** Supabase CLI, flyctl
  (`fly dashboard` opens the *web* console), doctl, Stripe CLI, `gh` (`gh-dash`
  is a separate 12.4k-star extension). k9s (34k stars) and lazygit (~75k) are
  deliberately separate from kubectl and git. Genuine bundled exceptions are
  narrow: atuin (TUI is one mode) and oha (the TUI *is* the output).
- **2026 discourse leans against new TUIs.** "Stop Making TUIs" (sockpuppet.org,
  2026-08-20) argues they route around obsolete terminal constraints and that
  AI-assisted coding makes native/web apps cheaper than fighting a terminal.
  Counterpoint: Hatchet built a k9s-like run-observability TUI in about two
  days with Bubble Tea and it got adoption; that was a *new* observability
  surface built cheaply, not a retrofit onto an admin command tree. ratatui's
  own discussion #1321 documents recurring friction from immediate-mode
  rendering forcing the app to own all cursor/scroll/selection state.
  *Unverified:* any maintainer's quantified TUI maintenance cost.
- **Access-audit tooling is tables and queries, not TUIs:** `rakkess` renders a
  static resource × verb matrix; `kubectl-who-can` has been dormant since
  2022-02. **Steampipe** is the standout model: a Postgres FDW over 100+ gRPC
  plugins (2,000+ tables) so governance questions become SQL joins; it has a
  working Okta plugin, proving the SaaS/IdP governance fit; no Alteryx plugin
  exists. CloudQuery's own split: Steampipe for ad-hoc investigation,
  CloudQuery for ongoing governance programs (cloudquery.io/blog, 2026-02).
  Powerpipe layers 5,000+ CIS/NIST/PCI controls on top as web dashboards.

## 5. Ideas worth stealing (mapped to waves)

| Idea | Source | Wave |
| --- | --- | --- |
| Caller auto-detection; drop spinners and truncation for agents | gcx | 0 |
| Exit-code + structured-error taxonomy with `remediation` and retryability | gcx, Arcjet | 0 (taxonomy exists; add fields) |
| Machine-readable catalog of own commands | gcx, `aria discover` | exists (`discover`, `catalog`) |
| Route "visualize this" to the existing web UI | flyctl | 0 (`one open`) |
| In-binary jq | (common request; `jaq`) | 0 |
| Access matrix view | rakkess | 1 |
| "Explain this permission" | GCP Policy Troubleshooter, AWS IAM Access Analyzer | 1 |
| Snapshot + diff for drift | `kubectl diff`, `pulumi preview` | 1 |
| Resource graph export in DOT | `terraform graph` | 1 |
| Cross-tool skill install protocol; three-tier disclosure | `gh skill`, Agent Skills spec | 2 |
| Meta-tool MCP collapse | Cloudflare Code Mode, Twilio, Stripe | 2 |
| Namespace mode as a tool-count knob | Azure MCP | 2 |
| OAuth PKCE over static admin keys | Algolia | exists (`--browser`), extend |
| Plan/apply with saved plan + idempotency key | Terraform, Pulumi, Stripe | 3 |
| Policy-as-code over exported inventory | OPA/Conftest | 3 |
| SQL over API | Steampipe | deferred (export instead) |

## Still unverified after two research passes

- Alteryx MCP Server GA date and auth flow.
- Any vendor's *written* rationale for keeping MCP servers as separate
  binaries from their CLIs (strong circumstantial pattern only).
- Quantified TUI maintenance cost from any maintainer.
- Okta/Entra terminal access-review tooling.
- The "~107 tools" agent-failure threshold.
