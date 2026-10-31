"""Untrustworthy clone policies, reports, and suppression directives."""


class CloneRejected(ValueError):
    """The clone check cannot accept the declared source or its detector result."""
