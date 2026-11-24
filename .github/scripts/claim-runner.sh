#!/usr/bin/env bash
# Decides whether this leg of a macOS job does the work. A macOS job has two legs from one
# definition: `hosted` on GitHub's runner and `local` on the maintainer's own Mac
# (docs/operations/20261101-ci_release.md#local_runner). GitHub's arm64 Macs can be out of capacity
# for longer than GitHub keeps a job queued, so:
#
#   local   starts at once and gives the hosted leg five minutes to get a machine. If one arrives
#           it stands down; otherwise it takes the job over (the step named "Take over").
#   hosted  works when its machine arrives, unless the local leg has taken the job over by then.
#
# Usage: claim-runner.sh <hosted|local> <name of the other leg's job>   (needs GH_TOKEN)
# Writes `work=true|false` to $GITHUB_OUTPUT; every later step of the job runs only when it is true.
set -euo pipefail
me="$1" other="$2"
# The jobs of this attempt of this run, through a jq filter.
jobs() {
  gh api "repos/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}/attempts/${GITHUB_RUN_ATTEMPT}/jobs?per_page=100" --jq "$1"
}
work=true
if [ "$me" = hosted ]; then
  took="$(jobs ".jobs[] | select(.name == \"${other}\") | .steps[]? | select(.name == \"Take over\" and .conclusion == \"success\") | .name")"
  if [ -n "$took" ]; then
    echo "The local runner took this job over while no hosted machine was available: nothing to do here."
    work=false
  fi
else
  for _ in $(seq 1 30); do
    state="$(jobs ".jobs[] | select(.name == \"${other}\") | \"\(.status) \(.conclusion)\"")"
    # No hosted leg in this run (the release tried on the local runner alone): nothing to wait for.
    [ -n "$state" ] || break
    case "$state" in
      in_progress*|"completed success"|"completed failure")
        echo "A hosted machine took the job (${state}): the local runner stands down."
        work=false
        break
        ;;
    esac
    sleep 10
  done
  [ "$work" = true ] && echo "No hosted machine (${state:-this run has no hosted leg}): the local runner takes over."
fi
echo "work=${work}" >>"$GITHUB_OUTPUT"
