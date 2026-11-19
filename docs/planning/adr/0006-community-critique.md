# ADR-0006: Community critique (beta, behind a flag)

- Status: accepted (2026-11-19), off by default (`COMMUNITY=on` enables it)
- Milestone: M9-T02

## Context

The grader is maths only (ADR-0002) and can't tell everything a person can.
We want human opinions without turning PencilPilot into a social network that
needs comment moderation.

## Decision

- **Opt-in sharing.** Only signed-in students can share, one drawing at a time,
  from the results page. Others never see the sharer's name or email.
- **No free text.** Judging is structured only:
  - *Which is closer?* Two drawings of the same reference; pick one. Each shared
    drawing keeps an Elo-style rating (K = 24). These pairwise votes are stored
    with both grader scores so they can feed grader calibration (#37).
  - *Rubric:* 1–5 for proportions, lines, light & dark, overall.
- **Safety first, fail closed.** Every shared drawing passes the ref-service's
  NSFW guard (`POST /api/moderate`, the same open_nsfw model that vets
  references) before it is stored. No model, an error or a timeout means the
  drawing is refused. This is the only automated look at a drawing, and it only
  answers "safe to show?", in line with ADR-0002.
- **Reports.** Three reports from different people hide a drawing for everyone
  but its owner.
- **Ownership.** Owners can delete a shared drawing at any time; deleting the
  account removes everything (foreign-key cascades). "Download my data"
  includes shared drawings and the judgements the person made.
- **Not public.** Shared drawings are served only to signed-in people
  (`/community/img/:id`, `cache-control: private`). `/api/moderate` is bound to
  localhost and not proxied to browsers.

## Consequences

- Needs accounts and the ref-service in production before it can be switched on.
- Rate limits: 20 shares a day, 300 judgements an hour per person.
- Moderation beyond the automatic filter (an admin review queue) is future work.
