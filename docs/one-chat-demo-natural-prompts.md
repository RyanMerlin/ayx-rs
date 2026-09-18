# One-Chat Demo — Natural-Language Prompts

Paste-ready, one-liner prompts for recording the ayx-one-ops demo. Invoke the
[`ayx-one-ops`](../skills/ayx-one-ops/SKILL.md) skill once at the start of the session — these prompts assume
it's already loaded and enforcing the safety rules, narration, and command translation. They deliberately say
nothing about CLI syntax, flags, or IDs; that's the whole point.

Run them in order, in one continuous session, with a pause between each for recording. Give explicit approval
whenever the skill stops for a checkpoint before a mutation.

1. **Governance baseline** — "Give me a full picture of who's in this workspace — members, admins, groups,
   roles — and confirm we're authenticated against the right workspace."

2. **Onboarding** — "Onboard ryanmerlin@gmail.com and show me what access that gives them."

3. **Connections & backing** — "Show me our workspace's cloud backing and guardrails, and how healthy our
   BigQuery connection is and who can touch it."

4. **Workflows** — "Walk me through our workflow portfolio, then spin up a quick disposable copy so I can see
   how workflow inspection works. Leave it up — I want to look at it in the UI."

5. **Schedules** — "Show me our schedules, then set up a test daily schedule on that workflow so I can see it.
   Make sure it's disabled so it won't actually run."

6. **Sharing & plans** — "Who could I share this workflow with, and what does our plan orchestration look like
   end to end? Go ahead and create the test plan too, if we have a fixture for it."

7. **Wrap-up** — "The demo is complete. Tear down every disposable resource we created and confirm we're back
   to where we started."

If a segment needs a retake, re-run only that prompt — the skill re-resolves IDs live each time, so nothing
earlier needs to be repeated. Only run prompt 7 once, at the very end, after every other segment (and any UI
walkthrough) is done.
