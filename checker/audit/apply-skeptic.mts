import { Option, Schema } from 'effect';
import type { Finding } from '../types.d.mts';
import { rejectUnverifiedWitnesses } from './reject-unverified.mts';

const SkepticVerdict = Schema.Literals(['critical', 'high', 'medium', 'reject']);

type SkepticVerdict = typeof SkepticVerdict.Type;

const SkepticReview = Schema.Struct({
  check: Schema.String,
  reason: Schema.String,
  verdict: SkepticVerdict,
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

type FormatFindingInput = Readonly<{
  review?: SkepticReview;
  row: Finding;
}>;

type FormatMessageInput = Readonly<{
  message: string;
  verdict: SkepticVerdict;
}>;

type RejectWitnessInput = Readonly<{
  reason: string;
  row: Finding;
}>;

type SeverityBuckets = Readonly<{
  critical: readonly Finding[];
  high: readonly Finding[];
  medium: readonly Finding[];
  other: readonly Finding[];
  unranked: readonly Finding[];
}>;

type AppendFindingInput = Readonly<{
  buckets: SeverityBuckets;
  finding: Finding;
  review?: SkepticReview;
}>;

function buildReviewLookup(reviews: readonly SkepticReview[]): ReadonlyMap<string, SkepticReview> {
  const checkedReviews = new Map<string, SkepticReview>();

  for (const review of reviews) {
    checkedReviews.set(review.check, review);
  }

  return checkedReviews;
}

function formatSeverityMessage({ message, verdict }: FormatMessageInput): string {
  const prefix = `[${verdict}] `;

  if (message.startsWith(prefix)) {
    return message;
  }

  return `${prefix}${message}`;
}

function formatRejectedWitness({ reason, row }: RejectWitnessInput): Finding {
  return {
    ...row,
    message: `unverified brownfield witness downgraded: ${reason} (${row.message})`,
    outcome: 'unresolved',
  };
}

function applyUnverifiedReview({ review, row }: FormatFindingInput): Finding {
  if (review === undefined) {
    return formatRejectedWitness({ reason: 'skeptic omitted verdict', row });
  }

  if (review.verdict === 'reject') {
    const reason = review.reason.trim();

    if (reason.length > 0) {
      return {
        ...row,
        message: `dismissed by skeptic: ${reason} (${row.message})`,
        outcome: 'unresolved',
      };
    }

    return formatRejectedWitness({ reason: 'rejected by skeptic', row });
  }

  return {
    ...row,
    message: formatSeverityMessage({ message: row.message, verdict: review.verdict }),
    severity: review.verdict,
  };
}

function applyVerifiedReview({ review, row }: FormatFindingInput): Finding {
  if (review === undefined) {
    return row;
  }

  const reason = review.reason.trim();

  if (review.verdict === 'reject' && reason.length > 0) {
    return {
      ...row,
      message: `dismissed by skeptic: ${reason} (${row.message})`,
      outcome: 'unresolved',
    };
  }

  if (review.verdict === 'reject') {
    return row;
  }

  return {
    ...row,
    message: formatSeverityMessage({ message: row.message, verdict: review.verdict }),
    severity: review.verdict,
  };
}

function applyReview({ review, row }: FormatFindingInput): Finding {
  if (row.outcome !== 'failed') {
    return row;
  }

  if (row.brownfield_unverified === true) {
    return applyUnverifiedReview({ review, row });
  }

  return applyVerifiedReview({ review, row });
}

function appendFinding({ buckets, finding, review }: AppendFindingInput): SeverityBuckets {
  if (finding.outcome !== 'failed') {
    return { ...buckets, other: [...buckets.other, finding] };
  }

  if (review === undefined || review.verdict === 'reject') {
    return { ...buckets, unranked: [...buckets.unranked, finding] };
  }

  return { ...buckets, [review.verdict]: [...buckets[review.verdict], finding] };
}

/** Decode skeptic verdicts, dismiss false positives, tag severities, and sort highest severity first. */
export function applySkepticOutput({ findings, raw }: ApplySkepticInput): readonly Finding[] {
  const decoded = decodeReviews(raw);

  if (Option.isNone(decoded)) {
    return rejectUnverifiedWitnesses(findings);
  }

  const reviewLookup = buildReviewLookup(decoded.value.reviews);

  let buckets: SeverityBuckets = { critical: [], high: [], medium: [], other: [], unranked: [] };

  for (const row of findings) {
    const review = reviewLookup.get(row.check);
    const finding = applyReview({ review, row });

    buckets = appendFinding({ buckets, finding, review });
  }

  return [
    ...buckets.critical,
    ...buckets.high,
    ...buckets.medium,
    ...buckets.unranked,
    ...buckets.other,
  ];
}
