# Phoenix vault product adapter plan

Status: isolated product adapter in progress, 2026-09-27. No live workspace has
been migrated. The running product release is unchanged.

Phoenix continues to own editor, graph, Reader, and UI meaning. The Kammi Library
service owns vault byte identity, durable journal commits, replay, and package
transport. The adapter will use only its owner-scoped HTTP client with a Phoenix
service credential. It will never open Ladybug or use a Library administrator
credential.

## Current product seams

- `WorkspaceDocument::save_atomic` commits the workspace tree to `workspace.json`.
  Note entry IDs survive rename; the workspace revision is separate from each
  document revision.
- `commit_document` publishes a revision-bound `.phxdoc` and returns a
  `DocumentLease` with entry ID, revision, BLAKE3 content hash, and UTF-8 text.
  Phoenix permits up to 16 MiB of note text.
- `ScenePublicationStore::publish` writes immutable `.psa` and `.pspi` files,
  then atomically replaces `current.pspm`. The kernel also verifies compiler
  authority receipts for full scenes. The current example full archive exceeds
  the Library's 10 MiB small-asset limit and needs streamed upload.
- Reader exact resume is audio-artifact/frame bound. Its narration plan has
  source byte ranges. The vault locator can name a source revision and a
  conservative source byte offset, while Phoenix keeps its richer resume data.
  Selection audition must not overwrite the book locator.

## Accepted Library interface decisions

1. **Source size.** `POST /v1/vaults/source-stream` accepts exact raw source
   bytes through 16 MiB; the SDK provides `vault_commit_source_file` and a
   verified source-byte download. This preserves Phoenix's note limit.
2. **SDK and credential.** The accepted `kammi-client-0.1.0.crate` is pinned by
   `vendor/SDK-HANDOFF-v1.json`; its SHA-256 is
   `95d049d2dc3f4809750043ce3a2387ae8941286a2369ec061a8bb5b5993c212f`.
   A per-installation Phoenix service actor and secret remain to be provisioned
   by the Library administrator; this product tree never stores the raw token.
3. **Cutover authority.** Library specifies `LEGACY_MIRROR` with a durable
   ordered outbox first. Only after drain, reconciliation, independent cold-open,
   and a bound cutover receipt can the one-way `product-primary` selector make
   Library authoritative. Divergence stops for explicit repair.

## Implementation gates

### 0. Isolated adapter and identities

- Add a product-owned adapter behind an explicit opt-in configuration. Default
  remains disabled for existing workspaces, including the running product app.
- Generate a vault ID once; derive stable source IDs from `EntryId`, never from
  note names or paths. Represent the workspace tree as its own source so rename,
  hierarchy, and active-entry state are portable.
- Keep the actor credential in the system credential store. Never log it or
  include it in workspace, package, or diagnostic output. Validate loopback or
  authenticated HTTPS, and reject ambiguous or missing actor configuration.
- All service I/O runs off the GPUI thread and uses bounded work. UI remains
  responsive when the service is slow or unavailable.

**Close:** deterministic IDs; disabled mode makes no service calls; invalid
configuration fails without changing local files; no secret appears in logs.

### 1. Source commit and crash recovery

- Hook only after the existing product save has passed its semantic checks.
  Bind the exact saved UTF-8 bytes and Phoenix revision to the Library source
  revision returned by an optimistic `base_revision` commit. Use a stable
  request ID per attempted commit so retry cannot create a second revision.
- At start and after service failure, compare the Library source view with the
  saved `DocumentLease` and workspace tree. Reconcile only under the approved
  cutover rule. A mismatch remains visible and blocks vault-dependent actions;
  it must never overwrite either side silently.
- Avoid a second local custody journal. Any minimal retry state is product
  synchronization state, not a competing source of authority.

**Close:** create, edit, rename, stale concurrent edit, service timeout, and
crash-at-each-boundary tests preserve bytes and converge through a documented
recovery path. The application never claims a Library commit from a local save.

### 2. Verified scene publication

- Observe the existing kernel's *verified, published* scene receipt; never
  stage an unverified candidate or infer authority from file presence.
- Stream the exact current `.psa` and `.pspi` and stage the needed compiler
  authority files. Build an explicit bounded asset inventory and stage the
  Library's canonical generation manifest. Select against the exact source
  epoch returned by the Library, after confirming the source revision still
  matches the scene's document binding.
- If source epoch advances during upload, leave staged assets unselected and
  report a stale generation. Preserve Phoenix's existing scene publication.

**Close:** scene IDs and bytes survive restart; stale publication is rejected;
the same published scene and sources yield the same selected asset inventory;
no graph truth or UI presentation state is rewritten by the adapter.

### 3. Reader locator

- Use the saved `DocumentLease` entry ID and revision, then choose a verified
  source byte boundary from the narration plan. Do not estimate a byte offset
  from audio-frame progress. Keep Phoenix's exact audio checkpoint separately.
- Commit on durable book position changes, not every playback tick; selection
  audition leaves the book locator unchanged. A source edit may leave a locator
  attached to the earlier immutable revision.

**Close:** pause, seek, bookmark, voice switch, document edit, selection
audition, and reopen preserve a valid locator without changing exact Phoenix
resume or cast behavior.

### 4. Cold-open product qualification

- Export through the Library client and keep the returned package root out of
  band. Import on a separate Library host, then materialize Phoenix files in a
  new workspace only after package verification and product semantic checks.
- Validate recovered workspace tree, note bytes, active scene and its authority
  closure, Reader locator, and any required voice/cast assets. Define and test
  an explicit rule for assets outside the vault package; do not imply full
  product portability from the Library's source/scene/locator test alone.
- Tampered package or root fails before changing the destination workspace.

**Close:** clean-host open, edit, graph, Reader resume, repeat import, crash
recovery, and tamper rejection pass with exact release identity and raw logs.

## Product release boundary

Do not point the running Phoenix app at the vault until gates 0-3 pass on a
separate fixture workspace and the interface decisions above are resolved.
Qualify the packaged binary and cold-open path before changing the pinned
product launcher. Keep unrelated renderer work and science trees untouched.

## Implementation checkpoint

An isolated `phoenix-vault-adapter` crate now snapshots saved workspace and
document bytes, gives notes stable source IDs, compares CAS identities, uses the
accepted `kammi-client`, and records ordered immutable mirror items. Its outbox
retries the exact request ID and base revision after a lost response and retains
queued bytes on invalid receipts. This is fixture-qualified product code only.
The kernel has not been wired to enqueue every commit, no service actor has been
provisioned for this fixture, and no live workspace or scene has been migrated.
