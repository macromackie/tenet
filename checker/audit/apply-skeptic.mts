import { Option, Schema } from 'effect';
import type { Finding } from '../types.d.mts';

const VerdictLiteral = Schema.Literals(['confirm', 'reject']);

const SkepticReview = Schema.Struct({
  check: Schema.String,
  reason: Schema.String,
  verdict: VerdictLiteral,
});

type SkepticReview = typeof SkepticReview.Type;

const ReviewList = Schema.Array(SkepticReview);

const ReviewEnvelope = Schema.Struct({
  reviews: ReviewList,
});

const decodeReviews = Schema.decodeUnknownOption(ReviewEnvelope, { onExcessProperty: 'error' });

type ApplySkepticInput = Readonly<{
  findings: readonly Finding[];
  raw: unknown;
}>;

function buildRejectionLookup(reviews: readonly SkepticReview[]): ReadonlyMap<string, string> {
  const rejectedChecks = new Map<string, string>();

  for (const review of reviews) {
    const reason = review.reason.trim();

    if (review.verdict === 'reject' && reason.length > 0) {
      rejectedChecks.set(review.check, reason);
    }
  }

  return rejectedChecks;
}

/** Decode raw skeptic JSON verdicts and downgrade rejected failing witnesses to `unresolved`. */
export function applySkepticOutput({ findings, raw }: ApplySkepticInput): readonly Finding[] {
  const decoded = decodeReviews(raw);

  if (Option.isNone(decoded)) {
    return findings;
  }

  const rejectedChecks = buildRejectionLookup(decoded.value.reviews);

  return findings.map((row) => {
    if (row.outcome !== 'failed') {
      return row;
    }

    const reason = rejectedChecks.get(row.check);

    if (reason === undefined) {
      return row;
    }

    return {
      ...row,
      message: `dismissed by skeptic: ${reason} (${row.message})`,
      outcome: 'unresolved',
    };
  });
}
