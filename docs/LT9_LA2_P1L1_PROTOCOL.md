# LT9-LA2-P1L1: LA2-B integration-failure anatomy

P1L1 freezes LA2-B HotpotQA replay as discovery evidence after the B2 memory
gate recorded 36 invalid routed episodes and two invalid authority
compartments. It is diagnostic only: no routing, credit, authority, ranking, or
serving state may be changed by this experiment.

## Inputs

- the frozen HotpotQA `corpus.jsonl` used by LA2-B;
- the sealed LA2-B receipt;
- the sealed P1K2 discovery receipt;
- the sealed P1K4 qualification receipt.

P1L1 reconstructs LA2-B's event and episode contract exactly: nearest source
and target occurrence, 24-token eligibility, candidate-local first-later
witness pairing, and episode ordering by nomination document. It uses the
qualified B2 pair route without changing it.

## Required evidence

For every invalid B2 episode, and for matched valid controls, the receipt
records the candidate, route and route path, expected phenotype, plurality
counts, max and runner-up counts, margins, tie sets, fixed-priority and raw
argmax assignments, priority inversion, mixed-marker status, marker masks,
before/after side masks, field, candidate-opportunity delay, witness polarity,
and the resulting route phenotype.

Route paths are classified as `UNIQUE_PLURALITY`, `TIE_RESOLVED`,
`TIE_ABSTAINED`, or `UNIQUE_CONFLICT_ABSTAINED`.

The receipt groups invalid episodes by candidate, route path, resulting
phenotype, and endpoint count signature. It maps every invalid episode routed
to a state with positive B2 authority, preserving the two contaminated
compartment histories as the authority-level hazard.

## P1K4 parity

Shared candidate/document endpoint keys are compared against P1K2 and P1K4.
P1K2's `plurality_outcome` is intentionally compared as a contract-difference
diagnostic: it is the raw plurality outcome, while B2 applies the frozen
unique-endpoint agreement route. P1K4 route decisions are expected to match on
shared keys. The first mismatch is recorded without treating it as a repair.

## Decision boundary

P1L1 cannot qualify a repair. The LA2-B replay remains a discovery boundary.
The next experiment must be selected from the failure anatomy: episode-contract
parity, a concentrated unique-plurality defect, a tie-resolver defect, or a
broader phenotype-representation defect. Retrieval evaluation remains dark.
