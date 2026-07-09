# Methodology Doctrine

Cross-cutting project-level principles for **how to make design decisions** in this codebase. Not about how individual layers work (see `infra-doctrine.md`, `backend-doctrine.md`, etc.) — about the patterns that prevent specific recurring classes of bug.

Each principle below was earned by a real bug, surface, or refactor — not invented prophylactically. If a principle isn't preventing a class of bug we've actually seen, it doesn't belong here.

---

## §1. Polysemous labels are bugs

**Principle.** When a bucket name, field name, enum value, or env-var name is doing wrong work — carrying two meanings — the fix is to split the name, not to overload it. Resolve every conflation by introducing a new label for the second meaning; never normalize ignoring "in this case it also means X."

**The signal.** "We say X means A, but in this case it also means B." If you find yourself writing that sentence (or thinking it during code review), the label is polysemous and is the bug.

**Why it matters.** Polysemous labels are invisible at the unit level — each callsite uses the label with one of its two meanings, which is locally correct. The conflation only surfaces when both callsites end up in the same conversation or audit. Until then, the system "works" by coincidence.

**Worked example — `orphan` bucket in `infra/scripts/dev/pull-secrets.sh`.** The spec's original five-bucket model used `orphan` to mean "in convex env, not in manifest" — designed to warn about stray manual `convex env set` invocations (rare). Under Spec 1's zero-manifest-entries world, every convex env key would have classified as `orphan` — 40 entries per run. The bucket's signal-to-noise inverted: it had become "stray AND audit-pending" carried by one name. The fix was to add a sixth bucket `audit_pending` and have the script suppress audit-pending keys from `orphan`. Two meanings, two labels. See `infra-doctrine §15.22` v2.40.0.

**Worked example — `CONVEX_WEBHOOK_SECRET` / `EMAIL_WEBHOOK_SECRET` (tbd `rnk-5f1a`).** The Cloudflare Worker (`apps/email-ingest/src/index.ts`) and Convex (`apps/clawcraft/convex/http.ts`) each named the same shared bearer token differently — the Worker called it "the secret to call Convex," the Convex side called it "the secret authenticating the email webhook." Each name was self-consistent in its own commit (11 minutes apart, same author). The cross-component contract — *both sides must read the same env-var name* — was un-named. Fix: unify on `EMAIL_WEBHOOK_SECRET` (matches the inventory side); rotate the Worker's secret + redeploy.

**Worked example — `status` vs `linqStatus` at the Linq boundary (add-linq).** A `linq_numbers` row has two distinct state machines that meet at the external boundary: OUR provisioning workflow (`pending → registering → active`) and the UPSTREAM partner's number health (`ACTIVE`/`FLAGGED`/`AT_RISK`/`CRITICAL`). Naming both `status` would overload one label with two owners' meanings. Fix: `status` is ours; `linqStatus` (origin-prefixed) is the partner's echo. Reject the bland `provisioningStatus`/`upstreamStatus` pair — the column is always read as `linq_numbers.status`, so column-scope already disambiguates local readers, while the partner-name prefix documents *where the second meaning comes from*.

**Worked example — `clawcraft_account_id` is an externalId, not a UUID (tbd `rnk-m223`, commit `93116fc`).** The Stripe checkout metadata field `clawcraft_account_id` is *named* like an account identifier, so treasury's `stripe-task.ts` read it as one — an `accounts.id` (a Postgres UUID) — and fed it straight to `eq(accounts.id, …)`. But the clawcraft app stamps it from `user.treasuryExternalId`, which `auth.ts` documents as "IS the Convex user _id" — an **externalId**, not the ledger's UUID. A Convex id is not valid UUID syntax, so every real subscription/pack payment threw `invalid input syntax for type uuid` → 500 → Cloud Tasks retried forever → **paying users got zero credits**. The name asserted a value-shape (`id`-of-the-ledger-account = UUID) the value never carried (externalId). Fix: `resolveUserAccountUuid` matches by `externalId` first (the real producer's value), UUID fallback guarded by a regex so a non-UUID can never reach the id column again. **The corollary that makes this a methodology bug, not just a treasury bug:** the integration test *constructed its own account and stamped `account.id` (the UUID)* into the metadata — it fed the consumer the shape the field name implied, which is exactly the value the ledger primitive wanted. So the test passed by the same coincidence that hid the polysemy, while production (which stamps the externalId) failed. **A test that constructs a contract's value itself will pick the shape convenient for the code under test and thereby validate a fiction; the test must carry the value the real *producer* emits.** Here: stamp the externalId, as the app does — see the `resolves clawcraft_account_id as an EXTERNAL id` regression test. Only a live prod-test-mode e2e (Phase 6.4) caught it; 90 treasury + 21 Convex unit/integration tests were all green.

**Counter-pattern.** Living with the conflation. "It's fine, you just have to remember that orphan also covers audit-pending in this case." That's a doctrine debt; pay it down by splitting.

**How to apply.** During design review, whenever you can't write the bucket/field/enum's definition in one sentence without an "and" or "except when," the label is polysemous. Split before merging.

**See also.** §3 (`transitional schema discipline` — sometimes the second meaning IS a transitional state, which earns a transitional label rather than overloading a permanent one). §4 (`inventory-as-debugging-tool` — inventories systematically surface polysemous labels).

---

## §2. No aspirational config

**Principle.** A config entry — env var, schema field, manifest entry, feature flag, doctrine reference — is a contract that the referenced state already exists. If the upstream secret, table, service, flag, or row is not provisioned, the entry does not get added. Provisioning and the entry land in the same commit, in that order.

**The signal.** "We'll add the manifest entry now; Spec 3 will create the GCP secret later." Or: "Let's reserve the schema field; we'll wire the consumer in the follow-up PR." Both versions of that sentence are the bug.

**Why it matters.** Aspirational entries fail at runtime, normalize ignoring config errors ("oh that one's not provisioned yet, look past it"), and erode trust in the inventory. Once devs start ignoring entries, the inventory stops being a source of truth and becomes a wishlist; the cost of every subsequent classification doubles.

**Worked example — `dev-secrets-manager` Spec 1 manifest-authority seed entries.** The spec's Phase 1.2 prescribed seeding 1–3 `authority: manifest` entries (`ENCRYPTION_KEY`, `CONVEX_WEBHOOK_SECRET`, etc.) referencing `clawcraft-dev-*` GCP secrets. At execution time, those GCP secrets did not exist. Creating them by copy-from-Convex-env would have been provenance laundering (the new "GCP-managed" secret would have an unknown lineage). Resolution: ship zero manifest-authority entries in Spec 1; defer provisioning + entry to Spec 3 (`secrets-migrate`), one secret per commit, in the three-step contract codified in `infra-doctrine §15.22`.

**Worked example — `bootstrap-projection.sh` cloud URL written for local Convex deployment.** Pre-fix, the script unconditionally wrote `https://<name>.convex.site/treasury/projection` into `apps/treasury/.env.local` for every non-prod deployment. For `CONVEX_DEPLOYMENT=local:*` (Convex's local-backend mode), `*.convex.site` does not route to the in-process backend — HTTP actions are served on `http://127.0.0.1:3211`. The cloud-shape URL was aspirational: the script wrote a string referencing no listener for the deployment type currently in use. Symptom: every treasury → Convex projection push 404'd silently; the only visible signal was the 422-gap retry path that landed via the v1.4.0 `treasury-doctrine §4.7` contract, days after the divergence began. The bootstrap script is a config-shaped entry point — it has the same contract as a manifest. Resolution: branch on deployment type so every URL the script emits is verified-reachable for that type at write time. See commit `0b4062a`.

**Counter-pattern.** "Stub the entry; fill in the value at deploy." The stub is the bug.

**How to apply.** Before adding any config-shaped entry, grep the upstream system for the referenced state. If absent: pause the entry until the state is provisioned. The cost of pausing is at most one PR; the cost of an aspirational entry compounds with every subsequent reader who has to remember the carve-out.

**See also.** `infra-doctrine §15.22` three-step promotion contract. Memory `feedback_secrets_management_split.md` for the rotate-on-migrate (never copy-on-migrate) corollary.

---

## §3. Transitional schema discipline

**Principle.** Some schema additions exist only for a migration window. They earn their place by the same rule as permanent fields — a real consumer in code — but they ship with a pinned end state and a pinned removal commit. Three required properties for transitional schema:

1. **Real consumer in code.** The script, query, or function reads the field today. No reserved-for-future-use fields.
2. **Named end state.** The condition under which the field becomes empty / unused is explicitly written. ("`audit_pending:` is empty.")
3. **Pinned removal commit.** The follow-up PR or spec that removes the field is referenced in advance ("removed in Spec 3's final commit").

If any of the three is missing, it is not a transitional field — it is a permanent field hiding behind transitional language.

**The signal.** "We'll clean this up later." Without "this" naming the end state and "later" naming the commit, that sentence is the bug.

**Worked example — `audit_pending:` in `infra/secrets-manifest.yaml`.** Added in Spec 1 to enumerate the 40 existing Convex env keys awaiting classification. Real consumer: `pull-secrets.sh` reads it for the sixth-bucket suppression. End state: empty (every key classified by Spec 2 and promoted by Spec 3). Removal commit: Spec 3's final commit also removes the `audit_pending:` field from the schema. All three properties met. See `infra-doctrine §15.22` v2.40.0 for the specific instance; this section is the general pattern.

**Counter-pattern.** Adding a `scope:`, `canonical_store:`, or `requires:` field "for future use" with no consumer. The Spec 1 brief originally proposed these; they were deferred because no script read them today. Permanent schema commitments hiding as transitional.

**How to apply.** Before adding any schema field, write the three properties as a comment in the schema header next to the field. If any property cannot be filled in, the field is not ready to land.

**See also.** §1 (polysemous labels — transitional schema lets you split a meaning during the migration window rather than overload a permanent name).

---

## §4. Inventory-as-debugging-tool

**Principle.** When you build an inventory of a system you thought you understood, expect to find bugs. The act of naming every entity surfaces conflations, naming drifts, dead pairs, and silent assumptions that were invisible at the unit level. **Build the inventory first**, before any non-trivial system change — it is the cheapest debugging tool available.

**The signal.** "I assume the system is in state X." Whenever you assume rather than enumerate, an inventory will surprise you.

**Why it matters.** The most expensive bugs in a project are the ones that "work by coincidence" — every callsite is locally correct, but the cross-component contract has silently drifted. Unit tests can't catch these; only an inventory that puts the contract on a single page can.

**Worked example — `rnk-5f1a` naming-drift fix.** While building the `audit_pending:` block in `infra/secrets-manifest.yaml` (Spec 1 inventory of every Convex env key), executor noticed `EMAIL_WEBHOOK_SECRET` was the canonical Convex-side name but the deployed Cloudflare Worker used `CONVEX_WEBHOOK_SECRET`. The system "worked" because deploy-time both sides happened to be set to the same value. Without the inventory, this bug was invisible — both source files were self-consistent. The inventory put the two names on one page and the drift was instantly obvious. Bug filed, RCA-ed, fixed mid-execution.

**Worked example — `backend-doctrine §16` drift.** Spec 1's inventory revealed that the Convex env had 43 keys but §16 listed only 15. 25+ env vars had been added by subsequent feature work without §16 updates. The bug had been compounding silently for months — no test failed because no test cared about §16's completeness. The inventory was the first surfacing.

**Counter-pattern.** "I'll just modify the system; I roughly know the entities." That assumption is the bug. Inventory first; modification second.

**How to apply.** Any non-trivial change touching a multi-component contract (shared env vars, shared schemas, cross-service auth, etc.) starts with: enumerate every callsite, every reader, every writer on one page. The cost is at most one grep + one markdown table. The savings is whatever inventory-finds-bug instance would have shipped as a coincidence.

**Corollary — start each spec with the inventory step.** The `dev-secrets-manager` spec did this implicitly (its `audit_pending:` block is the inventory). The pattern: an early phase that runs the existing system, enumerates the entities, and surfaces the surprises before any modification phase.

**See also.** §1 (the bugs inventories surface are typically polysemous labels). §2 (inventories also surface aspirational config — entries pointing at state that doesn't exist).

---

## §5. Pin external contracts to the source; validate to the partner's guarantee

**Principle.** When our code consumes an external system's contract — wire-format field names, envelope shapes, identity field value-shapes — we MUST pin them to the partner's **authoritative source** (their schema docs, by URL) and validate them to the **partner's** guarantee, never to a paraphrase or to our narrower convenience. Two facets:

1. **Names/shapes are pinned, not paraphrased.** A brief that describes a partner's wire format in prose ("text concatenation for `type: text`") instead of quoting the partner's actual schema will ship the wrong field name. Pin by URL; ideally force-paste a real JSON sample.
2. **Counterparty identity is opaque.** An identity field the partner calls a "handle" is a handle (phone OR email OR opaque id). Validating it to a shape *we* assume (E.164) silently drops valid traffic. Validate only the fields that are *our* routing keys (the platform's own provisioned number) to the shape *we* control; treat counterparty identity as opaque.

**The signal.** "Linq sends `sender_phone`, so I'll validate it as a phone." Both halves are assumptions about the partner's contract sourced from our heads, not their docs.

**Why it matters.** Facet 1 fails loudly (a field-name mismatch breaks at deploy) — costly but self-correcting. Facet 2 fails **silently**: the over-validated field drops the message before any row is written, the webhook still 200s so the partner never retries, and there is no error visible to operator or sender. Strictly worse than a deploy failure — no feedback loop at all.

**Worked example — three paraphrased Linq field names (add-linq).** The brief paraphrased Linq's wire format instead of pinning `https://docs.linqapp.com/guides/webhooks/events/`. Three names were wrong, each costing a prod-deploy cycle to surface + fix: `sender_phone`→`from_phone` (`a075994`), `type`→`event_type` (`997944d`), `parts[].text`→`parts[].value` (`68e1d05`). *(The FMEA-predicted `signing_secret` cross-fork-removal failure — F-1 — never triggered; the sovereign fork accepted the block without it. A prediction is not a worked example; this doctrine records the bugs that actually happened.)*

**Worked example — E.164 over-validation dropped iMessage email-handle senders (add-linq).** `domain/linq-routing.ts:extractRoutingKey` ran the inbound `sender_handle.handle` through `validateE164Phone` (`^\+[1-9]\d{1,14}$`). iMessage senders registered to an Apple-ID **email** report an email in that field, not E.164 — so `marianamontesdeocar@icloud.com` was rejected, the event dropped before any row, the webhook 200'd, and every email-handle sender was silently unreachable while the number looked "active." Fix: accept any non-empty handle; rename `senderPhone`→`senderHandle` (it is a handle, not a phone); keep E.164 validation only on `owner_handle`/`fromPhone` (our own provisioned number — the routing key into `linq_numbers.by_from_phone`).

**Counter-pattern.** "Our use case has only seen phones, so `senderPhone` validated as E.164 is fine." The next channel carrying email-or-phone identities repeats the silent drop.

**How to apply.** A brief crossing an external/sovereign-fork boundary lists each consumed field with `field | partner_source_url | wire_value_example`. Validate our keys to our shape; treat their keys as opaque strings until a real sample proves a tighter shape is safe.

**See also.** §1 (`senderPhone`-for-a-handle is also a polysemy trap — the name asserts a shape the value doesn't guarantee). §4 (a `Cross-fork contract index` is an inventory; build it before writing the brief).

---

## Document History

| Version | Date | Changes |
|---|---|---|
| 1.0.0 | 2026-05-12 | Initial doctrine. §1 (polysemous-label principle) seeded from tbd `rnk-c86p`, surfaced by the `dev-secrets-manager` Spec 1 execution (orphan bucket split) and the `rnk-5f1a` naming-drift fix. |
| 1.1.0 | 2026-05-12 | §2 (no aspirational config) added from tbd `rnk-a5jw`. Earned by the `dev-secrets-manager` Spec 1 decision to ship zero `authority: manifest` entries; codifies the rotate-on-migrate / no-copy-on-migrate working memory as a general principle. |
| 1.2.0 | 2026-05-12 | §3 (transitional schema discipline) added from tbd `rnk-9mr4`. Generalizes the one-off paragraph in `infra-doctrine §15.22` v2.40.0; the `audit_pending:` block is the reference instance. |
| 1.3.0 | 2026-05-12 | §4 (inventory-as-debugging-tool) added from tbd `rnk-gkiw`. Earned by the `rnk-5f1a` naming-drift discovery during the `dev-secrets-manager` Spec 1 audit_pending inventory. Closes the four-principle initial seed of this doctrine. |
| 1.4.0 | 2026-05-16 | §2 worked example added for the `bootstrap-projection.sh` local-deployment URL fix (commit `0b4062a`). Second instance of the §2 class — a script silently writing config that referenced no running listener. Reinforces that URL/endpoint synthesis MUST branch on every deployment-pointer variant the system can produce; bootstrap scripts that derive endpoints from a deployment pointer carry the same aspirational-config contract as manifest entries. |
| 1.5.0 | 2026-05-31 | **§5 (pin external contracts to the source; validate to the partner's guarantee) added** + §1 worked example (`status`/`linqStatus` boundary polysemy). Merged from the queued add-linq doctrine amendments. §5 earned by four real add-linq bugs: three paraphrased wire-format field names (`sender_phone`/`type`/`parts[].text`) and one silent E.164 over-validation drop (`@icloud.com` iMessage senders). The originally-proposed F-1 `signing_secret` cross-fork-removal worked example was **dropped, not merged** — it was a prediction that never triggered, and this doctrine records only bugs that actually happened. |
| 1.6.0 | 2026-07-06 | **§1 worked example added** — `clawcraft_account_id`-is-an-externalId-not-a-UUID (`rnk-m223`, commit `93116fc`): a metadata field named like a ledger UUID actually carried the Convex externalId, so treasury's UUID lookup 500'd and real subscribers got zero credits. Adds the **test-fidelity corollary** to §1: a test that constructs a contract's value itself picks the shape convenient for the code under test (here the integration test stamped the UUID the field name implied) and validates a fiction — the test must carry the value the real *producer* emits. Caught only by the live Phase-6.4 e2e; all unit/integration tests were green. |
