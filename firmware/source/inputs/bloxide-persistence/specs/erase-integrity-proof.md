# F1 diagnosis and proposed correction — proof before layout

Written after reproducing the original witness and before editing the candidate
layout. This is a conditional mathematical design argument, not product, physical
power-loss, ECC, allocation or AM13 qualification. Independent re-review must
precede any I1-I4 release.

## The defect and retained negative control

The unchanged reviewer programs under tests/original-f1/probes produce the exact
512-byte A(seq7,epoch1,rev10,2000/250), B(seq6,epoch1,rev9,1000/500), intended
new(seq8,epoch1,rev11,3000/750), torn B and mask. Original old/new assertion exits1.
In B, offsets16/24/25/26/27/60 gain masks08/54/b7/92/a3/04: 18 zero-to-one
changes. Sequence becomes14, epoch2744301397, stored CRC8e4858b3 becomes8e4858b7,
matching the changed data. Commit remains zero, A untouched. This is not an equal
CRC collision and must remain an admitted erase transition. Enlarging CRC cannot
establish absence of accepted codeword substitutions under erase.

## Selected single design and explicit model

Retain two data slots and isolated final commit granule. Encode every bit of the
entire fixed logical body, INCLUDING magic/version/schema/length/sequence, source
labels, operation identity, reserved bytes, stored CRC, payload and logical FF
padding, as a complementary pair. Logical byte d is stored as adjacent bytes
[d, d XOR FF]. Thus each of 2560 bit pairs is 01 or10; 00 and11 are invalid.
The 320 logical bytes become640 physical bytes. Physical alignment padding and
tail remain FF; the separate commit granule remains exactly zero. No extra
pre-erase authority/journal or unbounded metadata is introduced. Existing finite
wear authority remains solely wear/quarantine authority, not a selection oracle.

Model M (all are UNQUALIFIED T16 backend obligations):

- Healthy completed bits are stable between commands and after quiescence.
  Erase only changes an arbitrary subset of addressed-unit zero bits to one,
  including checksum and marker; no order/progress/atomicity assumption.
- Program starts after full verified erase and only changes an arbitrary subset
  of the requested one-to-zero transitions in its addressed G-byte granule.
  Completed granules and the other slot remain untouched. Successful operations
  settle at their specified values. Marker is isolated, programmed once and last,
  after full exact body verification. No partial program retry.
- Reads are coherent after proved idle; corrected/uncorrectable ECC is explicit
  and prevents write qualification. Transparent ECC/coupled cells/overprogram,
  nonmonotone erase or broader disturbance are NOT asserted absent on AM13.
- The controller reads the full inactive prestate before erase and checks Q below;
  no other writer or unmodeled change can intervene. A read/error/unsafe Q denies
  that erase, preserving RAM/capture/current media. Ordinary Reset cannot override Q.

The original witness lies inside M; corrected design must reject its promotion,
not exclude it. There is no checksum-collision exception to M's all-cut theorem.

## Why independently invalid records need a pre-erase check

An arbitrary pre-existing record is not necessarily an interrupted valid record.
For example, clear both rails of one pair in an otherwise valid high-sequence
record: pair00 is invalid, but erase can restore either01 or10. Or damage an FF
physical tail byte of an otherwise accepted high-sequence record: erase can repair
that tail. An unconditional erase of either invalid prestate could promote it.

Bounded predicate Q uses one complete healthy scan of the target before acquiring
an erase permit. It accepts exactly the union of these sufficient conditions:

Q1. At least one marker bit is1. Erase cannot make this exact-zero marker.
Q2. At least one body bit pair is11. Erase cannot make this pair complementary.
Q3. All body pairs are complementary, physical alignment padding and tail are FF,
    marker is zero; normal R3 classification is Valid or Corrupt. An Unsupported
    record anywhere locks all writes. A Valid target must already be subordinate
    to the selected current record (lower sequence, or exact duplicate). Any R3
    ambiguity, selected-slot change, read error or unreconciled quarantine denies.

Q3's Corrupt includes bad logical magic, CRC, length, zero schema/sequence, reserved
or logical padding, or invalid registered semantic values. Erase can either leave
this body unchanged and invalid, or break a complementary pair. It cannot repair
it into any accepted supported OR unsupported envelope. Q1/Q2 take precedence over
attempting semantic decode of malformed bodies. All healthy blank slots satisfy
Q1/Q2. Any other prestate is UnsafeErasePrestate: no erase, Failed(NoNewCommit),
write lock pending separately reviewed reprovisioning. This is concrete refinement
of the existing backend recovery authorization, not permission to discard F1.

Q does not by itself clear quarantine or grant a wear permit. Q passes even some
arbitrarily invalid prestates, because its sufficient conditions prove their
entire erase closure cannot acquire new selection authority. There is no claim
that every arbitrary corrupted state can safely be reused. No in-place repair,
pre-erase invalidation marker, or software-only waiver of Q is allowed.

## Proof over arbitrary subsets, not enumeration

Order physical bit strings componentwise with 0<1. Every complementary body has
exactly2560 ones. If two such bodies c<=d, equal weight implies c=d. Thus the set
of all syntactically complementary bodies is an antichain, independently of CRC,
semantics, field values, or future common-envelope version. Any nonempty erase
change to such a body creates11 and can never reach a distinct complementary body.
A zero marker may remain zero or become inexact; an inexact marker cannot become
zero during erase. FF padding/tail cannot change during modeled erase.

For Q1/Q2 the persistent one/11 witness excludes every accepted post-erase record.
For Q3, every accepted post-erase record must have precisely the same body and
marker, padding and tail as before erase. Valid predecessor remains the same
subordinate record; Corrupt remains Corrupt. No stale promotion, new unknown
version/schema dominance or duplicate-sequence ambiguity can be created by erase.
This includes changing the stored CRC itself and changing only the marker.

After a fully verified erase, program progress toward one fixed body c has, at
each pair, either11 or the final01/10. If the body is complementary at any partial
step, it equals c. Marker is still FF throughout body programming. Body verification
precedes any marker transition; marker programming cannot change the body. Hence
an exact-zero marker implies the exact captured new body. Whether the completion
arrives has no bearing on this implication. During marker programming the record
is either uncommitted or exactly new. Tail remains FF.

Consequently, for a save admitted with selected valid old record O and Q-qualified
inactive prestate, O is untouched and recovery after EVERY permitted cut selects O
or exact new N with checked sequence O.seq+1. Blank start selects validated defaults
or N(seq1). Read/receipt cuts change no media. Duplicate identical old records have
the same O. A genuinely Unsupported/ambiguous start admits no save. Sequence MAX
admits no changed save; DurableExisting is write-free. There is no modular wrap.

Induction: partial erase from Q1/Q2 retains its witness; partial erase from Q3 either
retains the old body or gains11/nonzero marker. Partial body programming has FF
marker, and partial commit has nonzero marker or exact new. Thus every reachable
interrupted state is Q-eligible on re-scan (subject separately to backend recovery,
quiescence, wear/quarantine and supported/unambiguous recovery). The argument works
with slots exchanged, after multiple interrupted saves, and from healthy blank
media. It needs no persistent intent and cannot consume unbounded receipt space.

## Corruption policy and actual limits

Single detectable corruption yields explicit Corrupt/Unreadable/fallback/default
status per R3. Unsupported and equal-sequence ambiguity remain read-only. Q is
required even when a backend administratively authorizes recovery; independently
invalid00 and damaged-tail examples must never be erased without Q.

Arbitrary nonmonotone disturbances can replace any finite accepted word by another,
including its redundancy/checksum. No finite encoding can detect all such errors
while accepting both configurations. This is an information limit, not a CRC-size
choice. Detected physical errors follow R3/R6; an undetected valid-codeword replacement
is a residual physical-integrity risk outside M, never evidence for old/new safety.
No claim of a residual numerical failure probability is made. Both-slot arbitrary
damage, unreported ECC transformations and isolation failures require separate
physical qualification/policy; they are not simulated-cut successes.

If the original all-cut requirement were intended to include unconstrained
nonmonotone replacement with no trustworthy external authority, it is impossible
for this or any finite local encoding. The explicit model defines that narrow
interpretation and its qualification boundary; this design retains the
all-cut requirement over explicit M, including the reported witness, and does not
change accepted S0/T17. T16 must either establish M, provide a separately reviewed
backend guarantee, or leave persistence read-only. No hardware fact is inferred.
