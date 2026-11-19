/** Render a failure at the operation that made its evidence unavailable. */
export function readReason(error: unknown): string {
  if (error instanceof SyntaxError) {
    return 'The checker response was not valid JSON.';
  }

  if (error instanceof Error) {
    return error.message;
  }

  return 'Could not complete the check.';
}
