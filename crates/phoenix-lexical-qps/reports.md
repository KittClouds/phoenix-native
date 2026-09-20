# Lexical Transport Reports (LT0–LT7)

Subject: `phoenix-native/crates/phoenix-lexical-qps` (QPS crate **source untouched**;
all experiments use public search APIs only).
Harness: `lt0-harness` (out-of-tree, `cargo run --release`).
Split discipline: **A** mines `R0`/`Psi` · **B** is dev · **C** is held-out eval.
`U(y|x)` is descriptive-only throughout; no eval feedback enters any table.

Frozen params: literal `q=1.0`, transported `q=0.35+0.5·s` capped `[0.35,0.85)`,
`R0` floor `s≥0.15`, max 4 expansions/group, `top_k=5` (pressure: pool cap 6, `top_k=3`).

## LT0 conclusion (carried forward)

Usage geometry contains replacement candidates (recall +2: `repair damaged engine`
3→4 via `faulty motor`, `car engine` 1→2 via `automobile`; `loss_exact=0`;
~3x posting fan-out). Slot geometry nominates but must not authorize
(`economic↔tumor slot=1.00`).

## LT1 arms

- `L0` literal only · `L1` frozen `R0` · `L2` `R0` + diversity gate (`m=3` of 7
  channels: slot>0.30, window>0.40, doc>0.08, field>0.60, left/right>0.30,
  containment>0.25) · `L3` `L2` + `C_pres(y|x,Q)≥0.20` (query-conditioned,
  abstain-neutral 0.5).

## Microscope (corpus A sidecar, vocab 77)

| pair | s | slot/win/doc | votes |
|---|---|---|---|
| damaged→broken | .79 | 0.95/0.82/0.17 | 7/7 |
| economic→tumor (bad) | .78 | 1.00/0.82/0.50 | 7/7 |
| car→automobile | .69 | 0.71/0.84/0.67 | 7/7 |
| repair→fix | .54 | 0.40/0.90/0.60 | 6/7 |
| engine→motor | .31 | 0.00/0.90/0.29 | 4/7 |
| bank→shore | .25 | 0.00/0.71/0.67 | 3/7 |

- `engine→motor` has **slot 0.00** yet transports on window 0.90: evidence
  vocabulary, not one similarity. A slot-only design would miss it.
- `C_pres(tumor|economic,"rapid economic growth")=1.00` — conditional windows
  coincide because anchors co-occur. `C_pres` as built **reinforces** the bad pair.
- `C_pres(fix|repair)=0.50` (`damaged`→0.0 sparse-data artifact, `engine`→1.0).
- L2 census over L1 table: pass 207 / kill 78.

## Dev (B) recall@5 / rows / pool

| query | L0 | L1 | L2 | L3 |
|---|---|---|---|---|
| repair damaged engine | 3 / 6 / 3 | 4 / 20 / 6 | 4 / 20 / 6 | 4 / 11 / 4 |
| car engine | 1 / 4 / 4 | 2 / 13 / 8 | 2 / 12 / 8 | 2 / 12 / 8 |
| bank erosion | 1 / 4 / 3 | 1 / 12 / 3 | 1 / 12 / 3 | 1 / 4 / 3 |
| bank collapse | 1 / 4 / 3 | 1 / 12 / 3 | 1 / 11 / 3 | 1 / 8 / 3 |
| rapid economic growth | 1 / 5 / 2 | 1* / 14 / 3 | 1 / 10 / 2 | 1 / 6 / 2 |

`*` L1 retrieves on-topic but adds noise doc 101 (`repair|rapid`,
`repair|growth`); L2 kills those expansions, pool 3→2. `loss_exact=0` everywhere.

L3 on q1 halves fan-out (rows 20→11, pool 6→4) with zero recall loss by
filtering spurious window-only nominations (`drive|repair`,
`automobile|damaged`); genuine winners (`fix|repair` .40→.50, `motor|engine`,
`faulty|damaged`) survive.

## Held-out (C)

- `car engine` 1→2 under L1/L2/L3: the gain **generalizes** past dev.
- `repair damaged engine`: L1/L2 add noise doc 203; L3 removes it (pool 3→2).
- `rapid economic growth`: L1 adds noise doc 202; L2/L3 clean.
- No regressions on any arm vs L0 anywhere.

## Pressure: eviction curve `E(P)=|B0\BT|`

`E(P)=0` at all strata P0..P4 (+0/+4/+8/+12/+16 distractors), both probes,
both L1/L3. BT pools saturate at the cap (6) and always superset B0.
Structural reason, not luck: expansions are strictly additive under a
pinned literal 1.0, so full-coverage literal docs outscore transport-only
distractors. **Risk concentrates on partial-coverage literals**, which this
stratum design never pressured (cap 6 > |B0|). Follow-up: cap ≤ literal
coverage count with distractors at full transported coverage.

## Mechanism resolution

- L2 suffices for the observed error class (kills `repair|rapid`-style damage).
- L3 adds fan-out reduction + spurious-nomination filtering, preserves all gains.
- Neither L2 nor L3 blocks `tumor|economic` itself (7/7 votes, `C_pres` 1.00);
  it is rank-harmless here because literal coverage dominates. The cliff edge
  stands, now precisely characterized: co-occurring anchors make conditional
  fingerprints coincide. Next authorization evidence must be sense-separating
  (document strata, entity/graph, rarity asymmetry) or a `U`-prior from dev B.
- Caveat: `U_proxy` is text-containment, not true winner identity
  (`capture_group_strengths_into` gives strengths, not term ids) — e.g. bank
  erosion L3 lists wins while tiers show `exp0`. Descriptive only.

## Carried forward (LT1)

> Usage geometry nominates; authorization needs diverse, sense-preserving evidence.

## LT2: contrastive residual context (sense separator)

Frozen: `R0`, L2, L3, `q`. New single channel `R_contrast`: leave-anchor-out
residual `Psi^{-z}(x)` = window distribution of `x` over occurrences whose
±8 neighborhood excludes anchor `z`; `S_res(x,y|z)` = cosine of residuals,
abstain-neutral when either side is empty. L4 = L3 + gate
`min_z S_res ≥ TAU_R` (`TAU_R=0.15`, a priori floor). Sidecar A widened 16→22
docs with sense basins (economic↔market/policy/trade/gdp/employment/inflation;
tumor↔cancer/cell/tissue/treatment/patient); B/C frozen.

Contract microscope (worst over anchors):

| pair | worst S_res | per-anchor | gate |
|---|---|---|---|
| repair→fix | 0.73 | -damaged 0.74, -engine 0.73 | PASS |
| engine→motor | 0.68 | -repair 0.77, -damaged 0.68 | PASS |
| damaged→faulty | 0.58 | -repair 0.58, -engine 0.59 | PASS |
| car→automobile | 0.88 | -engine 0.88 | PASS |
| economic→tumor | 0.00 | -rapid 0.00, -growth abstain | KILL |

Gap 0.58–0.88 vs 0.00: `TAU_R` sits in open space, no threshold tuning.
The kill is mechanistic: minus `rapid`, economic's basin
{policy, employment, trade, …} is fully disjoint from tumor's
{cells, treatment, …}. Note dilution alone was insufficient: with 22-doc A,
`R0s` economic→tumor fell 0.78→0.57 yet still passed L2 (7/7) and `C_pres` 1.00.

Dev B (L3→L4): `rapid economic growth` serves `economic:[economic, population]`
— tumor gone, rows 6→5, tiers back to L0 shape, recall 1. All other queries
byte-identical (q1 keeps rec 4). Held-out C: same kill generalizes
(L3 `exp1` → L4 `part1`, recall 1); zero regressions anywhere. `loss_exact=0`
throughout.

> Residual basins separate senses that shared frames conflate.

Next: (1) pressure rerun targeting partial-coverage literals, (2) `U`-prior
from dev B, (3) `q ∝ R0·U·C_pres·S_res` tested once on C.

## Side branch (sealed): phenotype census + anchor-adversary audit

A widened 16→22→34 docs with adversary basins (financial/medical crisis,
network/road traffic, cell/national culture). Probe corpus P kept separate;
B/C re-run as regression check (recall identical everywhere, `loss_exact=0`).

Phenotype signature = slot/window/doc bits + frame-overlap bit + residual bit
(`R` = min residual over shared anchors ≥ 0.15, `r` below, `.` abstain).
Labeled directed pairs (8 good, 7 bad; `cell→national` never nominated):

| family | good | bad | P(useful\|phenotype) |
|---|---|---|---|
| SWDAR | 6 | 0 | 1.00 |
| sWDAr | 2 (`engine↔motor`) | 0 | 1.00 |
| SWDAr | 0 | 2 (`economic↔tumor`) | 0.00 |
| SwdAr | 0 | 2 (financial↔medical) | 0.00 |
| SWdA. | 0 | 2 (`network↔road`) | 0.00 |
| swdA. | 0 | 1 (`national→cell`) | 0.00 |

Small N=15, but the split is exact: no family mixes good and bad. Two nuances:
(1) census-wide min is stopword-fragile — `engine→motor` carries a `-the: 0.00`
anchor (support 3/1); the serving path is immune because it aggregates only
query anchors. Residual evidence should carry support mass, not a raw min.
(2) `network→road` scores scalar 0.73 yet dies at diversity (doc-low) —
scalar alone would have served it.

Adversary audit (P, L3 vs L4): all three new pairs die at L2/L3
pre-residual (diversity or `C_pres`), so L3≡L4 on every probe and the audit
cannot discriminate the residual — it tests the earlier gates, which hold.
Genuine control `repair faulty motor` stays 2/2 on both arms. Lesson: future
adversaries must survive L3 to stress L4 (selection criterion); the tumor
class remains the only strong adversary observed, and its held-out kill
replicates. Nomination coverage limit noted: rare `cell→national` absent.

> Phenotypes separate; residual graduates contingent on support-weighted
> aggregation. Serving path unchanged: nominate broadly → authorize diversely
> → condition on query → remove shared-frame illusion → serve through QPS.

## LT3-RA: support-bearing residual aggregation (microscope only, no serving change)

Predeclared before seeing data: mass `m=min(nx,ny)`, alphas {.1,.2,.25},
`kmin=2`, frozen `TAU_R=0.15`. Serving untouched; microscope covers query-anchor
scope (serving semantics) and all-shared-anchor scope (global-table semantics).

Query scope: all 8 GOOD pass raw-min; BAD killed (economic/financial) or
abstain (network/road/cell — thin, never reach residual meaningfully).
Global scope: RA/RB(.1/.2/.25)/RC all PASS every GOOD (engine→motor
RA=0.74/RB20=0.68/RC=0.75 — stopword fragility gone); RA+RB KILL
economic/tumor/financial/medical (RA=0.0–0.08, RB=0.0). RC abstains on all four
(K empty — thin-support adversaries) → **RC rejected: fail-open**.

## LT3-AM: mined A4 stress set (40 pairs, labeled post-mining, no retuning)

Miner used frozen raw-min semantics only. Labels: 13 good / 26 bad / 1 mixed
(`bank→shore`: good under erosion, bad under collapse). Census-wide verdicts:

| agg | good kill/pass | bad kill/pass | P(bad\|KILL) | P(KILL\|bad) |
|---|---|---|---|---|
| RA (mean) | 10/3 | 15/11 | 0.60 | 0.58 |
| RB20 (quantile) | 11/2 | 26/0 | 0.70 | 1.00 |
| RC (kmin=2) | 4/9 | 4/22 | 0.50 | 0.15 |

No census-wide aggregator gates cleanly on thin corpus: RA kills 10/13 good,
RB20 perfect bad-recall at 11 good killed, RC fail-open. RB20 is the best
contradiction FLAG, not a gate. Reading: global-table residual needs mass per
anchor (corpus scale), not a cleverer collapse — the query-scoped L4 gate
(raw min over 1–3 query anchors) remains the validated form, because queries
select the anchors where contradiction concentrates. Mixed `bank→shore`:
RA pass / RB20 kill / RC pass — quantile is the conservative choice for
sense-ambiguous pairs, as predicted.

## LT4-MS: mass scaling (serving frozen; zero QPS calls)

Question: is global residual failure thin-data or wrong-abstraction? Scaled the
frozen 40-pair A4 set through S1/S2/S4/S8 (frame+basin docs) and T2/T4/T8
(basin-only, frame frozen), strata thin/low/mid/high over M=Σmin(nx,ny).
Three generator confounds were found and diagnosed — each is a finding:

1. S-series frame echo: frame docs inject shared L/R words into every
   residual; everything → PASS by S4. Residual-minus-z does not remove the
   frame, only z. S-curves discarded as separator evidence.
2. Mixed-field contamination (T-v1): doc11 puts alarms/doctors in BOTH basin
   lexicons (proven by DETAIL). Top-window lexicons inherit the conflation.
3. Cross-pair contamination: shared terms accumulate multi-pair basins, so
   pair-independence breaks at scale regardless.

Fix applied mid-branch: sense-pure basins (purge fields containing both
terms + framewords). T8 DETAIL bands with mass 30–100/anchor:

| pair | band | status |
|---|---|---|
| engine→motor | 0.64–0.83 | mass-backed PASS |
| financial→medical | 0.00–0.12 | mass-backed KILL (holds every variant to M~250) |
| vehicle→car | 0.00–0.19 | mass-backed KILL vs GOOD label |
| alarms→worries | 0.00–0.11 | mass-backed KILL vs GOOD label |
| economic→tumor | 0.19–0.25 | borderline; S1 thin kill unresolved at scale |
| bank→shore | 0.03–0.19 | mixed; RB kills (conservative) |

Verdict on the question: wrong dichotomy — binding constraint is support
PURITY (sense-pure, frame-free occurrences), not raw mass. Consequences:

- Residual is asymmetric: HIGH residual ⇒ shared usage (safe); LOW residual ⇒
  unknown (different senses OR thin usage, e.g. hapax-like vehicle). Kills
  need mass+purity; the abstention rule stands and should extend to thin
  zeros.
- Global-table collapse stays rejected for gating (RB kills label-good rare
  terms at mass). Query-scoped L4 + C_pres is where residual belongs: the
  query selects the anchors where contradiction concentrates.
- financial→medical is the study's strongest separator datum (kill survives
  every confound variant with mass). economic→tumor's S1 kill is corroborated
  by it as same-phenotype sibling, but its own mass-backed status is
  unresolved — flagged, not claimed.
- T8 pooled: good 8/12 RA kill, bad 10/24 — no global gate; RB remains a
  contradiction flag with P(bad\|KILL)≈0.6–0.7 on flagged pairs.

## LT5-SP: purity intervention (mass fixed N=24/side, serving frozen)

Frozen 4-pair set (engine→motor GOOD, financial→medical BAD, vehicle→car
GOOD-weak, economic→tumor BAD), purity p∈{1,.75,.5,.25}, three separately
injected types. RA shown (RB tracks it throughout — no RA/RB divergence under
homogeneous synthetic support; their difference lives in natural anchor
heterogeneity).

| injection | engine (G) | vehicle (Gw) | financial (B) | tumor (B) |
|---|---|---|---|---|
| echo | .80→.97 PASS | .26→.89 PASS | abst→.00 KILL | abst→.00 KILL |
| mixed | .80→.47 PASS | .26→.46 PASS | abst→.37 PASS | abst→.37 PASS |
| cross | .80→.96 PASS | .26→.82 PASS | abst→.60 PASS | abst→.60 PASS |

Three contamination types, three signatures:

1. Frame echo is substrate, not poison: p=1 abstains (no shared anchors — the
   residual question is undefined without shared context), then BAD→0.0 while
   GOOD stays high. Echo creates the anchors AND preserves contradiction.
   Discriminative ✓.
2. Mixed-field collapses everything to ~0.4: GOOD falls, BAD rises,
   discrimination destroyed symmetrically. Information-destroying.
3. Cross-pair manufactures shared usage (→0.6–1.0): BAD passes at p=.5 with
   1.00. False-positive generator; destroys discrimination asymmetrically.

Payoff: purity is not a scalar dial — its TYPE decides. Residual must be
computed over frame-shared occurrences only, excluding mixed-field (directly
detectable: field contains both terms) and crossed occurrences (needs
bootstrapped basins — future work). I.e. judge the pair exactly where it
claims substitutability. The p=1 abstains validate the abstention rule again:
no shared context ⇒ unknown, never a verdict. ∂S/∂p differs by class only
under echo support — the one regime where the question is licensed.

## LT6-BB: bootstrapped basins (serving frozen; zero QPS calls)

Per-term occurrence graphs over window-set Jaccard (frozen channel; field /
segment / document strata inert by construction in the synthetic regime —
stated scope limit), components at predeclared TAU_E=0.20, drop rule at
DELTA=0.10 (drop X-comp iff sim(cent,profY) > sim(cent,profXrest)+DELTA),
frame occurrences exempt as licensed evidence. Synthetic corpora per pair:
12 frame + (12−k) pure + k crossed docs, k∈{0,6,12}; construction tags held
out from inference. Four-way ablation per cell: naive / basin / noframe /
both. Criterion (predeclared): BAD collapses to contradiction, GOOD preserved.

Result: criterion NOT met. S_basin ≡ S_naive everywhere that matters
(financial k=6: 1.00/1.00; engine: PASS throughout). Three localized causes:

1. Frame-echo dominance: whenever basin occs are dropped, only frame occs
   remain, and their identical {L,R} windows vote 1.0 on every non-frame
   anchor. Dropping crossed evidence leaves the echo in charge.
2. Noframe alone fails oppositely: crossed docs mirror pure docs
   distribution-identically → 1.00. Each exclusion removes one support class;
   with 3-token synthetic docs nothing licensed remains (both→abstain).
3. simX≡0 structurally: word-overlap grouping guarantees rest never contains
   the comp's characteristic words, so the margin rule degenerates to a simY
   threshold — and contaminated profiles (via Y-crossed mass) then false-drop
   pure comps (financial pure simY=0.29 DROP). The "closer to Y than X"
   comparison needs rest-sharing vocabulary (natural function words) to bite;
   predeclared design flaw, stated plainly.

What DID work: clustering recovers construction basins (pure=6/6 at
financial k=6 — crossed comps separate cleanly), and the drop rule fires on
exactly the crossed comps. Inference succeeds; authorization comparison is
what needs rest-sharing contexts. Non-monotonicity noted: fully-crossed (k=12)
trivially diverges (basin swap); the dangerous regime is partial crossing
(p≈.5), matching LT5. Serving line untouched; L4 behavior unchanged.

## LT7-NW: naturalized windows (serving frozen; zero QPS calls)

Bridge experiment: LT6 machinery frozen (TAU_E, DELTA, TAU_R, rules), only
occurrence construction changed — longer windows over a shared background
substrate (reported/observed/noted/widely) so rest-sharing is possible in
principle. Same 4 pairs, k∈{0,6,12}, ablation naive/basin/noframe/both plus
post-hoc basinF (frame profile as margin reference), same criterion
(BAD→contradiction, GOOD→shared, no tuning).

Findings:

1. simX≡0 in EVERY row, naturalized or not. Single-linkage grouping
   partitions vocabulary, so "rest" structurally cannot share the centroid's
   words (or rest is empty). The specified margin comparison cannot exist in
   this design family — not a data shortage. basinF coincides with basin
   everywhere. Fair trial: impossible as specified; the rule needs
   replacement, not richer data.
2. Criterion fails at frozen tau: BAD never KILLs (closest: financial k=12
   basin 0.69). But rank separation is clean at k=0 (BAD 0.42/0.42 vs GOOD
   0.58–0.92) and compresses monotonically with k; drops bite directionally
   (BAD 0.81→0.69 at k=12) while GOOD holds 0.88–0.98.
3. Threshold diagnosis: background substrate floors everyone ≥0.4, so the
   thin-regime tau=0.15 cannot gate here. Fix direction (not implemented):
   rarity-weighted (IDF — QPS already computes term_rarities) residual
   similarities, or regime-relative calibration. Ubiquitous context mass must
   be discounted before any absolute gate.
4. Clustering kept for the third time: k=12 splits crossed comps cleanly
   (kindpure-validated); chaining at k≤6 is realistic coarse behavior.

Status: inference layers all kept (nominate → phenotype → frame-licensed
residual with epistemic contract). Authorization comparison is the confirmed
broken layer: needs overlapping (soft) basin assignment so rest-references
exist, plus rarity-weighted similarities. Serving L4 undamaged — its evidence
(natural B/C, S1 microscope, mass-backed financial kill) is independent of
this branch.

## LT8-OS: overlapping support (serving frozen; zero QPS calls)

Factorial over frozen LT7 regime (same builder, k∈{0,6,12}, 4 pairs):
support {hard comps (control), soft affinities} × similarity {plain cosine,
IDF-cosine (QPS formula over base+cell df), IDF-Jaccard}. Arms A–E; drop iff
own-pull margin M<−DELTA (frozen); M_pair=min over judgeable comps; rank
geometry primary, frozen-tau verdicts secondary. Band diagnostic on D.
In-flight bug found and fixed before reading results: prototype/component
order misalignment leaked self-matches into S_X — post-fix numbers only.

1. Soft overlap gives simX>0 only where comps split (k=12; M defined).
   At k≤6 everything merges → abstention, which is informative (no foreign
   basin to judge). Fair trial partially achieved.
2. IDF does NOT restore separation: rank gaps preserved proportionally in
   every arm (k=0 BAD 0.42/0.42 vs GOOD 0.58/0.92 plain; 0.32/0.36 vs 0.49/0.88
   IDFcos), contamination compresses all arms alike, frozen tau gates nothing
   anywhere. Cosine vs Jaccard = same order, stricter scale (E lowest
   everywhere; E k=12 inverts above D on frame-identical aggregates, 0.75 vs
   0.53 — set-identity maximizes Jaccard while IDF-cosine discounts).
3. Margins do NOT order by label: k=12 vehicle(GOOD-weak) most foreign-pulled
   (−0.39), engine abstains, E margins collapse to ≈−0.05. Foreign-pull
   measures crossedness+rarity, not validity.
4. Band diagnostic refutes the hoped mechanism: common-band dominance is
   UNIVERSAL (57–100%), including goods (engine 86%). BAD is not specially
   common-propped. k=12 BAD bands hit common=1.00 — direct proof of
   echo-dominance in the residual numerator.

Synthesis: ordinal geometry (GOOD above BAD at every k in every arm) survives
all transformations — that is the residual signal's robust content. Absolute
gating at frozen thin-regime tau is regime-miscalibrated, not fixable by
weighting alone. Serving role confirmed as ranking/flagging (RB20) plus
query-scoped L4 raw-min on natural-regime evidence; no universal cutoff.

## Frozen architecture (post-LT8): residual ranks and flags; queries authorize

Residual output contract (no scalar accept/reject):

$$R(x,y) = (\mathrm{rank},\ \mathrm{contradiction\ flag},\ \mathrm{support\ state},\ \mathrm{phenotype})$$

Online decision stays conditional:

$$T(y\mid x,Q) = g(R(x,y),\ C(y\mid x,Q),\ \mathrm{anchors},\ \mathrm{authority})$$

Ledger — keep: nomination, phenotype, query conditioning, occurrence-level
purity selection, basin inference, residual ordering, query-scoped
authorization. Kill: universal residual threshold, foreign-pull margin as
validity, IDF as rescue. No LT9 residual transformation; next work (if any)
goes to calibration regimes or the serving composer, not the channel.
