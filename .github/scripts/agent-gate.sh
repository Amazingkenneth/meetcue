#!/usr/bin/env bash
# Decides what happens to an agent pull request after a CI run on its branch:
# merge it, hand it to a human, or ask Claude to fix the failure.
# Runs from the default branch (workflow_run), so a pull request can't change
# the rules that judge it.
set -euo pipefail

: "${HEAD_BRANCH:?}" "${HEAD_SHA:?}" "${CONCLUSION:?}" "${RUN_URL:?}" "${MERGE_TOKEN:?}"
MAX_FIX_ATTEMPTS=3
FIX_MARKER="<!-- agent-fix-attempt"

out() { echo "$1" >> "${GITHUB_OUTPUT:-/dev/null}"; }

pr_json=$(gh pr list --head "$HEAD_BRANCH" --state open --json number,headRefOid,labels --jq '.[0] // empty')
if [ -z "$pr_json" ]; then echo "No open pull request for $HEAD_BRANCH"; exit 0; fi
pr=$(jq -r .number <<<"$pr_json")
head=$(jq -r .headRefOid <<<"$pr_json")
labels=$(jq -r '[.labels[].name] | join(",")' <<<"$pr_json")
has_label() { [[ ",$labels," == *",$1,"* ]]; }

if [ "$head" != "$HEAD_SHA" ]; then echo "#$pr has newer commits; waiting for their CI"; exit 0; fi
if ! has_label agent; then echo "#$pr is not an agent pull request"; exit 0; fi
if has_label agent:hold; then echo "#$pr is on hold"; exit 0; fi
if has_label needs-review; then echo "#$pr is waiting for a human review"; exit 0; fi

case "$CONCLUSION" in
  success) ;;
  failure|timed_out)
    comments=$(gh pr view "$pr" --json comments --jq '[.comments[].body | select(startswith("'"$FIX_MARKER"'"))]')
    if jq -e --arg sha "$HEAD_SHA" 'any(.[]; contains($sha))' <<<"$comments" >/dev/null; then
      echo "A fix for $HEAD_SHA was already requested"; exit 0
    fi
    attempts=$(jq length <<<"$comments")
    if [ "$attempts" -ge "$MAX_FIX_ATTEMPTS" ]; then
      gh pr edit "$pr" --add-label needs-review
      gh pr comment "$pr" --body "CI still fails after $MAX_FIX_ATTEMPTS fix attempts ([latest run]($RUN_URL)), so this needs a human. Remove the \`needs-review\` label to let it merge once CI passes."
      exit 0
    fi
    gh pr comment "$pr" --body "$FIX_MARKER $HEAD_SHA -->
CI failed ([run]($RUN_URL)). Asking Claude to fix it, attempt $((attempts + 1)) of $MAX_FIX_ATTEMPTS."
    out "fix=true"
    out "pr=$pr"
    exit 0
    ;;
  *) echo "CI run ended as $CONCLUSION; nothing to do"; exit 0 ;;
esac

# Paths an agent may not change without a human looking first.
mapfile -t patterns < <(grep -vE '^\s*(#|$)' .github/agent-protected-paths.txt)
protected=()
while IFS= read -r file; do
  for pattern in "${patterns[@]}"; do
    # shellcheck disable=SC2053 # the pattern is meant to glob
    if [[ $file == $pattern ]]; then protected+=("$file"); break; fi
  done
done < <(gh pr diff "$pr" --name-only)
if [ "${#protected[@]}" -gt 0 ]; then
  list=$(printf -- '- `%s`\n' "${protected[@]}")
  gh pr edit "$pr" --add-label needs-review
  gh pr comment "$pr" --body "CI passed, but this changes protected files, so it waits for a review instead of merging itself:

$list

Merge it yourself once you're happy with it."
  exit 0
fi

# CI runs once for the push and once for the pull request; merge only when
# every check on this commit has finished and passed.
unfinished=$(gh pr view "$pr" --json statusCheckRollup --jq '[.statusCheckRollup[]
  | select(if .__typename == "CheckRun"
      then .status != "COMPLETED" or (.conclusion | IN("SUCCESS", "SKIPPED", "NEUTRAL") | not)
      else .state != "SUCCESS" end)
  | .name // .context] | join(", ")')
if [ -n "$unfinished" ]; then echo "Waiting on checks: $unfinished"; exit 0; fi

# A personal token, so the merge triggers the push workflows on the default branch.
GH_TOKEN="$MERGE_TOKEN" gh pr merge "$pr" --squash --delete-branch --match-head-commit "$HEAD_SHA"
echo "Merged #$pr"
